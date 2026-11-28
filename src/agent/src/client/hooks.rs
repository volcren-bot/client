use gl::{CURRENT_PROGRAM, VERTEX_ARRAY_BINDING};
use retour::GenericDetour;
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicU8, Ordering},
        Mutex, OnceLock,
    },
};

use windows::{
    core::{BOOL, PCSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Gdi::HDC,
            OpenGL::{glGetIntegerv, GL_VIEWPORT},
        },
        System::{
            Console::{AllocConsole, AttachConsole, GetConsoleWindow, ATTACH_PARENT_PROCESS},
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
            Threading::GetCurrentProcessId,
        },
        UI::{
            Input::KeyboardAndMouse::{
                GetKeyboardState, MapVirtualKeyA, MapVirtualKeyW, ToUnicode, MAPVK_VK_TO_VSC,
            },
            WindowsAndMessaging::{
                CallWindowProcW, GetClientRect, GetWindowLongPtrW, IsWindowVisible,
                SetWindowLongPtrW, ShowWindow, GWLP_WNDPROC, SW_HIDE, SW_SHOWNORMAL, WM_CHAR,
                WM_IME_NOTIFY, WM_IME_SETCONTEXT, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_MOUSEMOVE,
                WNDPROC,
            },
        },
    },
};

use super::graphics::{
    draw_cursor, draw_overlay, load_opengl_extensions, read_frame, release_frame_buffers,
    restore_state,
};
use super::offsets::{
    find_actor_lists, find_actor_names, find_actor_path, find_actor_position,
    find_animation_height, find_appearance_decoder, find_camera, find_chat, find_client,
    find_collision_maps, find_component_fields, find_component_invalidate, find_component_sprite,
    find_decoration_layout, find_destination, find_frame, find_game_object_layout, find_game_state,
    find_ground_item_ownership, find_ground_items, find_hider, find_instance_flag, find_interfaces,
    find_item_containers, find_item_icon, find_item_pile, find_item_setter, find_loc_configs,
    find_local_player, find_local_projection, find_login_panel, find_login_provider, find_menu,
    find_menu_click, find_menu_layout, find_menu_open, find_menu_order, find_menu_settings,
    find_minimap, find_model_faces, find_model_vertices, find_npc_head_icons, find_npc_model,
    find_packet_layouts, find_packet_names, find_packets, find_projectiles, find_projection,
    find_render, find_run_energy, find_scene, find_script_runs, find_selection, find_skills,
    find_spot_anims, find_tick_count, find_varc_setters, find_varcs, find_varps, find_world_select,
    guarded, NamedPacket, PacketAnchors, Pe, ScanError, World,
};
use super::state::{LayoutMap, LAYOUT_MAP};
use super::unload::HookCall;
use crate::shared::{
    memory::{memory_manager, try_memory_manager, MemoryManager, MEMORY_MANAGER},
    sync::Events,
};
use ipc::input::{
    ACT_MESSAGE, CACHE_MESSAGE, MODIFIERS_MESSAGE, NAME_MESSAGE, OPEN_CONSOLE_MESSAGE,
    TOTAL_XP_MESSAGE,
};
use ipc::packets::Name;
use ipc::state::{
    part, CameraLayout, ChatLayout, CollisionLayout, GroundItemsLayout, InterfaceLayout, Layout,
    LoginLayout, MinimapLayout, PathLayout, PlayerLayout, ProjectileLayout, ProjectionLayout,
    SceneLoaderLayout, SkillsLayout, SpotAnimLayout, WorldLayout, WorldSelectLayout,
};
use ipc::{cursor_style, image_fits, overlay_mode};

const MODIFIER_KEYS: [u8; 3] = [0x10, 0x11, 0x12];
static KEYBOARD_MODIFIERS: Mutex<[bool; 3]> = Mutex::new([false; 3]);
static LAST_CHAR: AtomicU8 = AtomicU8::new(0);
static OVERLAY_OWNER: Mutex<super::writer::Owner> = Mutex::new(super::writer::Owner::new());

unsafe fn open_client_console() {
    let hwnd = GetConsoleWindow();
    if !hwnd.0.is_null() {
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            return;
        }

        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
    }
    if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
        let _ = AllocConsole();
    }
}

static HOOKED_HWND: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static WNDPROC_HOOKED: AtomicBool = AtomicBool::new(false);

pub fn hooked_window() -> isize {
    HOOKED_HWND.load(Ordering::SeqCst)
}

fn hooked_wndproc_address() -> isize {
    hooked_wndproc as *const () as isize
}

unsafe fn call_original(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let original: WNDPROC = std::mem::transmute(ORIGINAL_WNDPROC.load(Ordering::SeqCst));
    CallWindowProcW(original, hwnd, msg, wparam, lparam)
}

fn modifier_lparam(key: u8, down: bool) -> LPARAM {
    let scancode = unsafe { MapVirtualKeyA(key as u32, MAPVK_VK_TO_VSC) };
    let mut lparam = 1 | (scancode << 16);
    if !down {
        lparam |= (1 << 30) | (1 << 31);
    }
    LPARAM(lparam as isize)
}

unsafe fn toggle_modifiers(hwnd: HWND, wparam: WPARAM) {
    let mut held = KEYBOARD_MODIFIERS.lock().unwrap();
    for (bit, (&key, held)) in MODIFIER_KEYS.iter().zip(held.iter_mut()).enumerate() {
        if wparam.0 & (1 << bit) != 0 {
            let msg = if *held { WM_KEYUP } else { WM_KEYDOWN };
            let _ = call_original(
                hwnd,
                msg,
                WPARAM(key as usize),
                modifier_lparam(key, !*held),
            );
            *held = !*held;
        }
    }
}

unsafe fn modified_char(vkey: u8, held: [bool; 3]) -> WPARAM {
    let scancode = MapVirtualKeyW(vkey as u32, MAPVK_VK_TO_VSC);
    let mut keyboard_state = [0u8; 256];
    let _ = GetKeyboardState(&mut keyboard_state);
    keyboard_state[vkey as usize] = 0x80;
    for (&key, _) in MODIFIER_KEYS.iter().zip(held).filter(|&(_, held)| held) {
        keyboard_state[key as usize] = 0x80;
    }
    let mut buff = [0u16; 4];
    let _ = ToUnicode(vkey as u32, scancode, Some(&keyboard_state), &mut buff, 0);
    WPARAM(char::from_u32(buff[0] as u32).map_or(0, |c| c as usize))
}

unsafe extern "system" fn hooked_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let _call = HookCall::enter();
    catch_unwind(AssertUnwindSafe(|| wndproc(hwnd, msg, wparam, lparam))).unwrap_or(LRESULT(0))
}

static LAST_SYNTHETIC_MOUSE: AtomicU64 = AtomicU64::new(0);

fn filter_mouse(msg: u32, wparam: WPARAM) -> Option<WPARAM> {
    use ipc::input::{MOUSE_FIRST, MOUSE_LAST, REAL_MOUSE_BLOCK_MS, SYNTHETIC};
    if !(MOUSE_FIRST..=MOUSE_LAST).contains(&msg) {
        return Some(wparam);
    }
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let now = START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
        + 1;
    if wparam.0 as u64 & SYNTHETIC != 0 {
        LAST_SYNTHETIC_MOUSE.store(now, Ordering::Relaxed);
        return Some(WPARAM((wparam.0 as u64 & !SYNTHETIC) as usize));
    }
    let last = LAST_SYNTHETIC_MOUSE.load(Ordering::Relaxed);
    (last == 0 || now.saturating_sub(last) > REAL_MOUSE_BLOCK_MS).then_some(wparam)
}

unsafe fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(wparam) = filter_mouse(msg, wparam) else {
        return LRESULT(0);
    };
    match msg {
        OPEN_CONSOLE_MESSAGE => open_client_console(),
        MODIFIERS_MESSAGE => toggle_modifiers(hwnd, wparam),
        NAME_MESSAGE => super::name::on_request(),
        TOTAL_XP_MESSAGE => super::total_xp::on_request(),
        CACHE_MESSAGE => super::cache::on_request(),
        ACT_MESSAGE => super::act::on_request(),
        WM_KILLFOCUS | WM_IME_SETCONTEXT | WM_IME_NOTIFY => {}
        WM_KEYDOWN => {
            if KEYBOARD_MODIFIERS.lock().unwrap().contains(&true) {
                LAST_CHAR.store(wparam.0 as u8, Ordering::Relaxed);
            }
            return call_original(hwnd, msg, wparam, lparam);
        }
        WM_CHAR => {
            let held = *KEYBOARD_MODIFIERS.lock().unwrap();
            let wparam = if held.contains(&true) {
                modified_char(LAST_CHAR.load(Ordering::Relaxed), held)
            } else {
                wparam
            };
            return call_original(hwnd, msg, wparam, lparam);
        }
        WM_MOUSEMOVE => {
            let (x, y) = (lparam.0 as u16 as i32, (lparam.0 >> 16) as u16 as i32);
            memory_manager().set_mouse_position(x, y);
            return call_original(hwnd, msg, wparam, lparam);
        }
        _ => return call_original(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

unsafe fn hook_wndproc(hwnd: u64) {
    let hwnd = HWND(hwnd as *mut c_void);
    HOOKED_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

    let current = GetWindowLongPtrW(hwnd, GWLP_WNDPROC) as isize;
    if current == hooked_wndproc_address() {
        WNDPROC_HOOKED.store(true, Ordering::SeqCst);
        return;
    }

    ORIGINAL_WNDPROC.store(current, Ordering::SeqCst);
    let previous = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, hooked_wndproc_address() as _) as isize;
    if previous == 0 {
        panic!("[Agent]: Failed to hook WndProc.\r\n");
    }
    ORIGINAL_WNDPROC.store(previous, Ordering::SeqCst);
    WNDPROC_HOOKED.store(true, Ordering::SeqCst);

    println!("[Agent]: WndProc successfully hooked.\r\n");
}

unsafe fn wndproc_is_ours() -> bool {
    let hwnd = HWND(HOOKED_HWND.load(Ordering::SeqCst) as *mut c_void);
    GetWindowLongPtrW(hwnd, GWLP_WNDPROC) as isize == hooked_wndproc_address()
}

pub unsafe fn wndproc_can_be_restored() -> bool {
    !WNDPROC_HOOKED.load(Ordering::SeqCst) || wndproc_is_ours()
}

unsafe fn unhook_wndproc() {
    if !wndproc_is_ours() {
        return;
    }
    let hwnd = HWND(HOOKED_HWND.load(Ordering::SeqCst) as *mut c_void);

    SetWindowLongPtrW(
        hwnd,
        GWLP_WNDPROC,
        ORIGINAL_WNDPROC.load(Ordering::SeqCst) as _,
    );
    WNDPROC_HOOKED.store(false, Ordering::SeqCst);
    println!("[Agent]: WndProc successfully unhooked.\r\n");
}

static ORIGINAL_WGL_SWAPBUFFERS: OnceLock<GenericDetour<unsafe extern "system" fn(HDC) -> BOOL>> =
    OnceLock::new();

unsafe extern "system" fn hooked_wgl_swap_buffers(hdc: HDC) -> BOOL {
    let _call = HookCall::enter();
    let _ = catch_unwind(|| draw_frame());
    let presented = ORIGINAL_WGL_SWAPBUFFERS.get().unwrap().call(hdc);
    if let Some(memory) = try_memory_manager() {
        memory.frame_presented();
    }
    presented
}

unsafe fn draw_frame() {
    let mut viewport = [0; 4];
    glGetIntegerv(GL_VIEWPORT, viewport.as_mut_ptr());
    let mut prev_program: i32 = 0;
    let mut prev_vao: i32 = 0;
    glGetIntegerv(CURRENT_PROGRAM, &mut prev_program);
    glGetIntegerv(VERTEX_ARRAY_BINDING, &mut prev_vao);
    let [offset_x, offset_y, width, height] = viewport;

    let mut client_rect = RECT::default();
    let hwnd = HWND(HOOKED_HWND.load(Ordering::SeqCst) as *mut c_void);
    let _ = GetClientRect(hwnd, &mut client_rect);
    let window_height = client_rect.bottom - client_rect.top;
    let offset_y_top_down = window_height - offset_y - height;

    super::frame::set_viewport_offset(offset_x, offset_y_top_down);
    if !super::frame::active() {
        super::frame::on_frame((offset_x, offset_y_top_down));
    }

    let fits = image_fits(width, height);
    let (mouse, dest, overlay, (cursor, cursor_color, cursor_size)) = {
        let mem_manager = memory_manager();
        mem_manager.set_viewport(offset_x, offset_y, width, height, window_height);
        let overlay = (mem_manager.overlay_mode() != overlay_mode::OFF
            && OVERLAY_OWNER
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .alive(mem_manager.overlay_writer()))
        .then(|| {
                let serial = mem_manager.overlay_serial();
                let slot = serial as usize & 1;
                let size = mem_manager.overlay_size(slot);
                (mem_manager.overlay_ptr(slot), size, serial)
            })
            .filter(|&(_, [painted_width, painted_height], _)| {
                image_fits(painted_width, painted_height)
            });
        let dest = (fits && mem_manager.capture_enabled()).then(|| mem_manager.image_ptr());
        (
            mem_manager.get_mouse_position(),
            dest,
            overlay,
            mem_manager.cursor(),
        )
    };

    if !load_opengl_extensions() {
        return;
    }
    let _ = catch_unwind(|| {
        match dest {
            Some(dest) => read_frame(offset_x, offset_y, width, height, dest),
            None => release_frame_buffers(),
        }

        let world = super::world_shapes::refresh();
        super::interface_mask::want((overlay.is_some() || world).then_some([width, height]));
        if world {
            super::interface_mask::with_current(|mask| super::world_shapes::draw(viewport, mask));
        }
        if let Some((overlay, size, serial)) = overlay {
            super::interface_mask::with_current(|mask| {
                draw_overlay(viewport, overlay, size, serial, mask)
            });
        }

        if cursor != cursor_style::OFF && mouse.0 > -1 && mouse.1 > -1 {
            let (mouse_x, mouse_y) = (mouse.0 - offset_x, mouse.1 - offset_y_top_down);
            if mouse_x > -1 && mouse_y > -1 && mouse_x < width && mouse_y < height {
                draw_cursor(
                    mouse_x,
                    mouse_y,
                    width,
                    height,
                    cursor,
                    cursor_color,
                    cursor_size,
                );
            }
        }
    });
    restore_state(prev_program, prev_vao);
}

unsafe fn hook_wgl_swap_buffers() {
    let module = GetModuleHandleA(PCSTR(c"opengl32.dll".as_ptr().cast()))
        .expect("[Agent]: opengl32.dll module not found.\r\n");

    let addr = GetProcAddress(module, PCSTR(c"wglSwapBuffers".as_ptr().cast()))
        .expect("[Agent]: wglSwapBuffers function not found.\r\n");

    let original_fn: unsafe extern "system" fn(HDC) -> BOOL = std::mem::transmute(addr);
    ORIGINAL_WGL_SWAPBUFFERS
        .get_or_init(|| {
            GenericDetour::new(original_fn, hooked_wgl_swap_buffers)
                .expect("[Agent]: Failed to create wglSwapBuffers hook.\r\n")
        })
        .enable()
        .expect("[Agent]: Failed to enable wglSwapBuffers hook.\r\n");

    println!("[Agent]: wglSwapBuffers successfully hooked.\r\n");
}

unsafe fn unhook_wgl_swap_buffers() {
    if let Some(memory) = try_memory_manager() {
        memory.clear_overlay();
    }

    let Some(detour) = ORIGINAL_WGL_SWAPBUFFERS.get() else {
        return;
    };
    if !detour.is_enabled() {
        return;
    }

    match detour.disable() {
        Ok(()) => println!("[Agent]: wglSwapBuffers successfully unhooked.\r\n"),
        Err(error) => println!("[Agent]: Failed to disable wglSwapBuffers hook: {error}.\r\n"),
    }
}

pub unsafe fn unhook_all() {
    super::proxy::unhook();
    super::focus::unhook();
    unhook_wndproc();
    unhook_wgl_swap_buffers();
    super::frame::unhook();
    super::name::unhook_decoder();
    super::total_xp::unhook();
    super::packets::unhook();
    super::script_runs::unhook();
    super::varc_log::unhook();
    super::item_log::unhook();
    super::drawn::unhook();
}

pub(super) unsafe fn client_image<'a>() -> Option<&'a [u8]> {
    let base = GetModuleHandleA(PCSTR::null()).ok()?.0 as *const u8;
    let e_lfanew = (base.add(0x3c) as *const u32).read_unaligned() as usize;
    let size_of_image = (base.add(e_lfanew + 24 + 56) as *const u32).read_unaligned() as usize;
    Some(std::slice::from_raw_parts(base, size_of_image))
}

fn found<T: std::fmt::Debug>(name: &str, result: Result<T, ScanError>) -> Option<T> {
    match result {
        Ok(value) => {
            println!("[Agent]: {name} found: {value:x?}.\r\n");
            Some(value)
        }
        Err(error) => {
            println!("[Agent]: {name} not found: {error:?}.\r\n");
            None
        }
    }
}

unsafe fn find_offsets() {
    let set_found = |found: bool| memory_manager().set_offsets_found(found);
    let Some(image) = client_image() else {
        println!("[Agent]: Client image not found.\r\n");
        set_found(false);
        return;
    };
    let Some(pe) = Pe::parse(image) else {
        println!("[Agent]: Client image isn't a 64-bit PE.\r\n");
        set_found(false);
        return;
    };
    let pe = &pe;

    let actor = found("Actor position", guarded(|| find_actor_position(pe)));
    let path = actor.and_then(|a| found("Actor path", guarded(|| find_actor_path(pe, a.x))));
    let player = found("Local player", guarded(|| find_local_player(pe)));
    let game_state = found("Game state", guarded(|| find_game_state(pe)));
    let client = game_state.and_then(|state| found("Client", guarded(|| find_client(pe, state))));
    let skills = client.and_then(|c| found("Skills", guarded(|| find_skills(pe, c))));
    let tick_count = client.and_then(|c| found("Tick count", guarded(|| find_tick_count(pe, c))));
    let varps = found("Varps", guarded(|| find_varps(pe)));
    let login = game_state
        .and_then(|offset| found("Login panel", guarded(|| find_login_panel(pe, offset))));
    let login_provider =
        client.and_then(|c| found("Login provider", guarded(|| find_login_provider(pe, c))));
    let interfaces = found("Interfaces", guarded(|| find_interfaces(pe)));
    let sprite =
        interfaces.and_then(|_| found("Component sprite", guarded(|| find_component_sprite(pe))));
    let invalidate = found(
        "Component invalidation",
        guarded(|| find_component_invalidate(pe)),
    );
    let fields =
        interfaces.and_then(|_| found("Component fields", guarded(|| find_component_fields(pe))));
    let world_select =
        client.and_then(|c| found("World select", guarded(|| find_world_select(pe, c))));
    let minimap = client.and_then(|c| found("Minimap", guarded(|| find_minimap(pe, c))));
    let destination =
        minimap.and_then(|m| found("Destination", guarded(|| find_destination(pe, m.object))));
    let ground_items = found("Ground items", guarded(|| find_ground_items(pe)));
    let ground_item_ownership = ground_items.zip(tick_count).and_then(|(g, tick)| {
        found(
            "Ground item ownership",
            guarded(|| find_ground_item_ownership(pe, tick, &g)),
        )
    });
    let chat = found("Chat", guarded(|| find_chat(pe)));
    let projectiles = found("Projectiles", guarded(|| find_projectiles(pe)));
    let spot_anims = found("Spot animations", guarded(|| find_spot_anims(pe)));
    let packets = found("Packets", guarded(|| find_packets(pe)));
    let packet_layouts = packets.and_then(|p| match guarded(|| find_packet_layouts(pe, &p)) {
        Ok(layouts) => {
            println!(
                "[Agent]: Packet layouts found: {} of {} packets.\r\n",
                layouts.layouts.iter().filter(|l| l.is_some()).count(),
                layouts.layouts.len()
            );
            Some(layouts)
        }
        Err(error) => {
            println!("[Agent]: Packet layouts not found: {error:?}.\r\n");
            None
        }
    });
    let packet_names: Vec<(Name, NamedPacket)> = packets
        .zip(packet_layouts.as_ref())
        .map(|(rule, layouts)| {
            let anchors = PacketAnchors {
                tick_count,
                varps,
                chat_messages: chat.map(|chat| chat.messages),
            };
            catch_unwind(AssertUnwindSafe(|| {
                find_packet_names(pe, &rule, layouts, anchors)
            }))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, result)| match result {
                Ok(named) => {
                    println!(
                        "[Agent]: Packet {} is {}.\r\n",
                        name.spelling(),
                        named.opcode
                    );
                    Some((name, named))
                }
                Err(error) => {
                    println!(
                        "[Agent]: Packet {} not found: {error:?}.\r\n",
                        name.spelling()
                    );
                    None
                }
            })
            .collect()
        })
        .unwrap_or_default();
    let script_runs = found("Script runs", guarded(|| find_script_runs(pe)));
    let varc_setters = found("Varc setters", guarded(|| find_varc_setters(pe)));
    let tick = client.zip(tick_count);
    let containers = guarded(|| find_item_containers(pe));
    let item_setter = containers.ok().and_then(|c| {
        found("Item setter", guarded(|| find_item_setter(pe, &c))).map(|rule| (rule, c))
    });
    let camera = client.and_then(|c| found("Camera", guarded(|| find_camera(pe, c))));
    let projection = client.and_then(|c| found("Projection", guarded(|| find_projection(pe, c))));
    let scene = projection.and_then(|_| found("Scene", guarded(|| find_scene(pe))));
    let collision =
        scene.and_then(|s| found("Collision maps", guarded(|| find_collision_maps(pe, &s))));
    let instance_flag = found("Instance flag", guarded(|| find_instance_flag(pe)));
    let local_projection = client.zip(interfaces).zip(projection.zip(scene)).and_then(
        |((c, i), (projection, scene))| {
            found(
                "Local projection",
                guarded(|| find_local_projection(pe, c, i.canvas, &projection, &scene)),
            )
        },
    );
    let world = client
        .zip(player)
        .zip(actor)
        .and_then(|((client, player), actor)| {
            let world = (|| {
                Ok::<_, (&str, ScanError)>(World {
                    containers: containers.map_err(|e| ("item containers", e))?,
                    varcs: guarded(|| find_varcs(pe, client)).map_err(|e| ("varcs", e))?,
                    run_energy: guarded(|| find_run_energy(pe)).map_err(|e| ("run energy", e))?,
                    lists: guarded(|| find_actor_lists(pe))
                        .map_err(|e| ("NPC and player lists", e))?,
                    names: guarded(|| find_actor_names(pe)).map_err(|e| ("actor names", e))?,
                    client,
                    player,
                    actor,
                })
            })();
            match world {
                Ok(world) => Some(world),
                Err((what, error)) => {
                    println!("[Agent]: World not found ({what}): {error:?}.\r\n");
                    None
                }
            }
        });
    if world.is_some() {
        println!("[Agent]: World found.\r\n");
    }

    let module_base = image.as_ptr() as u64;
    let mut layout = Layout {
        module_base,
        client: client.unwrap_or(0),
        ..Layout::default()
    };
    if let Some(offset) = game_state.filter(|_| client.is_some()) {
        layout.found |= part::GAME_STATE;
        layout.game_state = offset;
    }
    if let Some(offset) = tick_count {
        layout.found |= part::TICK_COUNT;
        layout.tick_count = offset;
    }
    if let (Some(p), Some(a)) = (player, actor) {
        layout.found |= part::PLAYER;
        layout.player = PlayerLayout {
            world_views: p.world_views,
            world_view_id: p.world_view_id,
            player_index: p.player_index,
            players: p.players,
            base_x: a.base_x,
            local_x: a.x,
        };
    }
    if let Some(s) = skills {
        layout.found |= part::SKILLS;
        layout.skills = SkillsLayout {
            levels: s.levels,
            real_levels: s.real_levels,
            xp: s.xp,
            count: s.count,
        };
    }
    if let Some(rva) = varps {
        layout.found |= part::VARPS;
        layout.varps = rva;
    }
    if let Some(l) = login {
        layout.found |= part::LOGIN;
        layout.login = LoginLayout {
            screen: l.screen,
            panel_x: l.panel_x,
            box_x: l.box_x,
            center_x: l.center_x,
            y: l.y,
            ..LoginLayout::default()
        };
    }
    if let Some(p) = login_provider {
        layout.found |= part::LOGIN_PROVIDER;
        layout.login.provider_holder = p.holder;
        layout.login.provider = p.provider;
        layout.login.steam_provider_vtable = p.steam_vtable;
    }
    if let Some(w) = world_select {
        layout.found |= part::WORLD_SELECT;
        layout.world_select = WorldSelectLayout {
            switcher: w.switcher,
            current_world: w.current_world,
            master_count: w.master_count,
            master_worlds: w.master_worlds,
            open: w.open,
            list: w.list,
            state: w.state,
            layout: w.layout,
            selected: w.selected,
            hovered: w.hovered,
            page: w.page,
            compact: w.compact,
            list_rect: w.list_rect,
            row_offset: w.row_offset,
            row_height: w.row_height,
            rows: w.rows,
            columns: w.columns,
            compact_columns: w.compact_columns,
            previous: w.previous,
            next: w.next,
            id: w.world.id,
            types: w.world.types,
            players: w.world.players,
            activity: w.world.activity,
            location: w.world.location,
            reserved: 0,
        };
    }
    if let Some(m) = minimap {
        layout.found |= part::MINIMAP;
        layout.minimap = MinimapLayout {
            object: m.object,
            zoom: m.zoom,
            destination: destination.unwrap_or(0),
        };
        if destination.is_some() {
            layout.found |= part::DESTINATION;
        }
    }
    if let Some(g) = ground_items {
        layout.found |= part::GROUND_ITEMS;
        layout.ground_items = GroundItemsLayout {
            items: g.items,
            level: g.level,
            node_item: g.node_item,
            id: g.id,
            ..GroundItemsLayout::default()
        };
    }
    if let Some(o) = ground_item_ownership {
        layout.found |= part::GROUND_ITEM_OWNERSHIP;
        layout.ground_items.public_tick = o.public_tick;
        layout.ground_items.despawn_tick = o.despawn_tick;
        layout.ground_items.ownership = o.ownership;
        layout.ground_items.never_public = o.never_public;
        layout.ground_items.op_flags = o.op_flags;
    }
    if let Some(c) = chat {
        layout.found |= part::CHAT;
        layout.chat = ChatLayout {
            messages: c.messages,
            cycle: c.cycle,
            message_cycle: c.message_cycle,
            kind: c.kind,
            name: c.name,
            sender: c.sender,
            text: c.text,
            time_utc: c.time_utc,
            time_local: c.time_local,
            reserved: 0,
        };
    }
    if let Some((c, m)) = camera.zip(minimap).filter(|(c, m)| c.yaw == m.angle) {
        layout.found |= part::CAMERA;
        layout.camera = CameraLayout {
            position: c.position,
            pitch: c.pitch,
            yaw: c.yaw,
            focal: m.focal_x,
        };
    }
    if let (Some(i), Some(client)) = (interfaces, client) {
        let c = i.component;
        layout.found |= part::INTERFACES;
        layout.interfaces = InterfaceLayout {
            module_base,
            client,
            root: i.root,
            manager: i.manager,
            groups: i.groups,
            sub_interfaces: i.sub_interfaces,
            canvas: i.canvas,
            ui_width: i.ui_width,
            ui_scale: i.ui_scale,
            id: c.id,
            kind: c.kind,
            parent: c.parent,
            x: c.x,
            width: c.width,
            scroll_x: c.scroll_x,
            sprite: sprite.unwrap_or(0),
            text: invalidate.map_or(0, |rule| rule.text),
            hidden: fields.map_or(0, |f| f.hidden),
            text_color: fields.map_or(0, |f| f.text_color),
            scroll_width: fields.map_or(0, |f| f.scroll_width),
            index: fields.map_or(0, |f| f.index),
            children: fields.map_or(0, |f| f.children),
            reserved: 0,
        };
    }
    if let (Some(l), Some(p), Some(s)) = (
        local_projection.filter(|_| layout.has(part::CAMERA | part::INTERFACES)),
        projection,
        scene,
    ) {
        layout.found |= part::PROJECTION;
        layout.projection = ProjectionLayout {
            top_world_view: p.top_world_view,
            plane: p.plane,
            scene: s.scene,
            size_x: s.size_x,
            size_y: s.size_y,
            tiles: s.tiles,
            zoom: l.zoom,
            view_holder: l.view_holder,
            view: l.view,
            view_center: l.view_center,
            view_offset_x: l.view_offset_x,
            view_offset_y: l.view_offset_y,
            trig: l.trig,
            canvas_size: l.canvas_size,
            ui_size: l.ui_size,
            heights: l.heights,
            settings: l.settings,
            tile_x: l.tile_x,
            tile_model: l.tile_model,
            tile_paint: l.tile_paint,
            ground_decoration: s.ground_decoration,
            model_vertices: l.model_vertices,
            model_triangles: l.model_triangles,
            decoration_height: l.decoration_height,
        };
        if let Some(c) = collision {
            layout.found |= part::COLLISION;
            layout.collision = CollisionLayout {
                maps: c.maps,
                stride: c.stride,
            };
        }
    }
    if let Some(p) = projectiles.filter(|_| world.is_some()) {
        layout.found |= part::PROJECTILES;
        layout.projectiles = ProjectileLayout {
            list: p.list,
            node: p.node,
            cycle: p.cycle,
            source_level: p.source_level,
            source_x: p.source_x,
            source_y: p.source_y,
            source_actor: p.source_actor,
            start_height: p.start_height,
            target_level: p.target_level,
            target_x: p.target_x,
            target_y: p.target_y,
            target_actor: p.target_actor,
            end_height: p.end_height,
            start_cycle: p.start_cycle,
            end_cycle: p.end_cycle,
            slope: p.slope,
            start_pos: p.start_pos,
            id: p.id,
            x: p.x,
            z: p.z,
            y: p.y,
            orientation: p.orientation,
            moving: p.moving,
            reserved: 0,
        };
    }
    if let Some(g) = spot_anims.filter(|_| player.is_some()) {
        layout.found |= part::SPOT_ANIMS;
        layout.spot_anims = SpotAnimLayout {
            list: g.list,
            cycle: g.cycle,
            start_cycle: g.start_cycle,
            level: g.level,
            x: g.x,
            y: g.y,
            z: g.z,
            id: g.id,
        };
    }
    if let Some(p) = path.filter(|_| layout.has(part::PLAYER)) {
        layout.found |= part::PATH;
        layout.path = PathLayout {
            x: p.x,
            y: p.y,
            length: p.length,
            reserved: 0,
        };
    }
    if let Some(flag) = instance_flag {
        layout.found |= part::SCENE_LOADER;
        layout.scene_loader = SceneLoaderLayout {
            instance_flag: flag,
        };
    }
    let head_icons = found("NPC head icons", guarded(|| find_npc_head_icons(pe)));
    if let Some(w) = world {
        layout.found |= part::WORLD;
        layout.world = WorldLayout {
            module_base,
            client: w.client,
            world_views: w.player.world_views,
            world_view_id: w.player.world_view_id,
            base_x: w.actor.base_x,
            players: w.player.players,
            npc_table: w.lists.npc_table,
            npc_indices: w.lists.npc_indices,
            player_count: w.lists.player_count,
            local_x: w.actor.x,
            npc_name_override: w.names.npc_name_override,
            npc_definition: w.names.npc_definition,
            npc_animation: w.names.npc_animation,
            player_name: w.names.player_name,
            definition_id: w.names.definition_id,
            definition_name: w.names.definition_name,
            containers: w.containers.table,
            container_ids: w.containers.ids,
            container_quantities: w.containers.quantities,
            container_next: w.containers.next,
            varcs: w.varcs.object,
            varc_begin: w.varcs.begin,
            varc_entry_size: w.varcs.entry_size,
            varc_value: w.varcs.value,
            varc_kind: w.varcs.kind,
            run_energy: w.run_energy.energy,
            weight: w.run_energy.weight,
            npc_head_icons: head_icons.map_or(0, |h| h.field),
            head_icon_sprites: head_icons.map_or(0, |h| h.sprites),
            reserved: 0,
        };
    }

    if let Some(rule) = found("Menu", guarded(|| find_menu(pe))) {
        layout.found |= part::MENU;
        layout.menu.object = rule.object;
        layout.menu.entry_vtable = rule.entry_vtable;
        layout.menu.entry_size = rule.entry_size;
        layout.menu.target = rule.target;
        layout.menu.option = rule.option;
        layout.menu.ids = rule.ids;
        layout.menu.priority = rule.priority;
        layout.menu.kind = rule.kind;
        layout.menu.category = rule.category;
        layout.menu.cancel = rule.cancel;
        if let Some(key) = found("Menu order", guarded(|| find_menu_order(pe, &rule))) {
            layout.found |= part::MENU_ORDER;
            layout.menu.order_key = key;
        }
        if let Some(settings) = found("Menu settings", guarded(|| find_menu_settings(pe, &rule))) {
            layout.found |= part::MENU_SETTINGS;
            layout.menu.player_attack = settings.player_attack;
            layout.menu.npc_attack = settings.npc_attack;
            layout.menu.follower = settings.follower;
            layout.menu.team = settings.team;
            layout.menu.flag = settings.flag;
            layout.menu.npc_level = settings.npc_level;
            layout.menu.npc_follower = settings.npc_follower;
            layout.menu.player_level = settings.player_level;
            layout.menu.player_team = settings.player_team;
        }
        if let Some(click) = found("Menu click", guarded(|| find_menu_click(pe, &rule))) {
            layout.found |= part::MENU_CLICK;
            layout.menu.minimenu_vtable = click.minimenu_vtable;
            layout.menu.click_entry = click.entry;
            layout.menu.click_x = click.click_x;
            layout.menu.click_counter = click.counter;
            if let Some(open) = found("Menu open", guarded(|| find_menu_open(pe, &click))) {
                layout.found |= part::MENU_OPEN;
                layout.menu.open = open;
            }
            if let Some(menu_layout) =
                found("Menu layout", guarded(|| find_menu_layout(pe, &click)))
            {
                super::menu::set_menu_layout(menu_layout);
            }
        }
        if let Some(selection) = found("Selection", guarded(|| find_selection(pe, &rule))) {
            layout.found |= part::SELECTION;
            layout.menu.selection = selection.offset;
            layout.menu.selection_verb = selection.verb;
            layout.menu.selection_name = selection.name;
            layout.menu.selection_id = selection.id;
            layout.menu.selection_child = selection.child;
            layout.menu.selection_item = selection.item;
            layout.menu.selection_flags = selection.flags;
            layout.menu.selection_selected = selection.selected;
        }
    }

    if let Some(map) = LAYOUT_MAP.get() {
        map.publish(&layout);
    }
    let _ = super::own::LAYOUT.set(layout);
    if let Some(w) = world {
        if let Some(decoder) = found(
            "Appearance decoder",
            guarded(|| find_appearance_decoder(pe, w.names.player_name, w.player.player_index)),
        ) {
            super::name::hook_decoder(module_base, decoder);
        }
    }
    if layout.has(part::SKILLS) {
        if let Some(rule) = invalidate {
            super::total_xp::hook(module_base, rule);
        }
    }
    if let Some(rule) = packets {
        super::packets::hook(module_base, rule, packet_layouts.as_ref(), &packet_names);
    }
    if let Some(rule) = script_runs {
        super::script_runs::hook(module_base, rule, tick);
    }
    if let Some(rule) = varc_setters {
        super::varc_log::hook(module_base, rule);
    }
    if let Some((rule, containers)) = item_setter {
        super::item_log::hook(module_base, rule, containers, tick);
    }
    if let Some(builder) = found("Item icon builder", guarded(|| find_item_icon(pe))) {
        super::cache::set_item_icon(module_base, builder);
    }
    if let (Some(projection), Some(scene)) = (projection, scene) {
        let game = found(
            "Game objects",
            guarded(|| find_game_object_layout(pe, &scene)),
        );
        let decoration = found(
            "Decorations",
            guarded(|| find_decoration_layout(pe, &scene)),
        );
        let item_pile =
            decoration.and_then(|d| found("Item pile", guarded(|| find_item_pile(pe, &d))));
        if let Some(game) = game {
            let configs = found(
                "Loc configs",
                guarded(|| find_loc_configs(pe, &scene, &game)),
            );
            super::scene::set_scene(super::scene::Rules {
                module_base,
                scene,
                game,
                configs,
                plane: projection.plane,
                item_pile,
            });
        }
    }
    if let Some(rule) = found("NPC model", guarded(|| find_npc_model(pe))) {
        let vertices = found("Model vertices", guarded(|| find_model_vertices(pe, &rule)));
        let height = found(
            "Animation height",
            guarded(|| find_animation_height(pe, rule.definition)),
        );
        let faces =
            vertices.and_then(|v| found("Model faces", guarded(|| find_model_faces(pe, &v))));
        if let (Some(vertices), Some(faces)) = (vertices, faces) {
            super::model::set_npc_model(rule, vertices, faces);
        }
        let animation_slot = height.and_then(|height| {
            super::model::animation_slot(image, module_base, rule.get_model, height)
        });
        println!("[Agent]: Renderable animation height slot: {animation_slot:x?}.\r\n");
        if let (Some(render), Some(slot)) =
            (found("Render", guarded(|| find_render(pe))), animation_slot)
        {
            super::drawn::hook(module_base, render, slot);
        }
    }
    if let Some(rule) = found("Hider", guarded(|| find_hider(pe))) {
        super::hider::set_rule(module_base, rule);
    }
    if let Some(rule) = found("Frame", guarded(|| find_frame(pe))) {
        super::frame::hook(module_base, image.len() as u64, rule);
    }
    set_found(
        layout.has(part::GAME_STATE | part::TICK_COUNT | part::PLAYER | part::SKILLS | part::VARPS),
    );
}

pub unsafe extern "system" fn start(lparam: *mut c_void) -> u32 {
    let hwnd = lparam as u64;

    let pid = GetCurrentProcessId();
    let events = Events::create(pid);
    if events.is_none() {
        println!("[Agent]: Cannot create the unload and work events, agent can't unload.\r\n");
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        super::proxy::start(pid);
        let _ = MEMORY_MANAGER.set(Mutex::new(MemoryManager::create_map(pid)));
        super::world_shapes::create(pid);
        match LayoutMap::create(pid) {
            Some(layout) => drop(LAYOUT_MAP.set(layout)),
            None => println!("[Agent]: Cannot create the game state layout shared memory.\r\n"),
        }

        hook_wndproc(hwnd);
        super::focus::hook();
        hook_wgl_swap_buffers();
        find_offsets();
        super::definitions::start();
    }));

    if result.is_err() {
        println!("[Agent]: Failed to start, agent is inactive in this client until unloaded.\r\n");
    }
    if let Some(events) = events {
        events.listen();
    }
    0
}
