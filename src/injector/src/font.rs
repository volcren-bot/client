pub const FONTS_INDEX: u32 = 13;
pub const SPRITES_INDEX: u32 = 8;
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
    pub alpha: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub advances: [u8; 256],
    pub max_height: i32,
    pub glyphs: Vec<Glyph>,
}

impl Font {
    pub fn decode(metrics: &[u8], sprites: &[u8]) -> Result<Self, String> {
        if metrics.len() != METRICS_LEN {
            return Err(format!(
                "font metrics are {} bytes, expected {METRICS_LEN}",
                metrics.len()
            ));
        }
        let advances = metrics[..256].try_into().expect("checked length");
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
        Ok(Self {
            advances,
            max_height,
            glyphs,
        })
    }

    pub fn width(&self, text: &str, scale: i32) -> i32 {
        text.bytes()
            .map(|c| self.advances[c as usize] as i32)
            .sum::<i32>()
            * scale
    }

    pub fn plot(&self, text: &str, x: i32, y: i32, scale: i32, mut plot: impl FnMut(i32, i32)) {
        let mut pen = x;
        for c in text.bytes() {
            if (32..127).contains(&c) {
                let glyph = &self.glyphs[c as usize];
                for row in 0..glyph.height {
                    for col in 0..glyph.width {
                        if glyph.alpha[(row * glyph.width + col) as usize] >= 128 {
                            let (px, py) = (
                                pen + (glyph.offset_x + col) * scale,
                                y + (glyph.offset_y + row) * scale,
                            );
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
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub max_height: i32,
    pub alpha: Vec<u8>,
    pub rgba: Vec<u8>,
}

pub fn decode_sprites(data: &[u8]) -> Result<Vec<Frame>, String> {
    let short = || "sprite archive is truncated".to_string();
    let u8_at = |at: usize| data.get(at).copied().ok_or_else(short);
    let u16_at = |at: usize| Ok::<u16, String>(u16::from_be_bytes([u8_at(at)?, u8_at(at + 1)?]));

    let count = u16_at(data.len().checked_sub(2).ok_or_else(short)?)? as usize;
    let header = data.len().checked_sub(7 + count * 8).ok_or_else(short)?;
    let max_height = u16_at(header + 2)? as i32;

    let column = |base: usize| -> Vec<i32> {
        (0..count)
            .map(|i| u16_at(base + i * 2).map_or(0, |v| v as i32))
            .collect()
    };
    let offsets_x = column(header + 5);
    let offsets_y = column(header + 5 + count * 2);
    let widths = column(header + 5 + count * 4);
    let heights = column(header + 5 + count * 6);
    let palette_len = u8_at(header + 4)? as usize + 1;
    let palette_at = header
        .checked_sub((palette_len - 1) * 3)
        .ok_or_else(short)?;
    let mut palette = vec![[0u8; 3]; palette_len];
    for (i, colour) in palette.iter_mut().enumerate().skip(1) {
        let at = palette_at + (i - 1) * 3;
        *colour = [u8_at(at)?, u8_at(at + 1)?, u8_at(at + 2)?];
    }

    let mut frames = Vec::with_capacity(count);
    let mut at = 0;
    for i in 0..count {
        let (width, height) = (widths[i] as usize, heights[i] as usize);
        let flags = u8_at(at)?;
        at += 1;
        let vertical = flags & FLAG_VERTICAL != 0;
        let indices = read_plane(data, &mut at, width, height, vertical)?;
        let alpha = if flags & FLAG_ALPHA != 0 {
            read_plane(data, &mut at, width, height, vertical)?
        } else {
            indices
                .iter()
                .map(|&index| if index == 0 { 0 } else { 255 })
                .collect()
        };
        let mut rgba = Vec::with_capacity(indices.len() * 4);
        for (&index, &a) in indices.iter().zip(&alpha) {
            let [r, g, b] = palette.get(index as usize).copied().unwrap_or_default();
            rgba.extend_from_slice(&[r, g, b, a]);
        }
        frames.push(Frame {
            width: widths[i],
            height: heights[i],
            offset_x: offsets_x[i],
            offset_y: offsets_y[i],
            max_height,
            alpha,
            rgba,
        });
    }
    Ok(frames)
}

fn read_plane(
    data: &[u8],
    at: &mut usize,
    width: usize,
    height: usize,
    vertical: bool,
) -> Result<Vec<u8>, String> {
    let dimension = width * height;
    let bytes = at
        .checked_add(dimension)
        .and_then(|end| data.get(*at..end))
        .ok_or("sprite archive is truncated")?;
    let plane = if vertical {
        let mut plane = vec![0u8; dimension];
        for j in 0..width {
            for k in 0..height {
                plane[width * k + j] = bytes[j * height + k];
            }
        }
        plane
    } else {
        bytes.to_vec()
    };
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
    fn a_frame_bigger_than_the_archive_is_an_error() {
        for flags in [0, FLAG_VERTICAL] {
            let mut data = vec![flags];
            data.extend_from_slice(&[0, 0, 0, 12, 0]);
            data.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
            data.extend_from_slice(&[0, 1]);
            assert!(decode_sprites(&data).is_err());
        }
    }

    #[test]
    fn measures_text_by_advances() {
        let font = Font {
            advances: [4; 256],
            max_height: 12,
            glyphs: Vec::new(),
        };
        assert_eq!(font.width("abc", 1), 12);
        assert_eq!(font.width("abc", 2), 24);
        assert_eq!(font.width("", 1), 0);
    }

    #[test]
    #[ignore]
    fn decodes_p12_full() {
        let dir = std::env::var("FONT_DATA_DIR").expect("set FONT_DATA_DIR");
        let metrics = std::fs::read(format!("{dir}/p12_metrics.bin")).unwrap();
        let sprites = std::fs::read(format!("{dir}/p12_sprite.bin")).unwrap();
        let font = Font::decode(&metrics, &sprites).unwrap();
        assert_eq!(font.glyphs.len(), 256);
        assert_eq!(font.advances[b'A' as usize] as i32, font.width("A", 1));
        println!(
            "max height {} width of 'Taiman' {}",
            font.max_height,
            font.width("Taiman", 1)
        );
    }
}
