

pub const REGION_SIZE: i32 = 64;
pub const PLANES: usize = 4;
const TILES: usize = (REGION_SIZE * REGION_SIZE) as usize;

pub mod flag {
    pub const WALL_NORTH_WEST: u32 = 0x1;
    pub const WALL_NORTH: u32 = 0x2;
    pub const WALL_NORTH_EAST: u32 = 0x4;
    pub const WALL_EAST: u32 = 0x8;
    pub const WALL_SOUTH_EAST: u32 = 0x10;
    pub const WALL_SOUTH: u32 = 0x20;
    pub const WALL_SOUTH_WEST: u32 = 0x40;
    pub const WALL_WEST: u32 = 0x80;
    pub const OBJECT: u32 = 0x100;
    pub const PROJECTILE_SHIFT: u32 = 9;
    pub const OBJECT_PROJECTILE: u32 = 0x20000;
    pub const FLOOR_DECORATION: u32 = 0x40000;
    pub const FLOOR: u32 = 0x200000;
    pub const BLOCKED: u32 = OBJECT | FLOOR_DECORATION | FLOOR;
    // Not the client's: which of a tile's walls belong to a closed door (the cache has every door closed), and a door
    // that fills the tile (a diagonal one). The game's route masks leave these bits alone.
    pub const DOOR_NORTH: u32 = 1 << 24;
    pub const DOOR_EAST: u32 = 1 << 25;
    pub const DOOR_SOUTH: u32 = 1 << 26;
    pub const DOOR_WEST: u32 = 1 << 27;
    pub const DOOR_OBJECT: u32 = 1 << 28;
    pub const DOORS: u32 = 0x1f << 24;

    /// The door bit of a straight wall's bit; 0 for a corner.
    pub fn door_of(wall: u32) -> u32 {
        match wall {
            WALL_NORTH => DOOR_NORTH,
            WALL_EAST => DOOR_EAST,
            WALL_SOUTH => DOOR_SOUTH,
            WALL_WEST => DOOR_WEST,
            _ => 0,
        }
    }

    /// The walls and the block a tile's door bits stand for: what opening its doors takes away.
    pub fn door_blocks(flags: u32) -> u32 {
        let mut blocks = 0;
        for (door, wall) in [(DOOR_NORTH, WALL_NORTH), (DOOR_EAST, WALL_EAST), (DOOR_SOUTH, WALL_SOUTH), (DOOR_WEST, WALL_WEST)] {
            if flags & door != 0 {
                blocks |= wall;
            }
        }
        if flags & DOOR_OBJECT != 0 {
            blocks |= OBJECT;
        }
        blocks
    }
}

/// The menu option that opens a door, any case.
pub const DOOR_OPTION: &str = "open";

const SETTING_BLOCKED: u8 = 1;
const SETTING_BRIDGE: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocDef {
    pub size_x: u8,
    pub size_y: u8,
    pub interact_type: u8,
    pub blocks_projectile: bool,
    // Java's `int1` (op 19, else whether it has models of shape 10 or any option): what makes the minimap draw a wall red
    pub interactive: bool,
    // interactive with a `DOOR_OPTION`: a wall of it is a closed door
    pub door: bool,
}

impl Default for LocDef {
    fn default() -> Self {
        Self { size_x: 1, size_y: 1, interact_type: 2, blocks_projectile: true, interactive: false, door: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub id: u32,
    pub x: u8,
    pub y: u8,
    pub plane: u8,
    pub shape: u8,
    pub rotation: u8,
}

pub type Settings = [[[u8; REGION_SIZE as usize]; REGION_SIZE as usize]; PLANES];

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let v = *self.data.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(((self.u8()? as u16) << 8) | self.u8()? as u16)
    }


    fn skip(&mut self, n: usize) -> Option<()> {
        (self.at + n <= self.data.len()).then(|| self.at += n)
    }

    fn string(&mut self) -> Option<()> {
        let end = self.data.get(self.at..)?.iter().position(|&b| b == 0)?;
        self.at += end + 1;
        Some(())
    }

    fn text(&mut self) -> Option<String> {
        let end = self.data.get(self.at..)?.iter().position(|&b| b == 0)?;
        let text = String::from_utf8_lossy(&self.data[self.at..self.at + end]).into_owned();
        self.at += end + 1;
        Some(text)
    }

    fn ushort_smart(&mut self) -> Option<u32> {
        if *self.data.get(self.at)? < 128 { self.u8().map(u32::from) } else { self.u16().map(|v| v as u32 - 0x8000) }
    }

    fn uint_smart_compat(&mut self) -> Option<u32> {
        let mut total = 0;
        loop {
            let v = self.ushort_smart()?;
            if v != 32767 {
                return Some(total + v);
            }
            total += 32767;
        }
    }
}

pub fn decode_terrain(data: &[u8]) -> Option<Box<Settings>> {
    let mut settings = Box::new([[[0u8; REGION_SIZE as usize]; REGION_SIZE as usize]; PLANES]);
    let mut r = Reader::new(data);
    for plane in settings.iter_mut() {
        for column in plane.iter_mut() {
            for tile in column.iter_mut() {
                loop {
                    match r.u16()? {
                        0 => break,
                        1 => {
                            r.u8()?;
                            break;
                        }
                        2..=49 => r.skip(2)?,
                        op @ 50..=81 => *tile = (op - 49) as u8,
                        _ => {}
                    }
                }
            }
        }
    }
    Some(settings)
}

pub fn decode_locs(data: &[u8]) -> Option<Vec<Placement>> {
    let mut r = Reader::new(data);
    let mut locs = Vec::new();
    let mut id: i64 = -1;
    loop {
        let delta = r.uint_smart_compat()?;
        if delta == 0 {
            return Some(locs);
        }
        id += delta as i64;
        let mut position = 0u32;
        loop {
            let delta = r.ushort_smart()?;
            if delta == 0 {
                break;
            }
            position += delta - 1;
            let attributes = r.u8()?;
            locs.push(Placement {
                id: id as u32,
                x: ((position >> 6) & 63) as u8,
                y: (position & 63) as u8,
                plane: ((position >> 12) & 3) as u8,
                shape: attributes >> 2,
                rotation: attributes & 3,
            });
        }
    }
}

/// What a loc definition says about itself in the game: the name and the five right-click options (empty when it has none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocText {
    pub name: String,
    pub actions: [String; 5],
    pub transforms: Option<Transforms>,
}

/// A loc that shows as another one depending on a varbit or a varp (Java's `ObjectComposition.transform`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transforms {
    pub varbit: Option<u16>,
    pub varp: Option<u16>,
    // by value; the last is for any other value. `None` shows nothing.
    pub ids: Vec<Option<u32>>,
}

impl Transforms {
    pub fn pick(&self, value: Option<i32>) -> Option<u32> {
        let last = self.ids.len().checked_sub(1)?;
        match value {
            Some(v) if v >= 0 && (v as usize) < last => self.ids[v as usize],
            _ => self.ids[last],
        }
    }
}

pub fn decode_loc(data: &[u8]) -> Option<LocDef> {
    decode_loc_with(data, None)
}

pub fn decode_loc_text(data: &[u8]) -> Option<(LocDef, LocText)> {
    let mut text = LocText::default();
    let def = decode_loc_with(data, Some(&mut text))?;
    Some((def, text))
}

fn decode_loc_with(data: &[u8], mut text: Option<&mut LocText>) -> Option<LocDef> {
    let mut def = LocDef::default();
    let mut r = Reader::new(data);
    let mut int1: Option<u8> = None;
    // whether it has models, and the shape of the first when they say
    let mut models: Option<Option<u8>> = None;
    let mut options = [false; 5];
    let mut opens = false;
    loop {
        match r.u8()? {
            0 => {
                let interactive = match int1 {
                    Some(v) => v != 0,
                    None => models.is_some_and(|shape| shape.is_none_or(|s| s == 10)) || options.iter().any(|&o| o),
                };
                def.interactive = interactive;
                def.door = interactive && opens;
                return Some(def);
            }
            1 => {
                let n = r.u8()? as usize;
                if n > 0 {
                    r.skip(2)?;
                    models = Some(Some(r.u8()?));
                    r.skip((n - 1) * 3)?;
                }
            }
            2 => match text.as_deref_mut() {
                Some(text) => text.name = r.text()?,
                None => r.string()?,
            },
            5 => {
                let n = r.u8()? as usize;
                if n > 0 {
                    models = Some(None);
                }
                r.skip(n * 2)?;
            }
            6 => {
                let n = r.u8()? as usize;
                r.skip(n * 5)?;
            }
            7 => {
                let n = r.u8()? as usize;
                r.skip(n * 4)?;
            }
            14 => def.size_x = r.u8()?,
            15 => def.size_y = r.u8()?,
            17 => {
                def.interact_type = 0;
                def.blocks_projectile = false;
            }
            18 => def.blocks_projectile = false,
            19 => int1 = Some(r.u8()?),
            28 | 29 | 39 | 69 | 75 | 91 | 95 | 96 => r.skip(1)?,
            21 | 22 | 23 | 62 | 64 | 73 | 74 | 89 | 90 | 94 => {}
            24 | 61 | 65 | 66 | 67 | 68 | 70 | 71 | 72 | 82 => r.skip(2)?,
            27 => def.interact_type = 1,
            op @ 30..=34 => {
                let action = r.text()?;
                let shown = !action.eq_ignore_ascii_case("hidden");
                options[(op - 30) as usize] = shown;
                opens |= shown && action.eq_ignore_ascii_case(DOOR_OPTION);
                if let Some(text) = text.as_deref_mut() {
                    text.actions[(op - 30) as usize] = if shown { action } else { String::new() };
                }
            }
            40 | 41 => {
                let n = r.u8()? as usize;
                r.skip(n * 4)?;
            }
            op @ (77 | 92) => {
                let id = |v: u16| (v != 0xffff).then_some(v);
                let varbit = id(r.u16()?);
                let varp = id(r.u16()?);
                let default = if op == 92 { id(r.u16()?).map(u32::from) } else { None };
                let n = r.u8()? as usize;
                let mut ids = Vec::with_capacity(n + 2);
                for _ in 0..=n {
                    ids.push(id(r.u16()?).map(u32::from));
                }
                ids.push(default);
                if let Some(text) = text.as_deref_mut() {
                    text.transforms = Some(Transforms { varbit, varp, ids });
                }
            }
            78 => r.skip(4)?,
            79 => {
                r.skip(6)?;
                let n = r.u8()? as usize;
                r.skip(n * 2)?;
            }
            81 => r.skip(1)?,
            93 => r.skip(6)?,
            100 => {
                r.skip(2)?;
                r.string()?;
            }
            101 => {
                r.skip(13)?;
                r.string()?;
            }
            102 => {
                r.skip(15)?;
                r.string()?;
            }
            249 => {
                let n = r.u8()? as usize;
                for _ in 0..n {
                    let is_string = r.u8()? == 1;
                    r.skip(3)?;
                    if is_string { r.string()? } else { r.skip(4)? }
                }
            }
            _ => return None,
        }
    }
}

pub fn region_id(region_x: i32, region_y: i32) -> u32 {
    ((region_x as u32) << 8) | region_y as u32
}

pub trait Cell: Copy + Default + PartialEq {
    fn with(self, flags: u32) -> Self;
}

impl Cell for u32 {
    fn with(self, flags: u32) -> Self {
        self | flags
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Movement(pub u16);

impl Movement {
    pub const BLOCKS: u16 = 0x100;
    // the `flag::DOORS` bits, moved down
    const DOOR_SHIFT: u32 = 24 - 9;

    /// What it blocks with its closed doors open.
    pub fn opened(self) -> Movement {
        let doors = (self.0 as u32) << Self::DOOR_SHIFT & flag::DOORS;
        let blocks = flag::door_blocks(doors);
        Movement(self.0 & !(blocks as u16 & 0xff) & !if blocks & flag::OBJECT != 0 { Self::BLOCKS } else { 0 })
    }
}

impl Cell for Movement {
    fn with(self, flags: u32) -> Self {
        let doors = ((flags & flag::DOORS) >> Self::DOOR_SHIFT) as u16;
        Movement(self.0 | (flags & 0xff) as u16 | doors | if flags & flag::BLOCKED != 0 { Self::BLOCKS } else { 0 })
    }
}

#[derive(Clone)]
pub enum Square<T> {
    Missing,
    Open,
    Tiles(Box<[T; TILES]>),
}

const SQUARES: usize = 1 << 16;

pub struct WorldCollision<T: Cell = u32> {
    planes: Vec<Vec<Square<T>>>,
}

impl<T: Cell> Default for WorldCollision<T> {
    fn default() -> Self {
        Self { planes: (0..PLANES).map(|_| (0..SQUARES).map(|_| Square::Missing).collect()).collect() }
    }
}

fn square_index(x: i32, y: i32) -> Option<(usize, usize)> {
    if !(0..REGION_SIZE * 256).contains(&x) || !(0..REGION_SIZE * 256).contains(&y) {
        return None;
    }
    Some((region_id(x / REGION_SIZE, y / REGION_SIZE) as usize, ((x % REGION_SIZE) * REGION_SIZE + y % REGION_SIZE) as usize))
}

impl<T: Cell> WorldCollision<T> {
    pub fn flags(&self, plane: i32, x: i32, y: i32) -> Option<T> {
        let squares = self.planes.get(usize::try_from(plane).ok()?)?;
        let (square, tile) = square_index(x, y)?;
        match &squares[square] {
            Square::Missing => None,
            Square::Open => Some(T::default()),
            Square::Tiles(tiles) => Some(tiles[tile]),
        }
    }

    pub fn square(&self, plane: usize, region: u32) -> &Square<T> {
        &self.planes[plane][region as usize]
    }

    pub fn set_square(&mut self, plane: usize, region: u32, square: Square<T>) {
        self.planes[plane][region as usize] = square;
    }

    pub fn add_square(&mut self, region: u32) {
        for squares in &mut self.planes {
            if let Some(square @ Square::Missing) = squares.get_mut(region as usize) {
                *square = Square::Open;
            }
        }
    }

    pub fn remove_square(&mut self, region: u32) {
        for squares in &mut self.planes {
            if let Some(square) = squares.get_mut(region as usize) {
                *square = Square::Missing;
            }
        }
    }

    fn add(&mut self, plane: i32, x: i32, y: i32, flags: u32) {
        let Some(squares) = usize::try_from(plane).ok().and_then(|p| self.planes.get_mut(p)) else { return };
        let Some((square, tile)) = square_index(x, y) else { return };
        let square = &mut squares[square];
        if let Square::Open = square {
            *square = Square::Tiles(Box::new([T::default(); TILES]));
        }
        if let Square::Tiles(tiles) = square {
            tiles[tile] = tiles[tile].with(flags);
        }
    }

    pub fn shrink(&mut self) {
        for squares in &mut self.planes {
            for square in squares.iter_mut() {
                if let Square::Tiles(tiles) = square
                    && tiles.iter().all(|t| *t == T::default()) {
                        *square = Square::Open;
                    }
            }
        }
    }

    pub fn add_terrain_and_locs(&mut self, region: u32, settings: &Settings, locs: &[Placement], defs: &impl Fn(u32) -> Option<LocDef>) {
        let base_x = (region >> 8) as i32 * REGION_SIZE;
        let base_y = (region & 0xff) as i32 * REGION_SIZE;
        let bridge = |x: usize, y: usize| settings[1][x][y] & SETTING_BRIDGE != 0;
        for (plane, columns) in settings.iter().enumerate() {
            for (x, column) in columns.iter().enumerate() {
                for (y, &setting) in column.iter().enumerate() {
                    if setting & SETTING_BLOCKED != 0 {
                        let walked = plane as i32 - bridge(x, y) as i32;
                        self.add(walked, base_x + x as i32, base_y + y as i32, flag::FLOOR);
                    }
                }
            }
        }
        for loc in locs {
            let walked = loc.plane as i32 - bridge(loc.x as usize, loc.y as usize) as i32;
            if walked < 0 {
                continue;
            }
            let Some(def) = defs(loc.id) else { continue };
            self.add_loc(walked, base_x + loc.x as i32, base_y + loc.y as i32, loc.shape, loc.rotation, &def);
        }
    }

    fn add_loc(&mut self, plane: i32, x: i32, y: i32, shape: u8, rotation: u8, def: &LocDef) {
        match shape {
            22 => {
                if def.interact_type == 1 {
                    self.add(plane, x, y, flag::FLOOR_DECORATION);
                }
            }
            0..=3 if def.interact_type != 0 => self.add_wall(plane, x, y, shape, rotation, def),
            9..=21 if def.interact_type != 0 => {
                let (w, h) = if rotation == 1 || rotation == 3 { (def.size_y, def.size_x) } else { (def.size_x, def.size_y) };
                let door = if shape == 9 && def.door { flag::DOOR_OBJECT } else { 0 };
                let flags = flag::OBJECT | door | if def.blocks_projectile { flag::OBJECT_PROJECTILE } else { 0 };
                for dx in 0..w as i32 {
                    for dy in 0..h as i32 {
                        self.add(plane, x + dx, y + dy, flags);
                    }
                }
            }
            _ => {}
        }
    }

    fn add_wall(&mut self, plane: i32, x: i32, y: i32, shape: u8, rotation: u8, def: &LocDef) {
        for &(dx, dy, wall) in wall_marks(shape, rotation) {
            let doors = if def.door { (0..8).map(|bit| flag::door_of(wall & (1 << bit))).fold(0, |a, b| a | b) } else { 0 };
            let flags = wall | if def.blocks_projectile { wall << flag::PROJECTILE_SHIFT } else { 0 } | doors;
            self.add(plane, x + dx, y + dy, flags);
        }
    }
}

/// The wall bits a wall of `shape` and `rotation` puts on its tile and the tiles across it, as (dx, dy, bits).
pub fn wall_marks(shape: u8, rotation: u8) -> &'static [(i32, i32, u32)] {
    use flag::*;
    match (shape, rotation & 3) {
            (0, 0) => &[(0, 0, WALL_WEST), (-1, 0, WALL_EAST)],
            (0, 1) => &[(0, 0, WALL_NORTH), (0, 1, WALL_SOUTH)],
            (0, 2) => &[(0, 0, WALL_EAST), (1, 0, WALL_WEST)],
            (0, _) => &[(0, 0, WALL_SOUTH), (0, -1, WALL_NORTH)],
            (1 | 3, 0) => &[(0, 0, WALL_NORTH_WEST), (-1, 1, WALL_SOUTH_EAST)],
            (1 | 3, 1) => &[(0, 0, WALL_NORTH_EAST), (1, 1, WALL_SOUTH_WEST)],
            (1 | 3, 2) => &[(0, 0, WALL_SOUTH_EAST), (1, -1, WALL_NORTH_WEST)],
            (1 | 3, _) => &[(0, 0, WALL_SOUTH_WEST), (-1, -1, WALL_NORTH_EAST)],
            (2, 0) => &[(0, 0, WALL_WEST | WALL_NORTH), (-1, 0, WALL_EAST), (0, 1, WALL_SOUTH)],
            (2, 1) => &[(0, 0, WALL_NORTH | WALL_EAST), (0, 1, WALL_SOUTH), (1, 0, WALL_WEST)],
            (2, 2) => &[(0, 0, WALL_EAST | WALL_SOUTH), (1, 0, WALL_WEST), (0, -1, WALL_NORTH)],
            (2, _) => &[(0, 0, WALL_SOUTH | WALL_WEST), (0, -1, WALL_NORTH), (-1, 0, WALL_EAST)],
            _ => &[],
    }
}

const MAGIC: &[u8; 8] = b"BOTMOV01";
const OPEN: u8 = 1;
const TILES_RLE: u8 = 2;
const END: u8 = 0xff;

impl WorldCollision<Movement> {
    pub fn to_bytes(&self, key: u64) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&key.to_le_bytes());
        for (plane, squares) in self.planes.iter().enumerate() {
            for (region, square) in squares.iter().enumerate() {
                let tiles = match square {
                    Square::Missing => continue,
                    Square::Open => None,
                    Square::Tiles(tiles) => Some(tiles),
                };
                out.push(plane as u8);
                out.extend_from_slice(&(region as u16).to_le_bytes());
                let Some(tiles) = tiles else {
                    out.push(OPEN);
                    continue;
                };
                out.push(TILES_RLE);
                let mut at = 0;
                while at < TILES {
                    let value = tiles[at];
                    let run = tiles[at..].iter().take_while(|&&t| t == value).count();
                    out.extend_from_slice(&(run as u16).to_le_bytes());
                    out.extend_from_slice(&value.0.to_le_bytes());
                    at += run;
                }
            }
        }
        out.push(END);
        out
    }

    pub fn from_bytes(bytes: &[u8], key: u64) -> Option<Self> {
        let mut r = Reader::new(bytes);
        if bytes.get(..8)? != MAGIC || bytes.get(8..16)? != key.to_le_bytes() {
            return None;
        }
        r.at = 16;
        let le16 = |r: &mut Reader| Some(u16::from_le_bytes([r.u8()?, r.u8()?]));
        let mut world = Self::default();
        loop {
            let plane = r.u8()?;
            if plane == END {
                return Some(world);
            }
            let region = le16(&mut r)? as usize;
            let square = match r.u8()? {
                OPEN => Square::Open,
                TILES_RLE => {
                    let mut tiles = Box::new([Movement::default(); TILES]);
                    let mut at = 0;
                    while at < TILES {
                        let run = le16(&mut r)? as usize;
                        let value = Movement(le16(&mut r)?);
                        tiles.get_mut(at..at + run)?.fill(value);
                        at += run.max(1);
                    }
                    Square::Tiles(tiles)
                }
                _ => return None,
            };
            *world.planes.get_mut(plane as usize)?.get_mut(region)? = square;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TileOps<'a> = ((usize, usize, usize), &'a [u16]);

    fn terrain_with(tiles: &[TileOps]) -> Vec<u8> {
        let mut out = Vec::new();
        for plane in 0..PLANES {
            for x in 0..64 {
                for y in 0..64 {
                    if let Some((_, ops)) = tiles.iter().find(|(at, _)| *at == (plane, x, y)) {
                        for op in *ops {
                            out.extend_from_slice(&op.to_be_bytes());
                        }
                    }
                    out.extend_from_slice(&0u16.to_be_bytes());
                }
            }
        }
        out.push(0);
        out
    }

    #[test]
    fn decodes_terrain_settings() {
        // (0, 3, 4): an overlay (op 2 + u16) then settings 1; (1, 3, 4): bridge (settings 2).
        let data = terrain_with(&[((0, 3, 4), &[2, 0x0005, 50]), ((1, 3, 4), &[51])]);
        let settings = decode_terrain(&data).unwrap();
        assert_eq!(settings[0][3][4], SETTING_BLOCKED);
        assert_eq!(settings[1][3][4], SETTING_BRIDGE);
        assert_eq!(settings[0][3][5], 0);
    }

    #[test]
    fn decodes_locs() {
        // id 5 at plane 1, x 2, y 3, shape 10 rotation 1; id 8 at (0, 0, 0), shape 0 rotation 2.
        let position = (1 << 12) | (2 << 6) | 3;
        let mut data = vec![6]; // id delta: -1 + 6 = 5
        data.extend_from_slice(&((position + 1) as u16 | 0x8000).to_be_bytes());
        data.push((10 << 2) | 1);
        data.push(0);
        data.push(3); // 5 + 3 = 8
        data.push(1);
        data.push(2);
        data.push(0);
        data.push(0);
        let locs = decode_locs(&data).unwrap();
        assert_eq!(locs, vec![
            Placement { id: 5, x: 2, y: 3, plane: 1, shape: 10, rotation: 1 },
            Placement { id: 8, x: 0, y: 0, plane: 0, shape: 0, rotation: 2 },
        ]);
    }

    #[test]
    fn decodes_loc_collision_fields() {
        // name, size 2 x 3, op "Open", blocks nothing for projectiles, params, end.
        let mut data = vec![2];
        data.extend_from_slice(b"Door\0");
        data.extend_from_slice(&[14, 2, 15, 3, 30]);
        data.extend_from_slice(b"Open\0");
        data.extend_from_slice(&[18, 249, 1, 0, 0, 0, 1, 0, 0, 0, 7, 0]);
        assert_eq!(decode_loc(&data), Some(LocDef { size_x: 2, size_y: 3, interact_type: 2, blocks_projectile: false, interactive: true, door: true }));
        let (_, text) = decode_loc_text(&data).unwrap();
        assert_eq!(
            text,
            LocText { name: "Door".into(), actions: ["Open".into(), String::new(), String::new(), String::new(), String::new()], transforms: None }
        );
        assert_eq!(decode_loc(&[17, 0]), Some(LocDef { size_x: 1, size_y: 1, interact_type: 0, blocks_projectile: false, interactive: false, door: false }));
        assert_eq!(decode_loc(&[200, 0]), None);
    }

    #[test]
    fn interactive_is_the_minimaps_red_wall_rule() {
        // a wall model (op 1, shape 0) and no options: a plain wall
        let wall = [1, 1, 0, 5, 0, 0];
        assert_eq!(decode_loc(&wall).map(|d| (d.interactive, d.door)), Some((false, false)));
        // the same with "Open": a door; with only "Search": interactive, not a door
        let mut door = vec![1, 1, 0, 5, 0, 30];
        door.extend_from_slice(b"Open\0");
        door.push(0);
        assert_eq!(decode_loc(&door).map(|d| (d.interactive, d.door)), Some((true, true)));
        let mut search = vec![1, 1, 0, 5, 0, 30];
        search.extend_from_slice(b"Search\0");
        search.push(0);
        assert_eq!(decode_loc(&search).map(|d| (d.interactive, d.door)), Some((true, false)));
        // op 19 says otherwise
        let mut not = vec![19, 0, 30];
        not.extend_from_slice(b"Open\0");
        not.push(0);
        assert_eq!(decode_loc(&not).map(|d| (d.interactive, d.door)), Some((false, false)));
    }

    #[test]
    fn decodes_and_picks_transforms() {
        // op 92: varbit 7, no varp, default 30, ids [10, none]
        let data = [92, 0, 7, 0xff, 0xff, 0, 30, 1, 0, 10, 0xff, 0xff, 0];
        let (_, text) = decode_loc_text(&data).unwrap();
        let t = text.transforms.unwrap();
        assert_eq!((t.varbit, t.varp), (Some(7), None));
        assert_eq!(t.pick(Some(0)), Some(10));
        assert_eq!(t.pick(Some(1)), None);
        assert_eq!(t.pick(Some(5)), Some(30));
        assert_eq!(t.pick(None), Some(30));
    }

    #[test]
    fn marks_doors_and_opens_them_for_paths() {
        let region = region_id(50, 50);
        let (bx, by) = (50 * 64, 50 * 64);
        let settings = Box::new([[[0u8; 64]; 64]; PLANES]);
        // a door on the east edge of (5, 5), and a plain wall on the east edge of (5, 6)
        let locs = [Placement { id: 1, x: 5, y: 5, plane: 0, shape: 0, rotation: 2 }, Placement { id: 2, x: 5, y: 6, plane: 0, shape: 0, rotation: 2 }];
        let defs = |id: u32| Some(LocDef { door: id == 1, interactive: id == 1, ..LocDef::default() });
        let mut world = WorldCollision::<Movement>::default();
        world.add_square(region);
        world.add_terrain_and_locs(region, &settings, &locs, &defs);
        let door = world.flags(0, bx + 5, by + 5).unwrap();
        assert_eq!(door.opened().0 & flag::WALL_EAST as u16, 0);
        assert_ne!(door.0 & flag::WALL_EAST as u16, 0);
        let wall = world.flags(0, bx + 5, by + 6).unwrap();
        assert_eq!(wall.opened(), wall);
        let mut u32_world = WorldCollision::<u32>::default();
        u32_world.add_square(region);
        u32_world.add_terrain_and_locs(region, &settings, &locs, &defs);
        let flags = u32_world.flags(0, bx + 6, by + 5).unwrap();
        assert_eq!(flag::door_blocks(flags), flag::WALL_WEST);
    }

    #[test]
    fn places_walls_footprints_and_bridges() {
        let region = region_id(50, 50);
        let (bx, by) = (50 * 64, 50 * 64);
        let mut settings = Box::new([[[0u8; 64]; 64]; PLANES]);
        settings[1][10][10] = SETTING_BRIDGE;
        let locs = [
            // A wall on the north edge of (5, 5), blocking projectiles.
            Placement { id: 1, x: 5, y: 5, plane: 0, shape: 0, rotation: 1 },
            // A 2 x 1 object turned a quarter: 1 x 2 at (20, 20).
            Placement { id: 2, x: 20, y: 20, plane: 0, shape: 10, rotation: 1 },
            // An object on plane 1 over the bridge tile: walked on plane 0.
            Placement { id: 3, x: 10, y: 10, plane: 1, shape: 10, rotation: 0 },
        ];
        let defs = |id: u32| match id {
            2 => Some(LocDef { size_x: 2, size_y: 1, interact_type: 2, blocks_projectile: false, ..LocDef::default() }),
            _ => Some(LocDef::default()),
        };
        let mut world = WorldCollision::<u32>::default();
        world.add_square(region);
        world.add_terrain_and_locs(region, &settings, &locs, &defs);
        assert_eq!(world.flags(0, bx + 5, by + 5), Some(flag::WALL_NORTH | flag::WALL_NORTH << flag::PROJECTILE_SHIFT));
        assert_eq!(world.flags(0, bx + 5, by + 6), Some(flag::WALL_SOUTH | flag::WALL_SOUTH << flag::PROJECTILE_SHIFT));
        assert_eq!(world.flags(0, bx + 20, by + 20), Some(flag::OBJECT));
        assert_eq!(world.flags(0, bx + 20, by + 21), Some(flag::OBJECT));
        assert_eq!(world.flags(0, bx + 21, by + 20), Some(0));
        assert_eq!(world.flags(0, bx + 10, by + 10), Some(flag::OBJECT | flag::OBJECT_PROJECTILE));
        assert_eq!(world.flags(1, bx + 10, by + 10), Some(0));
        assert_eq!(world.flags(0, bx - 1, by), None);
    }

    #[test]
    fn movement_round_trips_through_bytes() {
        let region = region_id(50, 50);
        let mut world = WorldCollision::<Movement>::default();
        world.add_square(region);
        world.add_square(region_id(51, 50));
        world.add(0, 50 * 64 + 3, 50 * 64 + 4, flag::FLOOR | flag::WALL_EAST);
        world.shrink();
        let bytes = world.to_bytes(7);
        assert!(WorldCollision::from_bytes(&bytes, 8).is_none());
        let back = WorldCollision::from_bytes(&bytes, 7).unwrap();
        assert_eq!(back.flags(0, 50 * 64 + 3, 50 * 64 + 4), Some(Movement(Movement::BLOCKS | flag::WALL_EAST as u16)));
        assert_eq!(back.flags(0, 51 * 64, 50 * 64), Some(Movement(0)));
        assert_eq!(back.flags(0, 52 * 64, 50 * 64), None);
    }
}
