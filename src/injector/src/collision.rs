pub const REGION_SIZE: i32 = 64;
pub const PLANES: usize = 4;
pub(crate) const TILES: usize = (REGION_SIZE * REGION_SIZE) as usize;
pub(crate) const SQUARES: usize = 1 << 16;

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
    pub const DOOR_NORTH: u32 = 1 << 24;
    pub const DOOR_EAST: u32 = 1 << 25;
    pub const DOOR_SOUTH: u32 = 1 << 26;
    pub const DOOR_WEST: u32 = 1 << 27;
    pub const DOOR_OBJECT: u32 = 1 << 28;
    pub const DOORS: u32 = 0x1f << 24;
    pub const NO_FLOOR: u32 = 1 << 29;

    const DOOR_WALLS: [(u32, u32); 4] = [
        (DOOR_NORTH, WALL_NORTH),
        (DOOR_EAST, WALL_EAST),
        (DOOR_SOUTH, WALL_SOUTH),
        (DOOR_WEST, WALL_WEST),
    ];

    pub(super) fn doors_of(walls: u32) -> u32 {
        DOOR_WALLS
            .iter()
            .filter(|&&(_, wall)| walls & wall != 0)
            .fold(0, |doors, &(door, _)| doors | door)
    }

    pub fn door_blocks(flags: u32) -> u32 {
        let walls = DOOR_WALLS
            .iter()
            .filter(|&&(door, _)| flags & door != 0)
            .fold(0, |walls, &(_, wall)| walls | wall);
        walls | if flags & DOOR_OBJECT != 0 { OBJECT } else { 0 }
    }
}

const DOOR_OPTION: &str = "open";

const SETTING_BLOCKED: u8 = 1;
const SETTING_BRIDGE: u8 = 2;
const SETTING_HAS_FLOOR: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocDef {
    pub size_x: u8,
    pub size_y: u8,
    pub interact_type: u8,
    pub blocks_projectile: bool,
    pub door: bool,
}

impl Default for LocDef {
    fn default() -> Self {
        Self {
            size_x: 1,
            size_y: 1,
            interact_type: 2,
            blocks_projectile: true,
            door: false,
        }
    }
}

pub fn turned(size_x: u8, size_y: u8, rotation: u8) -> (u8, u8) {
    if rotation & 1 == 1 {
        (size_y, size_x)
    } else {
        (size_x, size_y)
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

    fn u16_le(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        (self.at + n <= self.data.len()).then(|| self.at += n)
    }

    fn until_nul(&mut self) -> Option<&'a [u8]> {
        let rest = self.data.get(self.at..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        self.at += end + 1;
        Some(&rest[..end])
    }

    fn string(&mut self) -> Option<()> {
        self.until_nul().map(drop)
    }

    fn text(&mut self) -> Option<String> {
        self.until_nul()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    fn ushort_smart(&mut self) -> Option<u32> {
        if *self.data.get(self.at)? < 128 {
            self.u8().map(u32::from)
        } else {
            self.u16().map(|v| v as u32 - 0x8000)
        }
    }

    fn uint_smart_compat(&mut self) -> Option<u32> {
        let mut total = 0u32;
        loop {
            let v = self.ushort_smart()?;
            if v != 32767 {
                return Some(total.wrapping_add(v));
            }
            total = total.wrapping_add(32767);
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
                        2..=49 => {
                            r.skip(2)?;
                            *tile |= SETTING_HAS_FLOOR;
                        }
                        op @ 50..=81 => *tile = *tile & SETTING_HAS_FLOOR | (op - 49) as u8,
                        _ => *tile |= SETTING_HAS_FLOOR,
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
            position = position.wrapping_add(delta - 1);
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocText {
    pub name: String,
    pub actions: [String; 5],
    pub transforms: Option<Transforms>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transforms {
    pub varbit: Option<u16>,
    pub varp: Option<u16>,
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
    let mut models: Option<Option<u8>> = None;
    let mut options = [false; 5];
    let mut opens = false;
    loop {
        match r.u8()? {
            0 => {
                let interactive = match int1 {
                    Some(v) => v != 0,
                    None => {
                        models.is_some_and(|shape| shape.is_none_or(|s| s == 10))
                            || options.iter().any(|&o| o)
                    }
                };
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
            7 | 40 | 41 => {
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
            28 | 29 | 39 | 69 | 75 | 81 | 91 | 95 | 96 => r.skip(1)?,
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
            op @ (77 | 92 | 97 | 98) => {
                let id = |v: u16| (v != 0xffff).then_some(v);
                let varbit = id(r.u16()?);
                let varp = id(r.u16()?);
                let long = op >= 97;
                let shown = |r: &mut Reader| -> Option<Option<u32>> {
                    Some(if long {
                        let v = (r.u8()? as u32) << 16 | r.u16()? as u32;
                        (v != 0xff_ffff).then_some(v)
                    } else {
                        id(r.u16()?).map(u32::from)
                    })
                };
                let default = if op == 92 || op == 98 {
                    shown(&mut r)?
                } else {
                    None
                };
                let n = r.u8()? as usize;
                let mut ids = Vec::with_capacity(n + 2);
                for _ in 0..=n {
                    ids.push(shown(&mut r)?);
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
    pub const NO_FLOOR: u16 = 0x4000;
    const DOOR_SHIFT: u32 = 24 - 9;

    pub fn opened(self) -> Movement {
        let doors = (self.0 as u32) << Self::DOOR_SHIFT & flag::DOORS;
        let blocks = flag::door_blocks(doors);
        let object = if blocks & flag::OBJECT != 0 {
            Self::BLOCKS
        } else {
            0
        };
        Movement(self.0 & !(blocks as u16 & 0xff) & !object)
    }
}

impl Cell for Movement {
    fn with(self, flags: u32) -> Self {
        let bit = |set: bool, bit: u16| if set { bit } else { 0 };
        Movement(
            self.0
                | (flags & 0xff) as u16
                | ((flags & flag::DOORS) >> Self::DOOR_SHIFT) as u16
                | bit(flags & flag::BLOCKED != 0, Self::BLOCKS)
                | bit(flags & flag::NO_FLOOR != 0, Self::NO_FLOOR),
        )
    }
}

#[derive(Clone)]
pub enum Square<T> {
    Missing,
    Open,
    Same(T),
    Tiles(Box<[T; TILES]>),
}

pub struct WorldCollision<T: Cell = u32> {
    planes: Vec<Vec<Square<T>>>,
}

impl<T: Cell> Default for WorldCollision<T> {
    fn default() -> Self {
        Self {
            planes: (0..PLANES)
                .map(|_| (0..SQUARES).map(|_| Square::Missing).collect())
                .collect(),
        }
    }
}

pub(crate) fn square_index(x: i32, y: i32) -> Option<(usize, usize)> {
    if !(0..REGION_SIZE * 256).contains(&x) || !(0..REGION_SIZE * 256).contains(&y) {
        return None;
    }
    Some((
        region_id(x / REGION_SIZE, y / REGION_SIZE) as usize,
        ((x % REGION_SIZE) * REGION_SIZE + y % REGION_SIZE) as usize,
    ))
}

fn same<T: Cell>(value: T) -> Square<T> {
    if value == T::default() {
        Square::Open
    } else {
        Square::Same(value)
    }
}

impl<T: Cell> WorldCollision<T> {
    pub fn flags(&self, plane: i32, x: i32, y: i32) -> Option<T> {
        let squares = self.planes.get(usize::try_from(plane).ok()?)?;
        let (square, tile) = square_index(x, y)?;
        match &squares[square] {
            Square::Missing => None,
            Square::Open => Some(T::default()),
            Square::Same(value) => Some(*value),
            Square::Tiles(tiles) => Some(tiles[tile]),
        }
    }

    pub fn square(&self, plane: usize, region: u32) -> &Square<T> {
        &self.planes[plane][region as usize]
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

    pub(crate) fn add(&mut self, plane: i32, x: i32, y: i32, flags: u32) {
        let Some(squares) = usize::try_from(plane)
            .ok()
            .and_then(|p| self.planes.get_mut(p))
        else {
            return;
        };
        let Some((square, tile)) = square_index(x, y) else {
            return;
        };
        let square = &mut squares[square];
        match square {
            Square::Open => *square = Square::Tiles(Box::new([T::default(); TILES])),
            Square::Same(value) => *square = Square::Tiles(Box::new([*value; TILES])),
            _ => {}
        }
        if let Square::Tiles(tiles) = square {
            tiles[tile] = tiles[tile].with(flags);
        }
    }

    pub fn shrink(&mut self) {
        for squares in &mut self.planes {
            for square in squares.iter_mut() {
                if let Square::Tiles(tiles) = square
                    && tiles.iter().all(|t| *t == tiles[0])
                {
                    *square = same(tiles[0]);
                }
            }
        }
    }

    pub fn add_terrain_and_locs(
        &mut self,
        region: u32,
        settings: &Settings,
        locs: &[Placement],
        defs: &impl Fn(u32) -> Option<LocDef>,
    ) {
        let base_x = (region >> 8) as i32 * REGION_SIZE;
        let base_y = (region & 0xff) as i32 * REGION_SIZE;
        let bridge = |x: usize, y: usize| settings[1][x][y] & SETTING_BRIDGE != 0;
        for (plane, columns) in settings.iter().enumerate() {
            for (x, column) in columns.iter().enumerate() {
                for (y, &setting) in column.iter().enumerate() {
                    let (wx, wy) = (base_x + x as i32, base_y + y as i32);
                    let walked = plane as i32 - bridge(x, y) as i32;
                    if setting & SETTING_BLOCKED != 0 {
                        self.add(walked, wx, wy, flag::FLOOR);
                    }
                    let above = plane + bridge(x, y) as usize;
                    let floored = settings
                        .get(above)
                        .is_some_and(|p| p[x][y] & SETTING_HAS_FLOOR != 0);
                    if plane > 0 && !floored {
                        self.add(plane as i32, wx, wy, flag::NO_FLOOR);
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
            self.add_loc(
                walked,
                base_x + loc.x as i32,
                base_y + loc.y as i32,
                loc.shape,
                loc.rotation,
                &def,
            );
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
                let (w, h) = turned(def.size_x, def.size_y, rotation);
                let door = if shape == 9 && def.door {
                    flag::DOOR_OBJECT
                } else {
                    0
                };
                let projectile = if def.blocks_projectile {
                    flag::OBJECT_PROJECTILE
                } else {
                    0
                };
                for dx in 0..w as i32 {
                    for dy in 0..h as i32 {
                        self.add(plane, x + dx, y + dy, flag::OBJECT | door | projectile);
                    }
                }
            }
            _ => {}
        }
    }

    fn add_wall(&mut self, plane: i32, x: i32, y: i32, shape: u8, rotation: u8, def: &LocDef) {
        for &(dx, dy, wall) in wall_marks(shape, rotation) {
            let doors = if def.door { flag::doors_of(wall) } else { 0 };
            let projectile = if def.blocks_projectile {
                wall << flag::PROJECTILE_SHIFT
            } else {
                0
            };
            self.add(plane, x + dx, y + dy, wall | projectile | doors);
        }
    }
}

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
        (2, 0) => &[
            (0, 0, WALL_WEST | WALL_NORTH),
            (-1, 0, WALL_EAST),
            (0, 1, WALL_SOUTH),
        ],
        (2, 1) => &[
            (0, 0, WALL_NORTH | WALL_EAST),
            (0, 1, WALL_SOUTH),
            (1, 0, WALL_WEST),
        ],
        (2, 2) => &[
            (0, 0, WALL_EAST | WALL_SOUTH),
            (1, 0, WALL_WEST),
            (0, -1, WALL_NORTH),
        ],
        (2, _) => &[
            (0, 0, WALL_SOUTH | WALL_WEST),
            (0, -1, WALL_NORTH),
            (-1, 0, WALL_EAST),
        ],
        _ => &[],
    }
}

const MAGIC: &[u8; 8] = b"BOTMOV01";
const OPEN: u8 = 1;
const TILES_RLE: u8 = 2;
const END: u8 = 0xff;

impl WorldCollision<Movement> {
    pub fn to_bytes(&self, key: u64) -> Vec<u8> {
        let push_run = |out: &mut Vec<u8>, run: usize, value: Movement| {
            out.extend_from_slice(&(run as u16).to_le_bytes());
            out.extend_from_slice(&value.0.to_le_bytes());
        };
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&key.to_le_bytes());
        for (plane, squares) in self.planes.iter().enumerate() {
            for (region, square) in squares.iter().enumerate() {
                if matches!(square, Square::Missing) {
                    continue;
                }
                out.push(plane as u8);
                out.extend_from_slice(&(region as u16).to_le_bytes());
                match square {
                    Square::Missing => {}
                    Square::Open => out.push(OPEN),
                    Square::Same(value) => {
                        out.push(TILES_RLE);
                        push_run(&mut out, TILES, *value);
                    }
                    Square::Tiles(tiles) => {
                        out.push(TILES_RLE);
                        let mut at = 0;
                        while at < TILES {
                            let value = tiles[at];
                            let run = tiles[at..].iter().take_while(|&&t| t == value).count();
                            push_run(&mut out, run, value);
                            at += run;
                        }
                    }
                }
            }
        }
        out.push(END);
        out
    }

    pub fn from_bytes(bytes: &[u8], key: u64) -> Option<Self> {
        if bytes.get(..8)? != MAGIC || bytes.get(8..16)? != key.to_le_bytes() {
            return None;
        }
        let mut r = Reader {
            data: bytes,
            at: 16,
        };
        let mut world = Self::default();
        loop {
            let plane = r.u8()?;
            if plane == END {
                return Some(world);
            }
            let region = r.u16_le()? as usize;
            let square = match r.u8()? {
                OPEN => Square::Open,
                TILES_RLE => {
                    let run = r.u16_le()? as usize;
                    let value = Movement(r.u16_le()?);
                    if run == TILES {
                        same(value)
                    } else {
                        let mut tiles = Box::new([Movement::default(); TILES]);
                        tiles.get_mut(..run)?.fill(value);
                        let mut at = run.max(1);
                        while at < TILES {
                            let run = r.u16_le()? as usize;
                            let value = Movement(r.u16_le()?);
                            tiles.get_mut(at..at + run)?.fill(value);
                            at += run.max(1);
                        }
                        Square::Tiles(tiles)
                    }
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
        let data = terrain_with(&[((0, 3, 4), &[2, 0x0005, 50]), ((1, 3, 4), &[51])]);
        let settings = decode_terrain(&data).unwrap();
        assert_eq!(settings[0][3][4], SETTING_BLOCKED | SETTING_HAS_FLOOR);
        assert_eq!(settings[1][3][4], SETTING_BRIDGE);
        assert_eq!(settings[0][3][5], 0);
    }

    #[test]
    fn decodes_locs() {
        let position = (1 << 12) | (2 << 6) | 3;
        let mut data = vec![6];
        data.extend_from_slice(&((position + 1) as u16 | 0x8000).to_be_bytes());
        data.push((10 << 2) | 1);
        data.push(0);
        data.push(3);
        data.push(1);
        data.push(2);
        data.push(0);
        data.push(0);
        let locs = decode_locs(&data).unwrap();
        assert_eq!(
            locs,
            vec![
                Placement {
                    id: 5,
                    x: 2,
                    y: 3,
                    plane: 1,
                    shape: 10,
                    rotation: 1
                },
                Placement {
                    id: 8,
                    x: 0,
                    y: 0,
                    plane: 0,
                    shape: 0,
                    rotation: 2
                },
            ]
        );
    }

    #[test]
    fn decodes_loc_collision_fields() {
        let mut data = vec![2];
        data.extend_from_slice(b"Door\0");
        data.extend_from_slice(&[14, 2, 15, 3, 30]);
        data.extend_from_slice(b"Open\0");
        data.extend_from_slice(&[18, 249, 1, 0, 0, 0, 1, 0, 0, 0, 7, 0]);
        assert_eq!(
            decode_loc(&data),
            Some(LocDef {
                size_x: 2,
                size_y: 3,
                interact_type: 2,
                blocks_projectile: false,
                door: true
            })
        );
        let (_, text) = decode_loc_text(&data).unwrap();
        assert_eq!(
            text,
            LocText {
                name: "Door".into(),
                actions: [
                    "Open".into(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new()
                ],
                transforms: None
            }
        );
        assert_eq!(
            decode_loc(&[17, 0]),
            Some(LocDef {
                size_x: 1,
                size_y: 1,
                interact_type: 0,
                blocks_projectile: false,
                door: false
            })
        );
        assert_eq!(decode_loc(&[200, 0]), None);
    }

    #[test]
    fn a_door_is_an_interactive_loc_that_opens() {
        let door_of = |data: &[u8]| decode_loc(data).map(|d| d.door);
        assert_eq!(door_of(&[1, 1, 0, 5, 0, 0]), Some(false));
        let mut door = vec![1, 1, 0, 5, 0, 30];
        door.extend_from_slice(b"Open\0");
        door.push(0);
        assert_eq!(door_of(&door), Some(true));
        let mut search = vec![1, 1, 0, 5, 0, 30];
        search.extend_from_slice(b"Search\0");
        search.push(0);
        assert_eq!(door_of(&search), Some(false));
        let mut not = vec![19, 0, 30];
        not.extend_from_slice(b"Open\0");
        not.push(0);
        assert_eq!(door_of(&not), Some(false));
        let mut shape_ten = vec![1, 1, 0, 5, 10, 31];
        shape_ten.extend_from_slice(b"Open\0");
        shape_ten.push(0);
        assert_eq!(door_of(&shape_ten), Some(true));
    }

    #[test]
    fn decodes_transforms_with_long_ids() {
        let data = [
            97, 0xff, 0xff, 0, 116, 2, 0, 0x71, 2, 0xff, 0xff, 0xff, 0, 0x71, 3, 0,
        ];
        let (_, text) = decode_loc_text(&data).unwrap();
        let t = text.transforms.unwrap();
        assert_eq!((t.varbit, t.varp), (None, Some(116)));
        assert_eq!(t.pick(Some(0)), Some(28930));
        assert_eq!(t.pick(Some(1)), None);
        assert_eq!(t.pick(Some(2)), Some(28931));
    }

    #[test]
    fn decodes_transforms_with_long_ids_and_a_default() {
        let data = [
            98, 0x25, 0x6a, 0xff, 0xff, 0, 0x38, 0xb1, 1, 0, 0x38, 0xb1, 0, 0x88, 0x0a, 0,
        ];
        let (_, text) = decode_loc_text(&data).unwrap();
        let t = text.transforms.unwrap();
        assert_eq!((t.varbit, t.varp), (Some(0x256a), None));
        assert_eq!(t.pick(Some(1)), Some(34826));
        assert_eq!(t.pick(Some(7)), Some(14513));
    }

    #[test]
    fn decodes_and_picks_transforms() {
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
        let locs = [
            Placement {
                id: 1,
                x: 5,
                y: 5,
                plane: 0,
                shape: 0,
                rotation: 2,
            },
            Placement {
                id: 2,
                x: 5,
                y: 6,
                plane: 0,
                shape: 0,
                rotation: 2,
            },
        ];
        let defs = |id: u32| {
            Some(LocDef {
                door: id == 1,
                ..LocDef::default()
            })
        };
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
            Placement {
                id: 1,
                x: 5,
                y: 5,
                plane: 0,
                shape: 0,
                rotation: 1,
            },
            Placement {
                id: 2,
                x: 20,
                y: 20,
                plane: 0,
                shape: 10,
                rotation: 1,
            },
            Placement {
                id: 3,
                x: 10,
                y: 10,
                plane: 1,
                shape: 10,
                rotation: 0,
            },
        ];
        let defs = |id: u32| match id {
            2 => Some(LocDef {
                size_x: 2,
                size_y: 1,
                interact_type: 2,
                blocks_projectile: false,
                ..LocDef::default()
            }),
            _ => Some(LocDef::default()),
        };
        let mut world = WorldCollision::<u32>::default();
        world.add_square(region);
        world.add_terrain_and_locs(region, &settings, &locs, &defs);
        assert_eq!(
            world.flags(0, bx + 5, by + 5),
            Some(flag::WALL_NORTH | flag::WALL_NORTH << flag::PROJECTILE_SHIFT)
        );
        assert_eq!(
            world.flags(0, bx + 5, by + 6),
            Some(flag::WALL_SOUTH | flag::WALL_SOUTH << flag::PROJECTILE_SHIFT)
        );
        assert_eq!(world.flags(0, bx + 20, by + 20), Some(flag::OBJECT));
        assert_eq!(world.flags(0, bx + 20, by + 21), Some(flag::OBJECT));
        assert_eq!(world.flags(0, bx + 21, by + 20), Some(0));
        assert_eq!(
            world.flags(0, bx + 10, by + 10),
            Some(flag::OBJECT | flag::OBJECT_PROJECTILE)
        );
        assert_eq!(world.flags(1, bx + 10, by + 10), Some(flag::NO_FLOOR));
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
        assert_eq!(
            back.flags(0, 50 * 64 + 3, 50 * 64 + 4),
            Some(Movement(Movement::BLOCKS | flag::WALL_EAST as u16))
        );
        assert_eq!(back.flags(0, 51 * 64, 50 * 64), Some(Movement(0)));
        assert_eq!(back.flags(0, 52 * 64, 50 * 64), None);
    }

    #[test]
    fn a_square_of_one_value_is_kept_as_that_value() {
        let region = region_id(50, 50);
        let mut world = WorldCollision::<Movement>::default();
        world.add_square(region);
        for x in 0..64 {
            for y in 0..64 {
                world.add(1, 50 * 64 + x, 50 * 64 + y, flag::NO_FLOOR);
            }
        }
        world.shrink();
        let no_floor = Movement::default().with(flag::NO_FLOOR);
        assert!(matches!(world.square(1, region), Square::Same(v) if *v == no_floor));
        assert_eq!(world.flags(1, 50 * 64 + 9, 50 * 64 + 9), Some(no_floor));

        let back = WorldCollision::from_bytes(&world.to_bytes(7), 7).unwrap();
        assert!(matches!(back.square(1, region), Square::Same(v) if *v == no_floor));
        assert!(matches!(back.square(0, region), Square::Open));

        world.add(1, 50 * 64, 50 * 64, flag::WALL_EAST);
        assert!(matches!(world.square(1, region), Square::Tiles(_)));
        assert_eq!(world.flags(1, 50 * 64 + 9, 50 * 64 + 9), Some(no_floor));
    }
}
