use gl::{CURRENT_PROGRAM, VERTEX_ARRAY_BINDING};
use retour::GenericDetour;
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering},
        LazyLock, Mutex, OnceLock,
    },
};

use std::char::from_u32;

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
                WM_IME_NOTIFY, WM_IME_SETCONTEXT, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS,
                WM_MOUSEMOVE, WNDPROC,
            },
        },
    },
};

use super::graphics::{
    draw_cursor, draw_overlay, load_opengl_extensions, read_frame, release_frame_buffers,
    restore_state,
};
use super::offsets::{
    find_actor_position, find_game_state, find_local_player, find_login_panel, find_skills, find_tick_count,
    find_varps, find_interfaces, find_item_containers, find_varcs, find_run_energy, find_actor_lists, find_actor_names,
    find_appearance_decoder, find_chat, find_component_fields, find_component_invalidate, find_component_sprite, find_game_vals, find_destination, find_spot_anims, find_packets, find_ground_items, find_item_pile, find_projectiles, find_menu, find_menu_click, find_menu_layout, find_menu_open, find_selection, find_actor_orientation, find_decoration_layout, find_game_object_layout, find_loc_configs, find_loc_names, find_scene, find_collision_maps, find_instance_flag, find_animation_height, find_model_faces, find_model_vertices, find_camera, find_minimap, find_npc_model, find_obj_type_lookup, find_projection, find_local_projection,
    find_world_select, ScanError, World,
};
use super::state::{LayoutMap, LAYOUT_MAP};
use ipc::{cursor_style, overlay_mode};
use ipc::state::{
    part, CameraLayout, ChatLayout, CollisionLayout, SpotAnimLayout, GroundItemsLayout, InterfaceLayout, Layout, LoginLayout, MinimapLayout, PlayerLayout, ProjectileLayout, ProjectionLayout, SceneLoaderLayout, SkillsLayout, WorldLayout, WorldSelectLayout,
};
use super::unload::HookCall;
use crate::shared::{
    memory::{MemoryManager, MEMORY_MANAGER},
    sync::event_listener,
    windows::{WI_CACHE, WI_CONSOLE, WI_DETACH, WI_GAME_VALS, WI_LOCS, WI_MENU, WI_MODEL, WI_PROJECT, WI_ITEM_NAMES, WI_MODIFIERS, WI_NAME, WI_TOTAL_XP},
};

static KEYBOARD_MODIFIERS: LazyLock<Mutex<(bool, bool, bool)>> = LazyLock::new(|| Mutex::new((false, false, false)));
static LAST_CHAR: LazyLock<Mutex<i32>> = LazyLock::new(|| Mutex::new(0));

pub unsafe fn open_client_console() {
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

//WndProc hook
static HOOKED_HWND: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static WNDPROC_HOOKED: AtomicBool = AtomicBool::new(false);

fn hooked_wndproc_address() -> isize {
    hooked_wndproc as *const () as isize
}

unsafe fn call_original(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let original: WNDPROC = std::mem::transmute(ORIGINAL_WNDPROC.load(Ordering::SeqCst));
    CallWindowProcW(original, hwnd, msg, wparam, lparam)
}

fn get_modifier_lparam(key: i32, down: bool) -> LPARAM {
    let scancode = unsafe { MapVirtualKeyA(key as u32, MAPVK_VK_TO_VSC) };

    let mut lparam = 1 | (scancode << 16);
    if !down {
        lparam |= (1 << 30) | (1 << 31);
    }
    LPARAM(lparam as isize)
}

fn decode_modifiers(wparam: WPARAM) -> (bool, bool, bool) {
    let value = wparam.0;
    let shift = (value & (1 << 0)) != 0;
    let ctrl = (value & (1 << 1)) != 0;
    let alt = (value & (1 << 2)) != 0;
    (shift, ctrl, alt)
}

unsafe fn get_updated_wparam(vkey: u32, shift: bool, ctrl: bool, alt: bool) -> WPARAM {
    let scancode = MapVirtualKeyW(vkey, MAPVK_VK_TO_VSC);

    let mut keyboard_state = [0u8; 256];
    let _ = GetKeyboardState(&mut keyboard_state);

    keyboard_state[vkey as usize] = 0x80;

    if shift {
        keyboard_state[0x10_usize] = 0x80;
    }

    if ctrl {
        keyboard_state[0x11_usize] = 0x80;
    }

    if alt {
        keyboard_state[0x12_usize] = 0x80;
    }

    let mut buff = [0u16; 4];
    let _ = ToUnicode(vkey, scancode, Some(&keyboard_state), &mut buff, 0);

    match from_u32(buff[0] as u32) {
        Some(unicode) => WPARAM(unicode as usize),
        None => WPARAM(0),
    }
}

fn rebuild_key(key: u8, shift: bool, ctrl: bool, alt: bool) -> u16 {
    let mut modifiers: u8 = 0;
    if shift {
        modifiers |= 0x01;
    }
    if ctrl {
        modifiers |= 0x02;
    }
    if alt {
        modifiers |= 0x04;
    }

    ((modifiers as u16) << 8) | (key as u16)
}

unsafe extern "system" fn hooked_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let _call = HookCall::enter();
    wndproc(hwnd, msg, wparam, lparam)
}

static LAST_SYNTHETIC_MOUSE: AtomicU64 = AtomicU64::new(0);

fn filter_mouse(msg: u32, wparam: WPARAM) -> Option<WPARAM> {
    use ipc::input::{MOUSE_FIRST, MOUSE_LAST, REAL_MOUSE_BLOCK_MS, SYNTHETIC};
    if !(MOUSE_FIRST..=MOUSE_LAST).contains(&msg) {
        return Some(wparam);
    }
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    let now = START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1;
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
        WI_CONSOLE => {
            open_client_console();
            return LRESULT(0);
        }
        WI_MODIFIERS => {
            let mut modifiers = KEYBOARD_MODIFIERS.lock().unwrap();
            let (shift, ctrl, alt) = &mut *modifiers;
            let (nshift, nctrl, nalt) = decode_modifiers(wparam);

            if nshift {
                let msg = if *shift { WM_KEYUP } else { WM_KEYDOWN };
                let lparam = get_modifier_lparam(0x10, !*shift);
                let _ = call_original(hwnd, msg, WPARAM(0x10), lparam);
            }

            if nctrl {
                let msg = if *ctrl { WM_KEYUP } else { WM_KEYDOWN };
                let lparam = get_modifier_lparam(0x11, !*ctrl);
                let _ = call_original(hwnd, msg, WPARAM(0x11), lparam);
            }

            if nalt {
                let msg = if *alt { WM_KEYUP } else { WM_KEYDOWN };
                let lparam = get_modifier_lparam(0x12, !*alt);
                let _ = call_original(hwnd, msg, WPARAM(0x12), lparam);
            }

            if nshift {
                *shift = !*shift;
            }
            if nctrl {
                *ctrl = !*ctrl;
            }
            if nalt {
                *alt = !*alt;
            }

            return LRESULT(0);
        }
        WI_NAME => {
            super::name::on_request();
            return LRESULT(0);
        }
        WI_TOTAL_XP => {
            super::total_xp::on_request();
            return LRESULT(0);
        }
        WI_ITEM_NAMES => {
            super::items::on_request();
            return LRESULT(0);
        }
        WI_GAME_VALS => {
            super::items::on_game_val_request();
            return LRESULT(0);
        }
        WI_PROJECT => {
            super::projection::on_request();
            return LRESULT(0);
        }
        WI_MENU => {
            super::menu::on_request();
            return LRESULT(0);
        }
        WI_MODEL => {
            super::model::on_request();
            return LRESULT(0);
        }
        WI_LOCS => {
            super::scene::on_locs_request();
            return LRESULT(0);
        }
        WI_CACHE => {
            super::cache::on_request();
            return LRESULT(0);
        }
        WI_DETACH => {
            unhook_wgl_swap_buffers();
            super::name::unhook_decoder();
            super::total_xp::unhook();
            super::packets::unhook();
            unhook_wndproc();

            return LRESULT(0);
        }
        WM_KEYDOWN => {
            let mut modifiers = KEYBOARD_MODIFIERS.lock().unwrap();
            let (shift, ctrl, alt) = &mut *modifiers;
            if *shift | *ctrl | *alt {
                *LAST_CHAR.lock().unwrap() = wparam.0 as i32;
            }
            WM_KEYDOWN
        }
        WM_CHAR => {
            let mut modifiers = KEYBOARD_MODIFIERS.lock().unwrap();
            let (shift, ctrl, alt) = &mut *modifiers;
            if *shift | *ctrl | *alt {
                let vkey = LAST_CHAR.lock().unwrap();
                let key = rebuild_key(*vkey as u8, *shift, *ctrl, *alt);
                let new_wparam = get_updated_wparam((key & 0xFF) as u32, *shift, *ctrl, *alt);

                //
                println!(
                    "[Agent]: ch: {:?}: key: {:?}, i16Key: {:?}\r\n",
                    wparam,
                    key,
                    key & 0xFF
                );
                println!(
                    "[Agent]: updated message: WM_CHAR, wparam: {:?}, lparam: {:?}\r\n",
                    new_wparam, lparam
                );

                return call_original(hwnd, msg, new_wparam, lparam);
            }

            WM_CHAR
        }
        WM_KILLFOCUS => return LRESULT(0),
        WM_IME_SETCONTEXT => return LRESULT(0),
        WM_IME_NOTIFY => return LRESULT(0),
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as u16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i32;

            let mem_manager = MEMORY_MANAGER
                .get()
                .expect("[Agent]: Memory manager is not initialized!\r\n")
                .lock()
                .unwrap();
            mem_manager.set_mouse_position(x, y);

            WM_MOUSEMOVE
        }
        _ => msg,
    };

    call_original(hwnd, msg, wparam, lparam)
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

pub unsafe fn unhook_wndproc() {
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

//OpenGL Hook
static ORIGINAL_WGL_SWAPBUFFERS: OnceLock<GenericDetour<unsafe extern "system" fn(HDC) -> BOOL>> =
    OnceLock::new();

unsafe extern "system" fn hooked_wgl_swap_buffers(hdc: HDC) -> BOOL {
    let _call = HookCall::enter();
    let mut viewport = [0, 0, 0, 0];
    glGetIntegerv(GL_VIEWPORT, viewport.as_mut_ptr());
    // Save current state
    let mut prev_program: i32 = 0;
    let mut prev_vao: i32 = 0;
    glGetIntegerv(CURRENT_PROGRAM, &mut prev_program);
    glGetIntegerv(VERTEX_ARRAY_BINDING, &mut prev_vao);

    let offset_x = viewport[0];
    let offset_y = viewport[1];
    let width = viewport[2];
    let height = viewport[3];
    let frame_size = width * height * 4;

    let mut client_rect = RECT::default();
    let hwnd = HWND(HOOKED_HWND.load(Ordering::SeqCst) as *mut c_void);
    let _ = GetClientRect(hwnd, &mut client_rect);
    let window_height = client_rect.bottom - client_rect.top;
    let offset_y_top_down = window_height - offset_y - height;

    let (mouse, dest, overlay, (cursor, cursor_color, cursor_size)) = {
        let mem_manager = MEMORY_MANAGER
            .get()
            .expect("[Agent]: Memory manager is not initialized!\r\n")
            .lock()
            .unwrap();
        mem_manager.set_viewport(offset_x, offset_y, width, height, window_height);
        // The injector paints the slot the serial doesn't select, so the shown one is never mid-paint.
        let overlay = match mem_manager.overlay_mode() {
            overlay_mode::OFF => None,
            overlay_mode::EVERY_FRAME => Some((mem_manager.overlay_ptr(0), None)),
            _ => {
                let serial = mem_manager.overlay_serial();
                Some((mem_manager.overlay_ptr(serial as usize & 1), Some(serial)))
            }
        };
        let dest = mem_manager
            .capture_enabled()
            .then(|| mem_manager.image_ptr());
        (mem_manager.get_mouse_position(), dest, overlay, mem_manager.cursor())
    };

    if load_opengl_extensions() {
        match dest {
            Some(dest) => read_frame(offset_x, offset_y, width, height, frame_size, dest),
            None => release_frame_buffers(),
        }

        if let Some((overlay, serial)) = overlay {
            draw_overlay(offset_x, offset_y, width, height, overlay, serial);
        }

        if cursor != cursor_style::OFF && mouse.0 > -1 && mouse.1 > -1 {
            let (mouse_x, mouse_y) = (mouse.0 - offset_x, mouse.1 - offset_y_top_down);
            if mouse_x > -1 && mouse_y > -1 && mouse_x < width && mouse_y < height {
                draw_cursor(mouse_x, mouse_y, width, height, cursor, cursor_color, cursor_size);
            }
        }
    }

    restore_state(prev_program, prev_vao);

    let original = ORIGINAL_WGL_SWAPBUFFERS.get().unwrap();
    let presented = original.call(hdc);
    // after the swap, so a reader woken by it paints the overlay for the next frame from what this one showed
    if let Some(memory) = MEMORY_MANAGER.get() {
        memory.lock().unwrap_or_else(|e| e.into_inner()).frame_presented();
    }
    presented
}

unsafe fn hook_wgl_swap_buffers() {
    let module = GetModuleHandleA(PCSTR(c"opengl32.dll".as_ptr().cast()))
        .expect("[Agent]: opengl32.dll module not found.\r\n");

    let addr = GetProcAddress(module, PCSTR(c"wglSwapBuffers".as_ptr().cast()))
        .expect("[Agent]: wglSwapBuffers function not found.\r\n");

    let original_fn: unsafe extern "system" fn(HDC) -> BOOL = std::mem::transmute(addr);
    let detour = GenericDetour::new(original_fn, hooked_wgl_swap_buffers)
        .expect("[Agent]: Failed to create wglSwapBuffers hook.\r\n");

    ORIGINAL_WGL_SWAPBUFFERS
        .set(detour)
        .expect("[Agent]: Failed to save original wglSwapBuffers function.\r\n");

    ORIGINAL_WGL_SWAPBUFFERS
        .get()
        .unwrap()
        .enable()
        .expect("[Agent]: Failed to enable wglSwapBuffers hook.\r\n");

    println!("[Agent]: wglSwapBuffers successfully hooked.\r\n");
}

pub unsafe fn unhook_wgl_swap_buffers() {
    if let Some(memory) = MEMORY_MANAGER.get() {
        memory.lock().unwrap_or_else(|e| e.into_inner()).clear_overlay();
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

pub(super) unsafe fn client_image<'a>() -> Option<&'a [u8]> {
    let base = GetModuleHandleA(PCSTR::null()).ok()?.0 as *const u8;
    if base.is_null() {
        return None;
    }

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
    let set_found = |found: bool| {
        MEMORY_MANAGER
            .get()
            .expect("[Agent]: Memory manager is not initialized!\r\n")
            .lock()
            .unwrap()
            .set_offsets_found(found)
    };
    let (Some(image), Ok(module)) = (client_image(), GetModuleHandleA(PCSTR::null())) else {
        println!("[Agent]: Client image not found.\r\n");
        set_found(false);
        return;
    };

    let actor = found("Actor position", find_actor_position(image));
    let player = found("Local player", find_local_player(image));
    let game_state = found("Game state", find_game_state(image));
    let skills = player.and_then(|p| found("Skills", find_skills(image, p.client)));
    let tick_count = player.and_then(|p| found("Tick count", find_tick_count(image, p.client)));
    let varps = found("Varps", find_varps(image));
    let login = game_state.and_then(|offset| found("Login panel", find_login_panel(image, offset)));
    let interfaces = found("Interfaces", find_interfaces(image));
    let sprite = interfaces.and_then(|_| found("Component sprite", find_component_sprite(image)));
    let invalidate = found("Component invalidation", find_component_invalidate(image));
    let fields = interfaces.and_then(|_| found("Component fields", find_component_fields(image)));
    let world_select = player.and_then(|p| found("World select", find_world_select(image, p.client)));
    let minimap = player.and_then(|p| found("Minimap", find_minimap(image, p.client)));
    let destination = minimap.and_then(|m| found("Destination", find_destination(image, m.object)));
    let ground_items = found("Ground items", find_ground_items(image));
    let chat = found("Chat", find_chat(image));
    let projectiles = found("Projectiles", find_projectiles(image));
    let spot_anims = found("Spot animations", find_spot_anims(image));
    let packets = found("Packets", find_packets(image));
    let camera = player.and_then(|p| found("Camera", find_camera(image, p.client)));
    let projection = player.and_then(|p| found("Projection", find_projection(image, p.client)));
    let scene = projection.and_then(|_| found("Scene", find_scene(image)));
    let collision = scene.and_then(|s| found("Collision maps", find_collision_maps(image, &s)));
    let instance_flag = found("Instance flag", find_instance_flag(image));
    let local_projection = player.zip(interfaces).zip(projection.zip(scene)).and_then(|((p, i), (projection, scene))| {
        found("Local projection", find_local_projection(image, p.client, i.canvas, &projection, &scene))
    });
    let world = player.zip(actor).and_then(|(player, actor)| {
        let world = (|| {
            Ok::<_, (&str, ScanError)>(World {
                containers: find_item_containers(image).map_err(|e| ("item containers", e))?,
                varcs: find_varcs(image, player.client).map_err(|e| ("varcs", e))?,
                run_energy: find_run_energy(image).map_err(|e| ("run energy", e))?,
                lists: find_actor_lists(image).map_err(|e| ("NPC and player lists", e))?,
                names: find_actor_names(image).map_err(|e| ("actor names", e))?,
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

    let module_base = module.0 as u64;
    let client = player.map_or(0, |p| p.client);
    let mut layout = Layout { module_base, client, ..Layout::default() };
    if let Some(offset) = game_state.filter(|_| player.is_some()) {
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
        layout.skills = SkillsLayout { levels: s.levels, real_levels: s.real_levels, xp: s.xp, count: s.count };
    }
    if let Some(rva) = varps {
        layout.found |= part::VARPS;
        layout.varps = rva;
    }
    if let Some(l) = login {
        layout.found |= part::LOGIN;
        layout.login = LoginLayout { screen: l.screen, panel_x: l.panel_x, box_x: l.box_x, center_x: l.center_x, y: l.y };
    }
    if let Some(w) = world_select {
        layout.found |= part::WORLD_SELECT;
        layout.world_select = WorldSelectLayout {
            open: w.open,
            count: w.count,
            worlds: w.worlds,
            offset: w.offset,
            max_offset: w.max_offset,
            current_world: w.current_world,
            background: w.background,
            background_height: w.background_height,
            id: w.world.id,
            types: w.world.types,
            players: w.world.players,
            activity: w.world.activity,
            location: w.world.location,
            hovered: w.hovered,
        };
    }
    if let Some(m) = minimap {
        layout.found |= part::MINIMAP;
        layout.minimap = MinimapLayout { object: m.object, zoom: m.zoom, destination: destination.unwrap_or(0) };
        if destination.is_some() {
            layout.found |= part::DESTINATION;
        }
    }
    if let Some(g) = ground_items {
        layout.found |= part::GROUND_ITEMS;
        layout.ground_items = GroundItemsLayout { items: g.items, level: g.level, node_item: g.node_item, id: g.id };
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
    // The minimap rule finds the focal point; both rules find the yaw, so they must agree.
    if let (Some(c), Some(m)) = (camera, minimap.filter(|m| camera.is_some_and(|c| c.yaw == m.angle))) {
        layout.found |= part::CAMERA;
        layout.camera = CameraLayout { position: c.position, pitch: c.pitch, yaw: c.yaw, focal: m.focal_x };
    }
    if let Some(i) = interfaces.filter(|_| player.is_some()) {
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
    if let (Some(l), Some(p), Some(s)) = (local_projection.filter(|_| layout.has(part::CAMERA | part::INTERFACES)), projection, scene) {
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
            layout.collision = CollisionLayout { maps: c.maps, stride: c.stride };
        }
    }
    // read through the world view, so it needs the world's client and actor layout
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
    // read through the local player's world view
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
    if let Some(flag) = instance_flag {
        layout.found |= part::SCENE_LOADER;
        layout.scene_loader = SceneLoaderLayout { instance_flag: flag };
    }
    if let Some(w) = world {
        layout.found |= part::WORLD;
        layout.world = WorldLayout {
            module_base,
            client: w.player.client,
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
            reserved: 0,
        };
    }

    if let Some(map) = LAYOUT_MAP.get() {
        map.publish(&layout);
    }
    let _ = super::own::LAYOUT.set(layout);
    if let Some(w) = world {
        if let Some(decoder) = found("Appearance decoder", find_appearance_decoder(image, w.names.player_name, w.player.player_index)) {
            super::name::hook_decoder(module_base, decoder);
        }
    }
    if layout.has(part::SKILLS) {
        if let Some(rule) = invalidate {
            super::total_xp::hook(module_base, rule);
        }
    }
    if let Some(rule) = packets {
        super::packets::hook(module_base, rule);
    }
    if let Some(rule) = found("Item definition lookup", find_obj_type_lookup(image)) {
        super::items::set_lookup(module_base, rule);
        if let Some(game_vals) = player.and_then(|p| found("Game vals", find_game_vals(image, p.client, &rule))) {
            super::items::set_game_vals(module_base, game_vals);
        }
    }
    if let Some(rule) = projection {
        super::projection::set_projection(module_base, rule);
    }
    if let (Some(projection), Some(scene)) = (projection, scene) {
        let game = found("Game objects", find_game_object_layout(image, &scene));
        let decoration = found("Decorations", find_decoration_layout(image, &scene));
        let names = found("Loc names", find_loc_names(image));
        let item_pile = decoration.and_then(|d| found("Item pile", find_item_pile(image, &d)));
        if let (Some(game), Some(decoration), Some(names)) = (game, decoration, names) {
            let configs = found("Loc configs", find_loc_configs(image, &scene, &game));
            super::scene::set_scene(super::scene::Rules {
                module_base,
                scene,
                game,
                decoration,
                names,
                configs,
                plane: projection.plane,
                decoration_height: local_projection.map(|l| l.decoration_height),
                item_pile,
            });
        }
    }
    if let Some(rule) = found("Menu", find_menu(image)) {
        super::menu::set_menu(module_base, rule);
        if let Some(click) = found("Menu click", find_menu_click(image, &rule)) {
            super::menu::set_menu_click(click);
            if let Some(open) = found("Menu open", find_menu_open(image, &click)) {
                super::menu::set_menu_open(open);
            }
            if let Some(layout) = found("Menu layout", find_menu_layout(image, &click)) {
                super::menu::set_menu_layout(layout);
            }
        }
        if let Some(selection) = found("Selection", find_selection(image, &rule)) {
            super::menu::set_selection(selection);
        }
    }
    if let Some(rule) = world.and_then(|w| found("NPC model", find_npc_model(image, w.names.npc_definition))) {
        let vertices = found("Model vertices", find_model_vertices(image, &rule));
        let orientation = found("Actor orientation", find_actor_orientation(image));
        let height = world.and_then(|w| found("Animation height", find_animation_height(image, w.names.npc_definition)));
        let faces = vertices.and_then(|v| found("Model faces", find_model_faces(image, &v)));
        if let (Some(vertices), Some(faces), Some(orientation), Some(height)) = (vertices, faces, orientation, height) {
            super::model::set_npc_model(module_base, image, rule, vertices, faces, orientation, height);
        }
    }
    set_found(layout.has(part::GAME_STATE | part::TICK_COUNT | part::PLAYER | part::SKILLS | part::VARPS));
}

//enable...
pub unsafe fn reenable_hooks() {
    let wgl_swapbuffers_detour = ORIGINAL_WGL_SWAPBUFFERS
        .get()
        .expect("[Agent]: wglSwapBuffers hook not found\r\n");

    wgl_swapbuffers_detour
        .enable()
        .expect("[Agent]: Failed to enable wglSwapBuffers hook.\r\n");

    super::name::rehook_decoder();
    super::total_xp::rehook();
    super::packets::rehook();
    hook_wndproc(HOOKED_HWND.load(Ordering::SeqCst) as u64);
}
pub unsafe extern "system" fn start(lparam: *mut c_void) -> u32 {
    let hwnd = lparam as u64;

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = MEMORY_MANAGER.set(Mutex::new(MemoryManager::create_map(GetCurrentProcessId())));
        match LayoutMap::create(GetCurrentProcessId()) {
            Some(layout) => drop(LAYOUT_MAP.set(layout)),
            None => println!("[Agent]: Cannot create the game state layout shared memory.\r\n"),
        }

        hook_wndproc(hwnd);
        hook_wgl_swap_buffers();
        find_offsets();

        event_listener(hwnd, GetCurrentProcessId());
    }));

    if result.is_err() {
        println!("[Agent]: Failed to start, agent is inactive in this client.\r\n");
    }
    0
}
