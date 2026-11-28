use serde::{Deserialize, Serialize};

use crate::projection;

pub const TILE: i32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub tile: Option<[i32; 2]>,
    pub heightmap: bool,
}

impl Point {
    pub fn from_local(point: projection::Point, [base_x, base_y]: [i32; 2]) -> Self {
        Self {
            x: point.x.wrapping_add(base_x.wrapping_mul(TILE)),
            y: point.y.wrapping_add(base_y.wrapping_mul(TILE)),
            z: point.z,
            tile: point
                .tile
                .map(|[x, y]| [x.wrapping_add(base_x), y.wrapping_add(base_y)]),
            heightmap: point.heightmap,
        }
    }

    pub fn local(&self, [base_x, base_y]: [i32; 2]) -> projection::Point {
        projection::Point {
            x: self.x.wrapping_sub(base_x.wrapping_mul(TILE)),
            y: self.y.wrapping_sub(base_y.wrapping_mul(TILE)),
            z: self.z,
            tile: self
                .tile
                .map(|[x, y]| [x.wrapping_sub(base_x), y.wrapping_sub(base_y)]),
            heightmap: self.heightmap,
        }
    }
}

pub type Color = [u8; 4];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Path {
        points: Vec<Point>,
        color: Color,
        width: u8,
        closed: bool,
    },
    Arrow {
        from: Point,
        to: Point,
        color: Color,
        offset_y: i16,
    },
    Label {
        at: Point,
        text: String,
        color: Color,
        bold: bool,
    },
    Loc {
        kind: u8,
        tile: [i32; 2],
        id: i32,
        faces: Color,
        hull: Color,
    },
    ScreenLabel {
        at: [i32; 2],
        text: String,
        color: Color,
        bold: bool,
    },
}

impl Shape {
    pub fn points(&self) -> impl Iterator<Item = &Point> {
        let (path, ends): (&[Point], [Option<&Point>; 2]) = match self {
            Self::Path { points, .. } => (points, [None, None]),
            Self::Arrow { from, to, .. } => (&[], [Some(from), Some(to)]),
            Self::Label { at, .. } => (&[], [Some(at), None]),
            Self::Loc { .. } | Self::ScreenLabel { .. } => (&[], [None, None]),
        };
        path.iter().chain(ends.into_iter().flatten())
    }
}

pub fn encode(shapes: &[Shape]) -> Vec<u8> {
    if shapes.is_empty() {
        return Vec::new();
    }
    postcard::to_allocvec(shapes).unwrap_or_default()
}

pub fn decode(bytes: &[u8]) -> Option<Vec<Shape>> {
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    postcard::from_bytes(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: i32, y: i32) -> Point {
        Point {
            x,
            y,
            z: -40,
            tile: Some([x / TILE, y / TILE]),
            heightmap: false,
        }
    }

    #[test]
    fn shapes_survive_their_wire_form() {
        let shapes = vec![
            Shape::Path {
                points: vec![point(409_600, 409_600), point(409_728, 409_600)],
                color: [255, 0, 0, 255],
                width: 3,
                closed: true,
            },
            Shape::Arrow {
                from: point(1, 2),
                to: point(3, 4),
                color: [0, 255, 255, 255],
                offset_y: 16,
            },
            Shape::Label {
                at: point(5, 6),
                text: "Step 1".into(),
                color: [255, 255, 255, 200],
                bold: true,
            },
            Shape::Loc {
                kind: 10,
                tile: [3200, 3201],
                id: 1234,
                faces: [255, 255, 0, 70],
                hull: [255, 255, 0, 255],
            },
            Shape::ScreenLabel {
                at: [1212, 1257],
                text: "Alching".into(),
                color: [255, 0, 255, 255],
                bold: true,
            },
        ];
        assert_eq!(
            shapes.iter().map(|s| s.points().count()).collect::<Vec<_>>(),
            [2, 2, 1, 0, 0]
        );
        assert_eq!(decode(&encode(&shapes)), Some(shapes));
        assert_eq!(decode(&encode(&[])), Some(Vec::new()));
        assert_eq!(decode(&[0xff, 0xff]), None);
    }

    #[test]
    fn world_points_move_with_the_scene_base() {
        let local = projection::Point {
            tile: Some([3, 4]),
            ..projection::Point::new(3 * TILE + 64, 4 * TILE, -10)
        };
        let world = Point::from_local(local, [3200, 3136]);
        assert_eq!(
            (world.x, world.y, world.tile),
            (3203 * TILE + 64, 3140 * TILE, Some([3203, 3140]))
        );
        assert_eq!(world.local([3200, 3136]), local);
        let moved = world.local([3192, 3136]);
        assert_eq!((moved.x, moved.tile), (11 * TILE + 64, Some([11, 4])));
    }
}
