use std::sync::Mutex;

use decode::shapes::Shape;

pub const MAX_SHAPES: usize = 20_000;
pub const MAX_POINTS: usize = 100_000;
pub const MAX_TEXT: usize = 200;
pub const MAX_WIDTH: u8 = 16;
pub const MAX_LOCS: usize = 16;

static KEPT: Mutex<Option<(u32, Vec<Shape>)>> = Mutex::new(None);

pub fn validate(shapes: &[Shape]) -> Result<(), String> {
    if shapes.len() > MAX_SHAPES {
        return Err(format!(
            "at most {MAX_SHAPES} debug shapes, got {}",
            shapes.len()
        ));
    }
    let points: usize = shapes.iter().map(|s| s.points().count()).sum();
    if points > MAX_POINTS {
        return Err(format!(
            "at most {MAX_POINTS} debug shape points, got {points}"
        ));
    }
    let locs = shapes
        .iter()
        .filter(|s| matches!(s, Shape::Loc { .. }))
        .count();
    if locs > MAX_LOCS {
        return Err(format!("at most {MAX_LOCS} debug loc models, got {locs}"));
    }
    shapes.iter().try_for_each(|shape| match shape {
        Shape::Path { width, .. } if !(1..=MAX_WIDTH).contains(width) => {
            Err(format!("a path's width is 1 to {MAX_WIDTH}, got {width}"))
        }
        Shape::Label { text, .. } | Shape::ScreenLabel { text, .. } if text.len() > MAX_TEXT => {
            Err(format!("a label is at most {MAX_TEXT} bytes"))
        }
        _ => Ok(()),
    })
}

pub fn set(pid: u32, shapes: Vec<Shape>) -> Result<(), String> {
    validate(&shapes)?;
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    *kept = Some((pid, shapes));
    show(&kept)
}

pub fn debugging_changed() -> Result<(), String> {
    show(&KEPT.lock().unwrap_or_else(|e| e.into_inner()))
}

fn show(kept: &Option<(u32, Vec<Shape>)>) -> Result<(), String> {
    let Some((pid, shapes)) = kept else {
        return Ok(());
    };
    let shown: &[Shape] = if crate::form::script_debugging() {
        shapes
    } else {
        &[]
    };
    publish(*pid, shown)
}

#[cfg(windows)]
fn publish(pid: u32, shapes: &[Shape]) -> Result<(), String> {
    crate::win::publish_world(pid, ipc::world_shapes::SCRIPT, shapes)
}

#[cfg(not(windows))]
fn publish(_: u32, _: &[Shape]) -> Result<(), String> {
    Err("clients can only be reached from Windows or Wine".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use decode::shapes::Point;

    fn point() -> Point {
        Point {
            x: 0,
            y: 0,
            z: 0,
            tile: None,
            heightmap: false,
        }
    }

    #[test]
    fn widths_texts_and_counts_are_limited() {
        let path = |width| Shape::Path {
            points: vec![point(); 2],
            color: [0; 4],
            width,
            closed: false,
        };
        assert!(validate(&[path(1), path(MAX_WIDTH)]).is_ok());
        assert!(validate(&[path(0)]).is_err());
        assert!(validate(&[path(MAX_WIDTH + 1)]).is_err());
        let label = |len| Shape::Label {
            at: point(),
            text: "a".repeat(len),
            color: [0; 4],
            bold: false,
        };
        assert!(validate(&[label(MAX_TEXT)]).is_ok());
        assert!(validate(&[label(MAX_TEXT + 1)]).is_err());
        let screen = |len| Shape::ScreenLabel {
            at: [10, 20],
            text: "a".repeat(len),
            color: [0; 4],
            bold: true,
        };
        assert!(validate(&[screen(MAX_TEXT)]).is_ok());
        assert!(validate(&[screen(MAX_TEXT + 1)]).is_err());
        assert!(validate(&vec![label(1); MAX_SHAPES + 1]).is_err());
        let long = Shape::Path {
            points: vec![point(); MAX_POINTS + 1],
            color: [0; 4],
            width: 1,
            closed: false,
        };
        assert!(validate(&[long]).is_err());
        let loc = Shape::Loc {
            kind: 10,
            tile: [0, 0],
            id: 1,
            faces: [0; 4],
            hull: [0; 4],
        };
        assert!(validate(&vec![loc.clone(); MAX_LOCS]).is_ok());
        assert!(validate(&vec![loc; MAX_LOCS + 1]).is_err());
    }
}
