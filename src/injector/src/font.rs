//! The client's bitmap fonts, decoded from the cache: a metrics archive and a sprite archive of glyphs, with ids fixed for `P12_FULL`.

// Only the Windows injector draws overlays, so on other hosts nothing here is called outside tests.
#![cfg_attr(not(windows), allow(dead_code))]

/// Index of the font metrics and of the sprite archive of each font.
pub const FONTS_INDEX: u32 = 13;
pub const SPRITES_INDEX: u32 = 8;
/// `p12_full`, the 12pt font the game uses for chat and most labels.
pub const P12_FULL: i32 = 495;

const METRICS_LEN: usize = 257;
const FLAG_VERTICAL: u8 = 1;
const FLAG_ALPHA: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub offset_x: i32,
    pub offset_y: i32,
    pub width: i32,
    pub height: i32,
    /// One byte per pixel, row by row. 255 is opaque, 0 is transparent.
    pub alpha: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub advances: [u8; 256],
    pub ascent: i32,
    pub max_height: i32,
    /// Indexed by character code.
    pub glyphs: Vec<Glyph>,
}

impl Font {
    pub fn decode(metrics: &[u8], sprites: &[u8]) -> Result<Self, String> {
        if metrics.len() != METRICS_LEN {
            return Err(format!("font metrics are {} bytes, expected {METRICS_LEN}", metrics.len()));
        }
        let mut advances = [0u8; 256];
        advances.copy_from_slice(&metrics[..256]);
        let frames = decode_sprites(sprites)?;
        if frames.len() < 256 {
            return Err(format!("font has {} glyphs, expected 256", frames.len()));
        }
        let max_height = frames[0].max_height;
        let glyphs = frames
            .into_iter()
            .take(256)
            .map(|frame| Glyph {
                offset_x: frame.offset_x,
                offset_y: frame.offset_y,
                width: frame.width,
                height: frame.height,
                alpha: frame.alpha,
            })
            .collect();
        Ok(Self { advances, ascent: metrics[256] as i32, max_height, glyphs })
    }

    /// Width of `text` in pixels at `scale`: the sum of the advances of its characters.
    pub fn width(&self, text: &str, scale: i32) -> i32 {
        text.bytes().map(|c| self.advances[c as usize] as i32).sum::<i32>() * scale
    }

    /// Calls `plot(x, y)` for each pixel of `text` drawn at `(x, y)` as `scale` blocks; non-printable ASCII is skipped.
    pub fn plot(&self, text: &str, x: i32, y: i32, scale: i32, mut plot: impl FnMut(i32, i32)) {
        let mut pen = x;
        for c in text.bytes() {
            if (32..127).contains(&c) {
                let glyph = &self.glyphs[c as usize];
                for row in 0..glyph.height {
                    for col in 0..glyph.width {
                        if glyph.alpha[(row * glyph.width + col) as usize] >= 128 {
                            let (px, py) = (pen + (glyph.offset_x + col) * scale, y + (glyph.offset_y + row) * scale);
                            for dy in 0..scale {
                                for dx in 0..scale {
                                    plot(px + dx, py + dy);
                                }
                            }
                        }
                    }
                }
            }
            pen += self.advances[c as usize] as i32 * scale;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    width: i32,
    height: i32,
    offset_x: i32,
    offset_y: i32,
    max_height: i32,
    alpha: Vec<u8>,
}

/// Reads the sprite archive's frames, keeping only coverage (which pixels are drawn), index 0 transparent unless the frame has alpha.
fn decode_sprites(data: &[u8]) -> Result<Vec<Frame>, String> {
    let short = || "sprite archive is truncated".to_string();
    let u8_at = |at: usize| data.get(at).copied().ok_or_else(short);
    let u16_at = |at: usize| Ok::<u16, String>(u16::from_be_bytes([u8_at(at)?, u8_at(at + 1)?]));

    let count = u16_at(data.len().checked_sub(2).ok_or_else(short)?)? as usize;
    let header = data.len().checked_sub(7 + count * 8).ok_or_else(short)?;
    let height = u16_at(header + 2)? as i32;

    let column = |base: usize| -> Vec<i32> { (0..count).map(|i| u16_at(base + i * 2).map_or(0, |v| v as i32)).collect() };
    let offsets_x = column(header + 5);
    let offsets_y = column(header + 5 + count * 2);
    let widths = column(header + 5 + count * 4);
    let heights = column(header + 5 + count * 6);

    let mut frames = Vec::with_capacity(count);
    let mut at = 0;
    for i in 0..count {
        let (frame_width, frame_height) = (widths[i], heights[i]);
        let dimension = (frame_width * frame_height).max(0) as usize;
        let flags = u8_at(at)?;
        at += 1;
        let vertical = flags & FLAG_VERTICAL != 0;
        let indices = read_plane(data, &mut at, frame_width, frame_height, vertical, dimension)?;
        let alphas = if flags & FLAG_ALPHA != 0 {
            Some(read_plane(data, &mut at, frame_width, frame_height, vertical, dimension)?)
        } else {
            None
        };
        let alpha = indices
            .iter()
            .enumerate()
            .map(|(j, &index)| match (index, &alphas) {
                (0, Some(alphas)) => alphas[j],
                (0, None) => 0,
                _ => 255,
            })
            .collect();
        frames.push(Frame {
            width: frame_width,
            height: frame_height,
            offset_x: offsets_x[i],
            offset_y: offsets_y[i],
            max_height: height,
            alpha,
        });
    }
    Ok(frames)
}

/// One plane of a frame: `dimension` bytes, row by row or column by column.
fn read_plane(data: &[u8], at: &mut usize, width: i32, height: i32, vertical: bool, dimension: usize) -> Result<Vec<u8>, String> {
    let short = || "sprite archive is truncated".to_string();
    let bytes = data.get(*at..*at + dimension).ok_or_else(short)?;
    let mut plane = vec![0u8; dimension];
    if vertical {
        let (w, h) = (width.max(0) as usize, height.max(0) as usize);
        for j in 0..w {
            for k in 0..h {
                plane[w * k + j] = bytes[j * h + k];
            }
        }
    } else {
        plane.copy_from_slice(bytes);
    }
    *at += dimension;
    Ok(plane)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(advance: u8) -> Vec<u8> {
        let mut m = vec![advance; 256];
        m.push(9);
        m
    }

    #[test]
    fn rejects_metrics_of_the_wrong_size() {
        assert!(Font::decode(&[0; 256], &[]).is_err());
    }

    #[test]
    fn rejects_a_truncated_sprite_archive() {
        assert!(Font::decode(&metrics(5), &[0, 1]).is_err());
        assert!(Font::decode(&metrics(5), &[]).is_err());
    }

    #[test]
    fn measures_text_by_advances() {
        // Only the metrics are needed to measure, so this font is a hand-built single-glyph stub.
        let font = Font { advances: [4; 256], ascent: 9, max_height: 12, glyphs: Vec::new() };
        assert_eq!(font.width("abc", 1), 12);
        assert_eq!(font.width("abc", 2), 24);
        assert_eq!(font.width("", 1), 0);
    }

    // Decodes the real p12_full from FONT_DATA_DIR (holding p12_metrics.bin and p12_sprite.bin); run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn decodes_p12_full() {
        let dir = std::env::var("FONT_DATA_DIR").expect("set FONT_DATA_DIR");
        let metrics = std::fs::read(format!("{dir}/p12_metrics.bin")).unwrap();
        let sprites = std::fs::read(format!("{dir}/p12_sprite.bin")).unwrap();
        let font = Font::decode(&metrics, &sprites).unwrap();
        assert_eq!(font.glyphs.len(), 256);
        assert_eq!(font.advances[b'A' as usize] as i32, font.width("A", 1));
        println!("ascent {} max height {} width of 'Taiman' {}", font.ascent, font.max_height, font.width("Taiman", 1));
    }
}
