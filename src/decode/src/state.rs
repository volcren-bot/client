use ipc::{
    read::{NAME_CLEAN, SHORT_STRING_MAX, client, local_name_object, local_player, short_string},
    state::{Layout, NameRequest, SKILL_NAMES, array, game_state as states, interfaces, part},
};

pub use ipc::read::current_game_state as current;

use crate::{
    Camera, GameState, Login, Minimap, Position, Skill, Tile,
    memory::{Memory, read_string},
};

pub const MAX_VARPS: usize = 8192;
const MAX_SKILLS: u32 = 64;
fn position(memory: &impl Memory, layout: &Layout) -> Option<Position> {
    let p = &layout.player;
    let (view, player) = local_player(memory, layout)?;
    let local_x = memory.i32(player + p.local_x as u64)?;
    let local_y = memory.i32(player + p.local_x as u64 + 4)?;
    let base_x = memory.i32(view + p.base_x as u64)?;
    let base_y = memory.i32(view + p.base_x as u64 + 4)?;
    // the player's own world view, not the top one: they differ on a boat
    let plane = layout.has(part::PROJECTION).then(|| memory.i32(view + layout.projection.plane as u64)).flatten();
    Some(Position { local_x, local_y, x: base_x + (local_x >> 7), y: base_y + (local_y >> 7), plane })
}

fn skills(memory: &impl Memory, layout: &Layout, client: u64) -> Option<Vec<Skill>> {
    let s = &layout.skills;
    if s.count > MAX_SKILLS {
        return None;
    }
    let count = s.count as usize;
    let levels = memory.i32s(client + s.levels as u64, count)?;
    let real_levels = memory.i32s(client + s.real_levels as u64, count)?;
    let xp = memory.i32s(client + s.xp as u64, count)?;
    Some(
        (0..count)
            .filter(|&i| i < SKILL_NAMES.len() || xp[i] > 0)
            .map(|i| Skill {
                name: SKILL_NAMES.get(i).map_or_else(|| format!("Skill {i}"), |name| (*name).to_owned()),
                level: levels[i],
                real_level: real_levels[i],
                xp: xp[i],
            })
            .collect(),
    )
}

pub fn total_xp(memory: &impl Memory, layout: &Layout) -> Option<i64> {
    let client = memory.ptr(layout.module_base + layout.client as u64)?;
    let skills = skills(memory, layout, client)?;
    Some(skills.iter().map(|s| s.xp.max(0) as i64).sum())
}

fn player_name(memory: &impl Memory, layout: &Layout) -> Option<String> {
    let name = local_name_object(memory, layout)?;
    Some(read_string(memory, name)?.replace('\u{a0}', " ")).filter(|name| !name.is_empty())
}

/// The logged in account's normalized name (lowercase, spaces as `_`), which the display name override never touches.
fn account_name(memory: &impl Memory, layout: &Layout) -> Option<String> {
    let object = local_name_object(memory, layout)?;
    Some(read_string(memory, object + NAME_CLEAN)?.replace('\u{a0}', " ")).filter(|name| !name.is_empty())
}

pub const MAX_NAME_BYTES: usize = SHORT_STRING_MAX;

pub fn name_request(memory: &impl Memory, layout: &Layout, name: &str) -> Result<NameRequest, String> {
    if name.is_empty() {
        return Ok(NameRequest::default());
    }
    if !layout.has(part::PLAYER | part::WORLD) {
        return Err("agent didn't find the local player's name".into());
    }
    let display = short_string(name.replace(' ', "\u{a0}").as_bytes())
        .ok_or_else(|| format!("a name can be at most {MAX_NAME_BYTES} bytes (a space counts as two)"))?;
    let object = local_name_object(memory, layout).ok_or("log in first: the name is set for the account logged in")?;
    let mut account = [0u8; 24];
    if !memory.read(object + NAME_CLEAN, &mut account) || account[23] & 0x80 != 0 {
        return Err("the account's name can't be read".into());
    }
    Ok(NameRequest { enabled: 1, display, account, ..NameRequest::default() })
}

fn ui(memory: &impl Memory, layout: &Layout, client: u64) -> Option<(i32, i32, f32)> {
    let i = &layout.interfaces;
    let canvas = memory.ptr(client + i.canvas as u64)?;
    let width = memory.i32(canvas + i.ui_width as u64)?;
    let height = memory.i32(canvas + i.ui_width as u64 + 4)?;
    let scale = memory.f32(canvas + i.ui_scale as u64)?;
    (scale.is_finite() && scale > 0.0).then_some((width, height, scale))
}

pub fn ui_scale(memory: &impl Memory, layout: &Layout) -> Option<f32> {
    if !layout.has(part::INTERFACES) {
        return None;
    }
    ui(memory, layout, client(memory, layout)?).map(|(_, _, scale)| scale)
}

pub fn root_interface(memory: &impl Memory, layout: &Layout, state: Option<i32>) -> Option<i32> {
    if !layout.has(part::INTERFACES) || !matches!(state, Some(states::LOGGED_IN | states::LOADING)) {
        return None;
    }
    memory.i32(client(memory, layout)? + layout.interfaces.root as u64)
}

fn login(memory: &impl Memory, layout: &Layout, scale: f32) -> Option<Login> {
    let l = &layout.login;
    let global = |rva: u32| memory.i32(layout.module_base + rva as u64);
    Some(Login {
        screen: global(l.screen)?,
        panel_x: global(l.panel_x)?,
        box_x: global(l.box_x)?,
        center_x: global(l.center_x)?,
        y: global(l.y)?,
        scale,
    })
}

fn minimap(memory: &impl Memory, layout: &Layout, client: u64) -> Option<Minimap> {
    let m = &layout.minimap;
    let object = memory.ptr(client + m.object as u64)?;
    let zoom = memory.i32(object + m.zoom as u64)?;
    (2..=8).contains(&zoom).then_some(Minimap { zoom })
}

fn destination(memory: &impl Memory, layout: &Layout, client: u64) -> Option<Tile> {
    let object = memory.ptr(client + layout.minimap.object as u64)?;
    let at = object + layout.minimap.destination as u64;
    let (x, y) = (memory.i32(at)?, memory.i32(at + 4)?);
    if x == 0 && y == 0 {
        return None;
    }
    let (view, _) = local_player(memory, layout)?;
    let base = view + layout.player.base_x as u64;
    Some(Tile { x: memory.i32(base)? + x, y: memory.i32(base + 4)? + y })
}

const VARC_ZOOM_FIXED: i32 = 73;
const VARC_ZOOM_RESIZABLE: i32 = 74;

fn camera(memory: &impl Memory, layout: &Layout, client: u64, root: Option<i32>) -> Option<Camera> {
    let c = &layout.camera;
    let field = |offset: u32| memory.i32(client + offset as u64);
    let varc = if root == Some(interfaces::ROOT_FIXED) { VARC_ZOOM_FIXED } else { VARC_ZOOM_RESIZABLE };
    Some(Camera {
        x: field(c.position)?,
        y: field(c.position + 8)?,
        z: field(c.position + 4)?,
        pitch: field(c.pitch)? & 0x3fff,
        yaw: field(c.yaw)? & 0x3fff,
        focal_x: field(c.focal)?,
        focal_y: field(c.focal + 8)?,
        focal_z: field(c.focal + 4)?,
        zoom: layout.has(part::WORLD).then(|| crate::world::varc(memory, &layout.world, varc)).flatten(),
    })
}

pub fn game_state(memory: &impl Memory, layout: &Layout) -> GameState {
    let Some(client) = client(memory, layout) else {
        return GameState::default();
    };
    let state = current(memory, layout);
    let logged_in = state == Some(states::LOGGED_IN);
    let on_login_screen = matches!(state, Some(states::LOGIN_SCREEN | states::LOGIN_SCREEN_AUTHENTICATOR));
    let ui = layout.has(part::INTERFACES).then(|| ui(memory, layout, client)).flatten();
    let run_energy = (logged_in && layout.has(part::WORLD))
        .then(|| {
            let w = &layout.world;
            Some((memory.i32(client + w.run_energy as u64)?, memory.i32(client + w.weight as u64)?))
        })
        .flatten();

    GameState {
        position: position(memory, layout),
        player_name: player_name(memory, layout),
        account_name: account_name(memory, layout),
        game_state: state,
        skills: (logged_in && layout.has(part::SKILLS)).then(|| skills(memory, layout, client)).flatten(),
        tick_count: layout.has(part::TICK_COUNT).then(|| memory.i32(client + layout.tick_count as u64)).flatten(),
        login: (on_login_screen && layout.has(part::LOGIN))
            .then(|| login(memory, layout, ui.map_or(1.0, |(_, _, scale)| scale)))
            .flatten(),
        root_interface: root_interface(memory, layout, state),
        ui_scale: ui.map(|(_, _, scale)| scale),
        run_energy: run_energy.map(|(energy, _)| energy),
        weight: run_energy.map(|(_, weight)| weight),
        camera: (logged_in && layout.has(part::CAMERA))
            .then(|| camera(memory, layout, client, root_interface(memory, layout, state)))
            .flatten(),
        minimap: (logged_in && layout.has(part::MINIMAP)).then(|| minimap(memory, layout, client)).flatten(),
        destination: (logged_in && layout.has(part::MINIMAP | part::DESTINATION)).then(|| destination(memory, layout, client)).flatten(),
    }
}

fn varp_array(memory: &impl Memory, layout: &Layout) -> Option<(u64, usize)> {
    if !layout.has(part::VARPS) || current(memory, layout) != Some(states::LOGGED_IN) {
        return None;
    }
    let object = layout.module_base + layout.varps as u64;
    let count = memory.u64(object + array::COUNT as u64)?;
    let elements = memory.ptr(object + array::DATA as u64)?;
    (count > 0).then_some((elements, (count as usize).min(MAX_VARPS)))
}

pub fn varps(memory: &impl Memory, layout: &Layout) -> Option<Vec<i32>> {
    let (elements, count) = varp_array(memory, layout)?;
    memory.i32s(elements, count)
}

pub fn varp(memory: &impl Memory, layout: &Layout, id: usize) -> Option<i32> {
    let (elements, count) = varp_array(memory, layout)?;
    (id < count).then(|| memory.i32(elements + id as u64 * 4)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;
    use ipc::state::{CameraLayout, InterfaceLayout, LoginLayout, MinimapLayout, PlayerLayout, ProjectionLayout, SkillsLayout, WorldLayout};

    const TABLE_BUCKETS: u64 = 0x8;
    const TABLE_COUNT: u64 = 0x10;
    const NODE_VALUE: u64 = 0x10;
    const NODE_NEXT: u64 = 0x18;
    const BASE: u64 = 0x1_4000_0000;
    const CLIENT: u64 = 0x10_0000;

    fn layout() -> Layout {
        Layout {
            found: part::GAME_STATE
                | part::TICK_COUNT
                | part::PLAYER
                | part::SKILLS
                | part::VARPS
                | part::LOGIN
                | part::INTERFACES
                | part::WORLD
                | part::CAMERA
                | part::MINIMAP
                | part::DESTINATION
                | part::PROJECTION,
            module_base: BASE,
            client: 0xe98c18,
            game_state: 0x2160,
            tick_count: 0x3198,
            varps: 0x155e518,
            player: PlayerLayout {
                world_views: 0xc9d0,
                world_view_id: 0xcc50,
                player_index: 0xcc4c,
                players: 0x60,
                base_x: 0x24,
                local_x: 0x268,
            },
            skills: SkillsLayout { levels: 0x3350, real_levels: 0x33b4, xp: 0x3418, count: 25 },
            login: LoginLayout { screen: 0xe98c10, panel_x: 0xe98bd8, box_x: 0x154fc64, center_x: 0xe98bdc, y: 0xcfb094 },
            interfaces: InterfaceLayout { canvas: 0x90, ui_width: 0x6c, ui_scale: 0x68, root: 0x413bc8, ..Default::default() },
            world: WorldLayout { run_energy: 0x34e0, weight: 0x34e4, player_name: 0x718, ..Default::default() },
            camera: CameraLayout { position: 0x895c8, pitch: 0x895dc, yaw: 0x895e0, focal: 0x8964c },
            minimap: MinimapLayout { object: 0x40eab0, zoom: 0x6470, destination: 0 },
            projection: ProjectionLayout { plane: 0x18, ..Default::default() },
            ..Layout::default()
        }
    }

    fn table(m: &mut Fake, table: u64, node: u64, key: i32, value: u64) {
        let buckets = node + 0x100;
        m.put_u64(table + TABLE_BUCKETS, buckets);
        m.put_i32(table + TABLE_COUNT, 1);
        m.put_u64(buckets, node);
        m.put_u64(buckets + 8, 0xdead);
        m.put(node, &[0; 0x20]);
        m.put_i32(node, key);
        m.put_u64(node + NODE_VALUE, value);
        m.put_u64(node + NODE_NEXT, 0);
    }

    fn logged_in() -> Fake {
        let l = layout();
        let mut m = Fake::default();
        m.put_u64(BASE + l.client as u64, CLIENT);
        m.put_i32(CLIENT + l.game_state as u64, states::LOGGED_IN);
        m.put_i32(CLIENT + l.tick_count as u64, 1234);

        let (view, player) = (0x20_0000, 0x21_0000);
        m.put_i32(CLIENT + l.player.world_view_id as u64, 0);
        m.put_i32(CLIENT + l.player.player_index as u64, 780);
        table(&mut m, CLIENT + l.player.world_views as u64, 0x22_0000, 0, view);
        table(&mut m, view + l.player.players as u64, 0x23_0000, 780, player);
        m.put_i32(view + 0x24, 3112);
        m.put_i32(view + 0x28, 3335);
        m.put_i32(view + 0x18, 1);
        m.put_i32(player + 0x268, 6592);
        m.put_i32(player + 0x26c, 6784);
        m.put_u64(player + 0x718, 0x24_0000);
        m.put_short_string(0x24_0000, "P\u{a0}4I\u{a0}N");

        for i in 0..25u64 {
            let xp = if i < 24 { 13_034_431 } else { 0 };
            m.put_i32(CLIENT + 0x3350 + i * 4, if i == 3 { 90 } else { 99 });
            m.put_i32(CLIENT + 0x33b4 + i * 4, 99);
            m.put_i32(CLIENT + 0x3418 + i * 4, xp);
        }

        let (varps, elements) = (BASE + l.varps as u64, 0x30_0000);
        m.put_u64(varps + 8, 5891);
        m.put_u64(varps + 0x10, elements);
        for id in 0..5891u64 {
            m.put_i32(elements + id * 4, id as i32);
        }

        let canvas = 0x40_0000;
        m.put_u64(CLIENT + 0x90, canvas);
        m.put_i32(canvas + 0x6c, 807);
        m.put_i32(canvas + 0x70, 1048);
        m.put(canvas + 0x68, &1.7373f32.to_le_bytes());
        m.put_i32(CLIENT + 0x413bc8, 161);
        m.put_i32(CLIENT + 0x34e0, 10000);
        m.put_i32(CLIENT + 0x34e4, -3);
        m.put_u64(CLIENT + 0x40eab0, 0x50_0000);
        m.put_i32(0x50_0000 + 0x6470, 4);
        m.put_i32(0x50_0000, 52);
        m.put_i32(0x50_0004, 61);
        for (offset, value) in [(0x895c8, 5336), (0x895cc, -936), (0x895d0, 7717), (0x895dc, 3064), (0x895e0, 1991)] {
            m.put_i32(CLIENT + offset, value);
        }
        for (offset, value) in [(0x8964c, 6336), (0x89650, -240), (0x89654, 6080)] {
            m.put_i32(CLIENT + offset, value);
        }

        for rva in [0xe98c10u64, 0xe98bd8, 0x154fc64, 0xe98bdc, 0xcfb094] {
            m.put_i32(BASE + rva, 1);
        }
        m
    }

    #[test]
    fn reads_everything_in_game() {
        let state = game_state(&logged_in(), &layout());
        assert_eq!(state.game_state, Some(states::LOGGED_IN));
        assert_eq!(state.position, Some(Position { local_x: 6592, local_y: 6784, x: 3163, y: 3388, plane: Some(1) }));
        assert_eq!(state.tick_count, Some(1234));
        assert_eq!(state.player_name.as_deref(), Some("P 4I N"));
        assert_eq!(state.account_name, None, "the fixture has no normalized name");
        let mut named = logged_in();
        named.put_short_string(0x24_0018, "p_4i_n");
        assert_eq!(game_state(&named, &layout()).account_name.as_deref(), Some("p_4i_n"));
        let skills = state.skills.unwrap();
        assert_eq!(skills.len(), 24, "the unused 25th slot has no xp");
        assert_eq!(skills[3], Skill { name: "Hitpoints".into(), level: 90, real_level: 99, xp: 13_034_431 });
        assert_eq!(state.login, None);
        assert_eq!(state.root_interface, Some(161));
        assert_eq!(state.ui_scale, Some(1.7373));
        assert_eq!((state.run_energy, state.weight), (Some(10000), Some(-3)));
        assert_eq!(
            state.camera,
            Some(Camera { x: 5336, y: 7717, z: -936, pitch: 3064, yaw: 1991, focal_x: 6336, focal_y: 6080, focal_z: -240, zoom: None })
        );
        assert_eq!(state.minimap, Some(Minimap { zoom: 4 }));
        assert_eq!(state.destination, Some(Tile { x: 3163 - (6592 >> 7) + 52, y: 3388 - (6784 >> 7) + 61 }));
    }

    #[test]
    fn has_no_destination_while_standing() {
        let mut m = logged_in();
        m.put_i32(0x50_0000, 0);
        m.put_i32(0x50_0004, 0);
        assert_eq!(game_state(&m, &layout()).destination, None);
    }

    #[test]
    fn hides_stale_values_on_the_login_screen() {
        let l = layout();
        let mut m = logged_in();
        m.put_i32(CLIENT + l.game_state as u64, states::LOGIN_SCREEN);
        m.put_u64(CLIENT + 0x90, 0);
        for (rva, value) in [(0xe98c10u64, 10), (0xe98bd8, 318), (0x154fc64, 520), (0xe98bdc, 700), (0xcfb094, 171)] {
            m.put_i32(BASE + rva, value);
        }

        let state = game_state(&m, &l);
        assert_eq!(state.game_state, Some(states::LOGIN_SCREEN));
        assert_eq!(state.login, Some(Login { screen: 10, panel_x: 318, box_x: 520, center_x: 700, y: 171, scale: 1.0 }));
        assert_eq!((state.skills, state.root_interface, state.run_energy, state.minimap), (None, None, None, None));
        assert_eq!(state.camera, None);
        assert_eq!(state.player_name, None, "the player table keeps the last account");
        assert_eq!(varps(&m, &l), None);
        assert_eq!(varp(&m, &l, 173), None);
    }

    #[test]
    fn asks_for_a_name_for_the_logged_in_account() {
        let (mut m, l) = (logged_in(), layout());
        m.put_short_string(0x24_0018, "p_4i_n");
        let request = name_request(&m, &l, "Test Player").unwrap();
        assert_eq!(request.enabled, 1);
        assert_eq!(&request.display[..12], "Test\u{a0}Player".as_bytes());
        assert_eq!(request.display[23], 23 - 12);
        assert_eq!(&request.account[..6], b"p_4i_n");
        assert_eq!(name_request(&m, &l, "").unwrap(), NameRequest::default(), "empty asks for the real name");
        assert!(name_request(&m, &l, "twenty three bytes long").is_err());

        m.put_i32(CLIENT + l.game_state as u64, states::LOGIN_SCREEN);
        assert!(name_request(&m, &l, "Test Player").is_err(), "needs the account logged in");
    }

    #[test]
    fn reads_varps_on_demand() {
        let (m, l) = (logged_in(), layout());
        assert_eq!(varp(&m, &l, 173), Some(173));
        assert_eq!(varp(&m, &l, 5890), Some(5890));
        assert_eq!(varp(&m, &l, 5891), None, "past the last varp");
        let all = varps(&m, &l).unwrap();
        assert_eq!(all.len(), 5891);
        assert_eq!(all[300], 300);
    }

    #[test]
    fn missing_parts_read_as_none() {
        let m = logged_in();
        let l = Layout { found: part::GAME_STATE, ..layout() };
        let state = game_state(&m, &l);
        assert_eq!(state.game_state, Some(states::LOGGED_IN));
        assert_eq!((state.position, state.skills, state.tick_count, state.ui_scale), (None, None, None, None));
        assert_eq!(varp(&m, &l, 173), None);
        assert_eq!(game_state(&m, &Layout::default()), GameState::default());
    }
}
