// Devtools overlays that are plain logic over client data, kept apart from the Windows overlay so they can be tested.

use crate::collision::flag;

// The tile settings of a scene tile (`Constants.TILE_FLAG_*` in RuneLite).
pub const TILE_FLAG_BRIDGE: u8 = 2;
pub const TILE_FLAG_UNDER_ROOF: u8 = 4;
pub const TILE_FLAG_VIS_BELOW: u8 = 8;

// A tile's flag text: B bridge, V visible below, R under a roof; empty when none is set.
pub fn tile_flag_text(bridge_above: bool, settings: u8) -> String {
    let mut text = String::new();
    if bridge_above {
        text.push('B');
    }
    if settings & TILE_FLAG_VIS_BELOW != 0 {
        text.push('V');
    }
    if settings & TILE_FLAG_UNDER_ROOF != 0 {
        text.push('R');
    }
    text
}

// The movement flags set in a tile's collision data, in RuneLite's `MovementFlag` order.
pub fn movement_flag_names(data: u32) -> Vec<&'static str> {
    const FLAGS: [(&str, u32); 12] = [
        ("BLOCK_MOVEMENT_NORTH_WEST", flag::WALL_NORTH_WEST),
        ("BLOCK_MOVEMENT_NORTH", flag::WALL_NORTH),
        ("BLOCK_MOVEMENT_NORTH_EAST", flag::WALL_NORTH_EAST),
        ("BLOCK_MOVEMENT_EAST", flag::WALL_EAST),
        ("BLOCK_MOVEMENT_SOUTH_EAST", flag::WALL_SOUTH_EAST),
        ("BLOCK_MOVEMENT_SOUTH", flag::WALL_SOUTH),
        ("BLOCK_MOVEMENT_SOUTH_WEST", flag::WALL_SOUTH_WEST),
        ("BLOCK_MOVEMENT_WEST", flag::WALL_WEST),
        ("BLOCK_MOVEMENT_OBJECT", flag::OBJECT),
        ("BLOCK_MOVEMENT_FLOOR_DECORATION", flag::FLOOR_DECORATION),
        ("BLOCK_MOVEMENT_FLOOR", flag::FLOOR),
        ("BLOCK_MOVEMENT_FULL", flag::BLOCKED),
    ];
    FLAGS.iter().filter(|(_, bits)| data & bits != 0).map(|(name, _)| *name).collect()
}

// The RuneLite region of a world tile: its region id, and its offset inside that region.
pub fn region(x: i32, y: i32) -> (i32, i32, i32) {
    (((x >> 6) << 8) | (y >> 6), x & 0x3f, y & 0x3f)
}

// Whether there is a line of sight between two tiles as `WorldArea.hasLineOfSightTo`; unknown tiles don't block.
pub fn line_of_sight(flags: impl Fn(i32, i32) -> u32, from: (i32, i32), to: (i32, i32)) -> bool {
    let (x1, y1) = from;
    let (x2, y2) = to;
    if (x1, y1) == (x2, y2) {
        return true;
    }
    let (dx, dy) = (x2 - x1, y2 - y1);
    let (dx_abs, dy_abs) = (dx.abs(), dy.abs());
    let full = flag::OBJECT_PROJECTILE;
    let sight = |bit: u32| bit << flag::PROJECTILE_SHIFT;
    // The flags that block a step along each axis, which the tile being entered must not have.
    let x_flags = full | if dx < 0 { sight(flag::WALL_EAST) } else { sight(flag::WALL_WEST) };
    let y_flags = full | if dy < 0 { sight(flag::WALL_NORTH) } else { sight(flag::WALL_SOUTH) };
    let blocked = |x: i32, y: i32, bits: u32| flags(x, y) & bits != 0;

    if dx_abs > dy_abs {
        let mut x = x1;
        let mut y_big = (y1 << 16) + 0x8000 - i32::from(dy < 0);
        let slope = (dy << 16) / dx_abs;
        let direction = if dx < 0 { -1 } else { 1 };
        while x != x2 {
            x += direction;
            let y = y_big >> 16;
            if blocked(x, y, x_flags) {
                return false;
            }
            y_big += slope;
            let next_y = y_big >> 16;
            if next_y != y && blocked(x, next_y, y_flags) {
                return false;
            }
        }
    } else {
        let mut y = y1;
        let mut x_big = (x1 << 16) + 0x8000 - i32::from(dx < 0);
        let slope = (dx << 16) / dy_abs;
        let direction = if dy < 0 { -1 } else { 1 };
        while y != y2 {
            y += direction;
            let x = x_big >> 16;
            if blocked(x, y, y_flags) {
                return false;
            }
            x_big += slope;
            let next_x = x_big >> 16;
            if next_x != x && blocked(next_x, y, x_flags) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_is_its_own_line_of_sight() {
        assert!(line_of_sight(|_, _| flag::OBJECT_PROJECTILE, (5, 5), (5, 5)));
    }

    #[test]
    fn open_ground_has_a_line_of_sight_in_every_direction() {
        for to in [(9, 5), (1, 5), (5, 9), (5, 1), (8, 7), (2, 3), (7, 2)] {
            assert!(line_of_sight(|_, _| 0, (5, 5), to), "{to:?}");
        }
    }

    #[test]
    fn a_wall_between_two_tiles_blocks_the_sight() {
        // A wall on the west edge of tile (6, 5): walking east from (5, 5) enters that tile through it.
        let wall = |x: i32, y: i32| if (x, y) == (6, 5) { flag::WALL_WEST << flag::PROJECTILE_SHIFT } else { 0 };
        assert!(!line_of_sight(wall, (5, 5), (8, 5)));
        assert!(line_of_sight(wall, (8, 5), (5, 5)), "walking west leaves through the other edge");
        assert!(line_of_sight(wall, (5, 5), (5, 8)));
    }

    #[test]
    fn a_full_object_blocks_the_sight_through_its_tile() {
        let rock = |x: i32, y: i32| if (x, y) == (6, 5) { flag::OBJECT_PROJECTILE } else { 0 };
        assert!(!line_of_sight(rock, (5, 5), (7, 5)));
        assert!(line_of_sight(rock, (5, 5), (5, 7)));
    }

    #[test]
    fn movement_flags_are_named_in_order() {
        assert_eq!(movement_flag_names(0), Vec::<&str>::new());
        assert_eq!(movement_flag_names(flag::WALL_NORTH | flag::WALL_WEST), ["BLOCK_MOVEMENT_NORTH", "BLOCK_MOVEMENT_WEST"]);
        assert_eq!(movement_flag_names(flag::FLOOR), ["BLOCK_MOVEMENT_FLOOR", "BLOCK_MOVEMENT_FULL"]);
    }

    #[test]
    fn tile_flags_read_as_letters() {
        assert_eq!(tile_flag_text(false, 0), "");
        assert_eq!(tile_flag_text(true, TILE_FLAG_VIS_BELOW | TILE_FLAG_UNDER_ROOF), "BVR");
        assert_eq!(tile_flag_text(false, TILE_FLAG_UNDER_ROOF), "R");
    }

    #[test]
    fn regions_split_a_tile_into_its_region_and_offset() {
        // Lumbridge castle is in region 50, 50 (3222, 3218).
        assert_eq!(region(3222, 3218), (((3222 >> 6) << 8) | (3218 >> 6), 3222 & 63, 3218 & 63));
        assert_eq!(region(3222, 3218).0, (50 << 8) | 50);
    }
}
