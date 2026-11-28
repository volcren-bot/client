use std::{collections::HashSet, ffi::CString, sync::Mutex};

use decode::{
    font::Font,
    model::Model,
    projection::{self, Terrain},
    shapes::{self, Shape},
};
use ipc::{
    state::{model_target, ModelRequest},
    world_shapes::{self, Channel, Fetch, World, CHANNELS},
};
use windows::{
    core::PCSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Memory::{
            CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
            MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
        },
    },
};

use super::{
    graphics::{draw_world, Atlas, WorldVertex},
    own::{OwnMemory, LAYOUT},
    writer::Writer,
};
use decode::interface_mask::Rect;

const LABEL_SCALE: i32 = 2;
const LABEL_SHADOW: [u8; 3] = [0, 0, 0];
const SHADOW_OFFSETS: [[i32; 2]; 4] = [[1, 1], [2, 1], [1, 2], [2, 2]];
const ARROW_WIDTH: f32 = 2.0;
const ARROW_SCALE: f32 = 2.0;
const SCREEN_LIMIT: f32 = 100_000.0;
const ATLAS_WIDTH: i32 = 512;
const SOLID: [f32; 2] = [0.5, 0.5];

pub fn create(pid: u32) {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.is_some() {
        return;
    }
    let Some(name) = CString::new(world_shapes::mapping_name(pid)).ok() else {
        return;
    };
    unsafe {
        let Ok(mapping) = CreateFileMappingA(
            HANDLE::default(),
            None,
            PAGE_READWRITE,
            0,
            world_shapes::SIZE as u32,
            PCSTR(name.as_ptr().cast()),
        ) else {
            println!(
                "[Agent]: Cannot create the world shapes mapping; world drawings are off.\r\n"
            );
            return;
        };
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, world_shapes::SIZE);
        if view.Value.is_null() {
            println!("[Agent]: Cannot map the world shapes; world drawings are off.\r\n");
            let _ = CloseHandle(mapping);
            return;
        }
        *state = Some(State {
            world: view.Value.cast(),
            mapping: mapping.0 as isize,
            channels: Default::default(),
            font_sequence: None,
            glyphs: None,
            solid: Atlas {
                generation: 0,
                width: 1,
                height: 1,
                texels: vec![255],
            },
            terrain: None,
        });
    }
}

pub fn close() {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).take();
}

#[derive(Default)]
struct Shown {
    sequence: Option<u32>,
    writer: Option<Writer>,
    shapes: Vec<Shape>,
}

impl Shown {
    fn refresh(&mut self, channel: *const Channel) {
        if let Fetch::Changed {
            sequence,
            writer,
            bytes,
        } = unsafe { Channel::fetch(channel, self.sequence) }
        {
            self.sequence = Some(sequence);
            self.shapes = shapes::decode(&bytes).unwrap_or_default();
            self.writer = None;
            if !self.shapes.is_empty() {
                self.writer = Writer::open(writer);
                if self.writer.is_none() {
                    self.shapes.clear();
                }
            }
        }
        if self.writer.as_ref().is_some_and(Writer::exited) {
            self.writer = None;
            self.shapes.clear();
        }
    }
}

struct Typeface {
    font: Font,
    places: Vec<[i32; 2]>,
}

struct Glyphs {
    faces: Vec<Typeface>,
    atlas: Atlas,
}

impl Glyphs {
    fn new(fonts: Vec<Font>, generation: u64) -> Self {
        let tallest = fonts
            .iter()
            .flat_map(|f| &f.glyphs)
            .map(|g| g.height)
            .max()
            .unwrap_or(0)
            .max(1);
        let [mut x, mut y] = [2, 0];
        let mut faces: Vec<Typeface> = Vec::with_capacity(fonts.len());
        for font in fonts {
            let mut places = Vec::with_capacity(font.glyphs.len());
            for glyph in &font.glyphs {
                if x + glyph.width > ATLAS_WIDTH {
                    x = 0;
                    y += tallest + 1;
                }
                places.push([x, y]);
                x += glyph.width + 1;
            }
            faces.push(Typeface { font, places });
        }
        let height = y + tallest;
        let mut texels = vec![0u8; (ATLAS_WIDTH * height) as usize];
        texels[0] = 255;
        for face in &faces {
            for (glyph, &[gx, gy]) in face.font.glyphs.iter().zip(&face.places) {
                for row in 0..glyph.height {
                    for col in 0..glyph.width {
                        if glyph.alpha[(row * glyph.width + col) as usize] >= 128 {
                            texels[((gy + row) * ATLAS_WIDTH + gx + col) as usize] = 255;
                        }
                    }
                }
            }
        }
        Self {
            faces,
            atlas: Atlas {
                generation,
                width: ATLAS_WIDTH,
                height,
                texels,
            },
        }
    }

    fn face(&self, bold: bool) -> Option<&Typeface> {
        self.faces.get(usize::from(bold)).or(self.faces.first())
    }
}

struct State {
    world: *mut World,
    mapping: isize,
    channels: [Shown; CHANNELS],
    font_sequence: Option<u32>,
    glyphs: Option<Glyphs>,
    solid: Atlas,
    terrain: Option<Terrain>,
}

unsafe impl Send for State {}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.world.cast(),
            });
            let _ = CloseHandle(HANDLE(self.mapping as *mut _));
        }
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(use_state: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.as_mut().map(use_state)
}

pub fn refresh() -> bool {
    with_state(|state| unsafe {
        let world = state.world;
        for (i, shown) in state.channels.iter_mut().enumerate() {
            if let Some(channel) = World::channel(world, i) {
                shown.refresh(channel);
            }
        }
        let any = state.channels.iter().any(|c| !c.shapes.is_empty());
        if any {
            if let Fetch::Changed {
                sequence, bytes, ..
            } = world_shapes::Font::fetch(World::font(world), state.font_sequence)
            {
                state.font_sequence = Some(sequence);
                state.glyphs = Font::decode_all(&bytes)
                    .filter(|fonts| !fonts.is_empty())
                    .map(|fonts| Glyphs::new(fonts, u64::from(sequence) + 1));
            }
        }
        any
    })
    .unwrap_or(false)
}

pub fn draw(viewport: [i32; 4], mask: &[Rect]) {
    let Some(layout) = LAYOUT.get() else { return };
    with_state(|state| {
        let points: Vec<shapes::Point> = state
            .channels
            .iter()
            .flat_map(|c| &c.shapes)
            .flat_map(Shape::points)
            .copied()
            .collect();
        if state.channels.iter().all(|c| c.shapes.is_empty()) {
            return;
        }
        let Ok(screen) =
            projection::project_world(&OwnMemory, layout, &mut state.terrain, &points, || {
                super::scene::locs(decode::locs::request::PLAYER_PLANE)
                    .ok()
                    .map(|locs| projection::decorations(&locs))
            })
        else {
            return;
        };
        let base = projection::scene_base(&OwnMemory, layout);
        let mut mesh = Mesh::default();
        let mut next = screen.iter().copied();
        for shape in state.channels.iter().flat_map(|c| &c.shapes) {
            let model = match (shape, base) {
                (Shape::Loc { kind, tile, id, .. }, Some([x, y])) => {
                    loc_model(*kind, [tile[0] - x, tile[1] - y], *id)
                }
                _ => None,
            };
            mesh.shape(shape, &mut next, state.glyphs.as_ref(), model.as_ref());
        }
        let atlas = state.glyphs.as_ref().map_or(&state.solid, |g| &g.atlas);
        draw_world(viewport, &mesh.vertices, atlas, mask);
    });
}

fn loc_model(kind: u8, [x, y]: [i32; 2], id: i32) -> Option<Model> {
    let request = ModelRequest {
        target: model_target::LOC,
        loc_kind: u32::from(kind),
        loc_x: x,
        loc_y: y,
        loc_id: id,
        ..ModelRequest::default()
    };
    super::model::served(decode::model::args(&request)?).flatten()
}

fn shown([x, y]: [f32; 2]) -> Option<[f32; 2]> {
    (x.is_finite() && y.is_finite() && x.abs() < SCREEN_LIMIT && y.abs() < SCREEN_LIMIT)
        .then_some([x, y])
}

#[derive(Default)]
struct Mesh {
    vertices: Vec<WorldVertex>,
}

impl Mesh {
    fn shape(
        &mut self,
        shape: &Shape,
        screen: &mut impl Iterator<Item = [f32; 2]>,
        glyphs: Option<&Glyphs>,
        model: Option<&Model>,
    ) {
        match shape {
            Shape::Path {
                points,
                color,
                width,
                closed,
            } => {
                let ends: Vec<Option<[f32; 2]>> = screen.by_ref().take(points.len()).map(shown).collect();
                let edges = match (*closed, ends.len()) {
                    (true, n) if n > 2 => n,
                    (_, n) => n.saturating_sub(1),
                };
                for i in 0..edges {
                    if let (Some(a), Some(b)) = (ends[i], ends[(i + 1) % ends.len()]) {
                        self.segment(a, b, f32::from((*width).max(1)), *color);
                    }
                }
            }
            Shape::Arrow {
                color, offset_y, ..
            } => {
                let (from, to) = (screen.next().and_then(shown), screen.next().and_then(shown));
                if let (Some(from), Some(to)) = (from, to) {
                    let drop = f32::from(*offset_y);
                    self.arrow([from[0], from[1] + drop], [to[0], to[1] + drop], *color);
                }
            }
            Shape::Label {
                text, color, bold, ..
            } => {
                let at = screen.next().and_then(shown);
                if let (Some(at), Some(face)) = (at, glyphs.and_then(|g| g.face(*bold))) {
                    self.label(face, text, at, *color);
                }
            }
            Shape::Loc { faces, hull, .. } => {
                if let Some(model) = model {
                    self.model(model, *faces, *hull);
                }
            }
        }
    }

    fn triangle(&mut self, corners: [[f32; 2]; 3], texels: [[f32; 2]; 3], color: [u8; 4]) {
        for (position, texel) in corners.into_iter().zip(texels) {
            self.vertices.push(WorldVertex {
                position,
                texel,
                color,
            });
        }
    }

    fn quad(&mut self, corners: [[f32; 2]; 4], texels: [[f32; 2]; 4], color: [u8; 4]) {
        let [a, b, c, d] = corners;
        let [ta, tb, tc, td] = texels;
        self.triangle([a, b, c], [ta, tb, tc], color);
        self.triangle([a, c, d], [ta, tc, td], color);
    }

    fn segment(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: [u8; 4]) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy);
        let (ux, uy) = if length < 1e-3 {
            (1.0, 0.0)
        } else {
            (dx / length, dy / length)
        };
        let half = width / 2.0;
        let (ex, ey) = (ux * half, uy * half);
        let (nx, ny) = (-uy * half, ux * half);
        let (start, end) = ([a[0] - ex, a[1] - ey], [b[0] + ex, b[1] + ey]);
        self.quad(
            [
                [start[0] + nx, start[1] + ny],
                [end[0] + nx, end[1] + ny],
                [end[0] - nx, end[1] - ny],
                [start[0] - nx, start[1] - ny],
            ],
            [SOLID; 4],
            color,
        );
    }

    fn arrow(&mut self, from: [f32; 2], to: [f32; 2], color: [u8; 4]) {
        self.segment(from, to, ARROW_WIDTH, color);
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length = dx.hypot(dy);
        if length < 1.0 {
            return;
        }
        let (ux, uy) = (dx / length * ARROW_SCALE, dy / length * ARROW_SCALE);
        let base = [to[0] - 5.0 * ux, to[1] - 5.0 * uy];
        self.triangle(
            [
                to,
                [base[0] - 3.0 * uy, base[1] + 3.0 * ux],
                [base[0] + 3.0 * uy, base[1] - 3.0 * ux],
            ],
            [SOLID; 3],
            color,
        );
    }

    fn model(&mut self, model: &Model, faces: [u8; 4], hull: [u8; 4]) {
        if faces[3] > 0 {
            let pixel = |[x, y]: [f32; 2]| [x.round() as i32, y.round() as i32];
            let mut drawn = HashSet::new();
            for face in &model.faces {
                for i in 0..3 {
                    let (a, b) = (face[i], face[(i + 1) % 3]);
                    let edge = (pixel(a).min(pixel(b)), pixel(a).max(pixel(b)));
                    if drawn.insert(edge) {
                        self.segment(a, b, 1.0, faces);
                    }
                }
            }
        }
        if hull[3] > 0 {
            for (i, &a) in model.hull.iter().enumerate() {
                let b = model.hull[(i + 1) % model.hull.len()];
                self.segment(a, b, 1.0, hull);
            }
        }
    }

    fn label(&mut self, face: &Typeface, text: &str, [x, y]: [f32; 2], color: [u8; 4]) {
        let font = &face.font;
        let line_height = font.max_height * LABEL_SCALE;
        let lines: Vec<&str> = text.split('\n').collect();
        let bottom = y.round() as i32;
        let placed: Vec<(&str, [i32; 2])> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let left = x.round() as i32 - font.width(line, LABEL_SCALE) / 2;
                let top = bottom - (lines.len() - i) as i32 * line_height;
                (*line, [left, top])
            })
            .collect();
        let [r, g, b] = LABEL_SHADOW;
        for &(line, [left, top]) in &placed {
            for [dx, dy] in SHADOW_OFFSETS {
                self.text(face, line, [left + dx, top + dy], [r, g, b, color[3]]);
            }
        }
        for &(line, at) in &placed {
            self.text(face, line, at, color);
        }
    }

    fn text(&mut self, face: &Typeface, text: &str, [left, top]: [i32; 2], color: [u8; 4]) {
        let font = &face.font;
        let mut pen = left;
        for c in text.bytes() {
            let glyph = &font.glyphs[c as usize];
            if (32..127).contains(&c) && glyph.width > 0 && glyph.height > 0 {
                let [gx, gy] = face.places[c as usize];
                let x0 = (pen + glyph.offset_x * LABEL_SCALE) as f32;
                let y0 = (top + glyph.offset_y * LABEL_SCALE) as f32;
                let (x1, y1) = (
                    x0 + (glyph.width * LABEL_SCALE) as f32,
                    y0 + (glyph.height * LABEL_SCALE) as f32,
                );
                let (u0, v0) = (gx as f32, gy as f32);
                let (u1, v1) = (u0 + glyph.width as f32, v0 + glyph.height as f32);
                self.quad(
                    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
                    [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                    color,
                );
            }
            pen += i32::from(font.advances[c as usize]) * LABEL_SCALE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use decode::font::Glyph;

    fn font() -> Font {
        let mut advances = [0u8; 256];
        advances[b'A' as usize] = 4;
        let glyphs = (0..256)
            .map(|c| Glyph {
                offset_x: 0,
                offset_y: 1,
                width: if c == b'A' as i32 { 3 } else { 0 },
                height: if c == b'A' as i32 { 2 } else { 0 },
                alpha: if c == b'A' as i32 {
                    vec![255; 6]
                } else {
                    Vec::new()
                },
            })
            .collect();
        Font {
            advances,
            max_height: 5,
            glyphs,
        }
    }

    #[test]
    fn a_closed_path_draws_every_edge_it_can_see() {
        let mut mesh = Mesh::default();
        let shape = Shape::Path {
            points: vec![shapes::Point::from_local(projection::Point::new(0, 0, 0), [0, 0]); 4],
            color: [255; 4],
            width: 2,
            closed: true,
        };
        let mut screen = [[0.0, 0.0], [10.0, 0.0], [f32::NAN, 0.0], [0.0, 10.0]].into_iter();
        mesh.shape(&shape, &mut screen, None, None);
        assert_eq!(mesh.vertices.len(), 2 * 6);
        assert!(screen.next().is_none());
    }

    #[test]
    fn labels_sit_centred_above_their_point_with_a_shadow() {
        let mut bold = font();
        bold.max_height = 6;
        let glyphs = Glyphs::new(vec![font(), bold], 1);
        assert_eq!(glyphs.atlas.texels[0], 255);
        let plain = glyphs.face(false).unwrap();
        let [gx, gy] = plain.places[b'A' as usize];
        assert_eq!(glyphs.atlas.texels[(gy * ATLAS_WIDTH + gx) as usize], 255);
        let heavy = glyphs.face(true).unwrap();
        assert_eq!(heavy.font.max_height, 6);
        assert_ne!(heavy.places[b'A' as usize], plain.places[b'A' as usize]);
        let mut mesh = Mesh::default();
        mesh.label(plain, "A", [100.0, 50.0], [1, 2, 3, 200]);
        assert_eq!(mesh.vertices.len(), 5 * 6);
        let text = &mesh.vertices[4 * 6];
        assert_eq!(text.position, [96.0, 42.0]);
        assert_eq!(text.color, [1, 2, 3, 200]);
        assert_eq!(mesh.vertices[0].position, [97.0, 43.0]);
        assert_eq!(mesh.vertices[3 * 6].position, [98.0, 44.0]);
        assert_eq!(mesh.vertices[0].color, [0, 0, 0, 200]);
        let mut mesh = Mesh::default();
        mesh.label(plain, "A\nAA", [100.0, 50.0], [1, 2, 3, 200]);
        assert_eq!(mesh.vertices.len(), 3 * 5 * 6);
        let first = &mesh.vertices[12 * 6];
        let second = &mesh.vertices[13 * 6];
        assert_eq!(first.position, [96.0, 32.0]);
        assert_eq!(second.position, [92.0, 42.0]);
        assert_eq!(Glyphs::new(vec![font()], 2).face(true).unwrap().font.max_height, 5);
    }

    #[test]
    fn a_model_draws_each_shared_edge_once_and_its_hull() {
        let model = Model {
            bounding_box: false,
            orientation: 0,
            vertex_count: 4,
            corners: Vec::new(),
            hull: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            face_count: 2,
            faces: vec![
                [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
                [[0.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            ],
        };
        let mut mesh = Mesh::default();
        mesh.model(&model, [255, 255, 0, 70], [0; 4]);
        assert_eq!(mesh.vertices.len(), 5 * 6);
        let mut mesh = Mesh::default();
        mesh.model(&model, [0; 4], [255; 4]);
        assert_eq!(mesh.vertices.len(), 4 * 6);
    }
}
