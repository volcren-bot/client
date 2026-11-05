//! Pictures from the client, fetched from a loaded client once and kept on disk: item icons as the inventory draws them, and sprites of the cache (prayer and spell icons).

use std::{
    collections::{BTreeSet, HashMap},
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use eframe::egui;
use injector::{Image, ItemIconRequest};

/// The icons file in the data folder: a header, then one record per picture, only ever appended to.
const FILE_NAME: &str = "icons.bin";
const MAGIC: &[u8; 4] = b"VICN";
/// Version 1 held item icons only; its files are ignored and the icons drawn again.
const VERSION: u8 = 2;
const RECORD_ITEM: u8 = 0;
const RECORD_SPRITE: u8 = 1;
/// Pictures asked of the client in one request.
pub const BATCH: usize = 64;
/// How long after a failed read the next one waits, so a client that can't answer isn't asked every frame.
pub const RETRY_AFTER: Duration = Duration::from_secs(5);

/// What a picture is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// An item at a quantity, the number drawn on it or not.
    Item(ItemIconRequest),
    /// A sprite of the cache, by id.
    Sprite(i32),
}

impl Key {
    pub fn item(id: i32, quantity: i32, show_count: bool) -> Self {
        Key::Item(ItemIconRequest {
            id,
            quantity,
            show_count,
        })
    }
}

/// What's known about a picture: the client drew it, or it couldn't.
enum Entry {
    Texture(egui::TextureHandle),
    Missing,
}

/// The pictures a client is asked for in one go.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Batch {
    pub items: Vec<ItemIconRequest>,
    pub sprites: Vec<i32>,
}

#[derive(Default)]
pub struct Icons {
    known: HashMap<Key, Entry>,
    /// Pictures the UI asked for that no client has drawn yet.
    wanted: BTreeSet<Key>,
    /// Pictures a client is drawing now.
    in_flight: Vec<Key>,
    /// When to ask again after a read that failed.
    retry_at: Option<Instant>,
    loaded_file: bool,
    pub error: Option<String>,
}

fn path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "volcren", "Volcren")
        .map(|dirs| dirs.data_dir().join(FILE_NAME))
}

fn texture(
    ctx: &egui::Context,
    key: Key,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> egui::TextureHandle {
    let name = match key {
        Key::Item(item) => format!(
            "item-{}-{}-{}",
            item.id,
            item.quantity,
            u8::from(item.show_count)
        ),
        Key::Sprite(id) => format!("sprite-{id}"),
    };
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba),
        egui::TextureOptions::NEAREST,
    )
}

/// The records of the icons file, each `(key, width, height, rgba)`; a file that doesn't parse is ignored.
fn parse(bytes: &[u8]) -> Vec<(Key, u32, u32, Vec<u8>)> {
    let mut records = Vec::new();
    if bytes.len() < 5 || &bytes[..4] != MAGIC || bytes[4] != VERSION {
        return records;
    }
    let i32_at = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let mut at = 5;
    while let Some(&kind) = bytes.get(at) {
        let (key, header) = match kind {
            RECORD_ITEM if at + 10 <= bytes.len() => (
                Key::item(i32_at(at + 1), i32_at(at + 5), bytes[at + 9] == 1),
                10,
            ),
            RECORD_SPRITE if at + 5 <= bytes.len() => (Key::Sprite(i32_at(at + 1)), 5),
            _ => break,
        };
        if at + header + 4 > bytes.len() {
            break;
        }
        let (width, height) = (
            u32::from(u16_at(at + header)),
            u32::from(u16_at(at + header + 2)),
        );
        let len = (width * height * 4) as usize;
        let start = at + header + 4;
        let Some(rgba) = bytes.get(start..start + len) else {
            break;
        };
        records.push((key, width, height, rgba.to_vec()));
        at = start + len;
    }
    records
}

fn record(key: Key, width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(14 + rgba.len());
    match key {
        Key::Item(item) => {
            out.push(RECORD_ITEM);
            out.extend_from_slice(&item.id.to_le_bytes());
            out.extend_from_slice(&item.quantity.to_le_bytes());
            out.push(u8::from(item.show_count));
        }
        Key::Sprite(id) => {
            out.push(RECORD_SPRITE);
            out.extend_from_slice(&id.to_le_bytes());
        }
    }
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    out.extend_from_slice(rgba);
    out
}

impl Icons {
    /// Reads the pictures kept on disk, once.
    fn load_file(&mut self, ctx: &egui::Context) {
        if self.loaded_file {
            return;
        }
        self.loaded_file = true;
        let Some(path) = path() else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        for (key, width, height, rgba) in parse(&bytes) {
            self.known
                .insert(key, Entry::Texture(texture(ctx, key, width, height, &rgba)));
        }
    }

    fn append_file(&mut self, records: &[Vec<u8>]) {
        let Some(path) = path() else {
            return;
        };
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // a file of an older version is started over
            let header = std::fs::File::open(&path).ok().and_then(|mut file| {
                let mut header = [0u8; 5];
                file.read_exact(&mut header).ok().map(|()| header)
            });
            let fresh = header.is_none_or(|header| &header[..4] != MAGIC || header[4] != VERSION);
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(!fresh)
                .write(true)
                .truncate(fresh)
                .open(&path)?;
            if fresh {
                file.write_all(MAGIC)?;
                file.write_all(&[VERSION])?;
            }
            for record in records {
                file.write_all(record)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(format!("Cannot save icons to {}: {error}", path.display()));
        }
    }

    /// The picture's texture, or none while it hasn't been drawn (it is asked of the next client read) or when the client couldn't draw it.
    pub fn texture(&mut self, ctx: &egui::Context, key: Key) -> Option<&egui::TextureHandle> {
        self.load_file(ctx);
        if !self.known.contains_key(&key) && !self.in_flight.contains(&key) {
            self.wanted.insert(key);
        }
        match self.known.get(&key) {
            Some(Entry::Texture(texture)) => Some(texture),
            _ => None,
        }
    }

    /// Whether some pictures are waiting for a client to draw them, none are being drawn, and a failed read's wait is over.
    pub fn due(&self, now: Instant) -> bool {
        !self.wanted.is_empty()
            && self.in_flight.is_empty()
            && self.retry_at.is_none_or(|at| now >= at)
    }

    /// How long until a failed read is tried again, when pictures wait for that.
    pub fn retry_in(&self, now: Instant) -> Option<Duration> {
        let at = self.retry_at.filter(|_| !self.wanted.is_empty())?;
        Some(at.saturating_duration_since(now))
    }

    /// Takes the next batch of pictures to ask a client for. The rest are forgotten: the UI asks again for those it still shows.
    pub fn start(&mut self) -> Batch {
        let keys: Vec<Key> = self.wanted.iter().take(BATCH).copied().collect();
        self.wanted.clear();
        let mut batch = Batch::default();
        for key in &keys {
            match key {
                Key::Item(item) => batch.items.push(*item),
                Key::Sprite(id) => batch.sprites.push(*id),
            }
        }
        self.in_flight = keys;
        batch
    }

    /// Keeps what the client drew. A picture it couldn't draw stays missing until the loader restarts. With `error`, the read failed part way: what came is kept, and the rest is asked for again after `RETRY_AFTER` if the UI still shows it.
    pub fn finish(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        pictures: Vec<(Key, Option<Image>)>,
        error: Option<String>,
    ) {
        let keys = std::mem::take(&mut self.in_flight);
        let mut records = Vec::new();
        for (key, image) in pictures {
            let entry = match image
                .as_ref()
                .and_then(|image| Some((image, image.pixels()?)))
            {
                Some((image, rgba)) => {
                    records.push(record(key, image.width, image.height, &rgba));
                    Entry::Texture(texture(ctx, key, image.width, image.height, &rgba))
                }
                None => Entry::Missing,
            };
            self.known.insert(key, entry);
        }
        if !records.is_empty() {
            self.append_file(&records);
        }
        self.retry_at = error.is_some().then(|| now + RETRY_AFTER);
        if error.is_none() {
            for key in keys {
                self.known.entry(key).or_insert(Entry::Missing);
            }
        }
        self.error = error;
    }
}

/// The size an item icon is drawn at: the client's own pixels, one point each.
pub const SLOT_SIZE: egui::Vec2 = egui::vec2(36.0, 32.0);

/// Draws an item's icon, or its name while there is none, in a slot the size of the client's.
pub fn slot(
    ui: &mut egui::Ui,
    texture: Option<&egui::TextureHandle>,
    fallback: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(SLOT_SIZE, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        match texture {
            Some(texture) => {
                egui::Image::from_texture(texture).paint_at(ui, rect);
            }
            None => {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    ui.visuals().widgets.noninteractive.bg_stroke,
                    egui::StrokeKind::Inside,
                );
                let text = fallback.chars().take(4).collect::<String>();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::TextStyle::Small.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
            }
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_a_short_file_is_cut() {
        let item = Key::item(995, 1000, true);
        let sprite = Key::Sprite(115);
        let rgba = vec![7u8; 2 * 2 * 4];
        let mut file = MAGIC.to_vec();
        file.push(VERSION);
        file.extend(record(item, 2, 2, &rgba));
        file.extend(record(sprite, 2, 2, &rgba));
        file.extend(record(Key::item(1, 1, false), 2, 2, &rgba));
        let records = parse(&file);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0], (item, 2, 2, rgba.clone()));
        assert_eq!(records[1].0, sprite);
        assert!(matches!(records[2].0, Key::Item(i) if !i.show_count));
        // a record cut short (a write that didn't finish) is dropped, the ones before it kept
        assert_eq!(parse(&file[..file.len() - 3]).len(), 2);
        assert!(parse(b"nope").is_empty());
        let mut old = MAGIC.to_vec();
        old.push(1);
        assert!(parse(&old).is_empty());
    }

    #[test]
    fn a_batch_splits_items_from_sprites() {
        let mut icons = Icons::default();
        icons.wanted.insert(Key::item(1, 1, false));
        icons.wanted.insert(Key::Sprite(5));
        let now = Instant::now();
        assert!(icons.due(now));
        let batch = icons.start();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.sprites, [5]);
        assert!(!icons.due(now));
    }

    #[test]
    fn a_failed_read_waits_before_the_next_and_keeps_what_came() {
        let ctx = egui::Context::default();
        let mut icons = Icons {
            loaded_file: true,
            ..Default::default()
        };
        let (item, sprite) = (Key::item(1, 1, false), Key::Sprite(5));
        icons.wanted.insert(item);
        icons.wanted.insert(sprite);
        let now = Instant::now();
        icons.start();
        // the items came, the sprites failed
        icons.finish(&ctx, now, vec![(item, None)], Some("no answer".into()));
        assert!(icons.known.contains_key(&item));
        assert!(!icons.known.contains_key(&sprite), "it is asked for again");

        // the UI still shows the sprite: it waits for the retry
        icons.texture(&ctx, sprite);
        assert!(!icons.due(now));
        assert_eq!(icons.retry_in(now), Some(RETRY_AFTER));
        assert!(icons.due(now + RETRY_AFTER));

        icons.start();
        icons.finish(&ctx, now + RETRY_AFTER, vec![(sprite, None)], None);
        assert_eq!(icons.retry_in(now + RETRY_AFTER), None);
        assert!(icons.error.is_none());
    }
}
