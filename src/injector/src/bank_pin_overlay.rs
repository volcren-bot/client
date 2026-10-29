//! The panel drawn over the bank PIN keypad while `bank_pin_enter` handles the PIN: the keypad darkened, the VOLCREN
//! logo and name in its top left corner and what is running in its middle, in the Tokyo Night colours (the logo keeps its own).
//!
//! The art is pre-rendered by `assets/make_bank_pin_art.sh` (the logo, and the text in Fira Code Bold), so nothing here
//! needs the font at run time. The overlay is blended with straight alpha, so the art keeps its smooth edges.

// Only the Windows injector has an overlay to paint on, so on other hosts nothing here is called outside tests.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::Rgba;

const LOGO: &[u8] = include_bytes!("../assets/bank_pin_logo.bin");
const TITLE: &[u8] = include_bytes!("../assets/bank_pin_title.bin");
const MESSAGE: &[u8] = include_bytes!("../assets/bank_pin_message.bin");

/// Tokyo Night's background over the keypad, see-through enough to show it; the border is its `blue0`.
const SHADE: Rgba = Rgba {
    r: 0x1a,
    g: 0x1b,
    b: 0x26,
    a: 200,
};
const BORDER: Rgba = Rgba {
    r: 0x3d,
    g: 0x59,
    b: 0xa1,
    a: 255,
};
const BORDER_WIDTH: i32 = 2;
/// From the panel's edge to the logo, and from the logo to the name.
const MARGIN: i32 = 10;
const GAP: i32 = 8;

/// Straight alpha RGBA pixels, row by row, as the art files hold them: a u16 width and height, little endian, then the pixels.
struct Art<'a> {
    width: i32,
    height: i32,
    pixels: &'a [u8],
}

impl<'a> Art<'a> {
    fn parse(data: &'a [u8]) -> Self {
        let width = i32::from(u16::from_le_bytes([data[0], data[1]]));
        let height = i32::from(u16::from_le_bytes([data[2], data[3]]));
        let pixels = &data[4..];
        assert_eq!(
            pixels.len(),
            (width * height * 4) as usize,
            "the bank PIN art is truncated"
        );
        Self {
            width,
            height,
            pixels,
        }
    }

    fn pixel(&self, x: i32, y: i32) -> Rgba {
        let at = ((y * self.width + x) * 4) as usize;
        Rgba {
            r: self.pixels[at],
            g: self.pixels[at + 1],
            b: self.pixels[at + 2],
            a: self.pixels[at + 3],
        }
    }
}

/// `color` drawn over the shade, as one pixel with its own alpha, so the overlay shows the same as the two would.
fn over_shade(color: Rgba) -> Rgba {
    let (a, shade) = (f32::from(color.a) / 255.0, f32::from(SHADE.a) / 255.0);
    let out = a + shade * (1.0 - a);
    if out <= 0.0 {
        return SHADE;
    }
    let mix =
        |c: u8, s: u8| ((f32::from(c) * a + f32::from(s) * shade * (1.0 - a)) / out).round() as u8;
    Rgba {
        r: mix(color.r, SHADE.r),
        g: mix(color.g, SHADE.g),
        b: mix(color.b, SHADE.b),
        a: (out * 255.0).round() as u8,
    }
}

/// Draws `art` with its top left at `(x, y)`, `width` pixels wide at most (shrunk to fit, keeping its shape).
fn blit(art: &Art, x: i32, y: i32, width: i32, put: &mut dyn FnMut(i32, i32, Rgba)) {
    let width = width.clamp(1, art.width);
    let height = (art.height * width / art.width).max(1);
    for dy in 0..height {
        for dx in 0..width {
            let color = art.pixel(dx * art.width / width, dy * art.height / height);
            if color.a > 0 {
                put(x + dx, y + dy, over_shade(color));
            }
        }
    }
}

/// Paints the panel over `area` (`[x1, y1, x2, y2]`, canvas pixels) through `put(x, y, color)`.
pub fn paint(area: [i32; 4], put: &mut dyn FnMut(i32, i32, Rgba)) {
    let [x1, y1, x2, y2] = area;
    for y in y1..=y2 {
        for x in x1..=x2 {
            let edge = x - x1 < BORDER_WIDTH
                || x2 - x < BORDER_WIDTH
                || y - y1 < BORDER_WIDTH
                || y2 - y < BORDER_WIDTH;
            put(x, y, if edge { BORDER } else { SHADE });
        }
    }

    let (logo, title, message) = (Art::parse(LOGO), Art::parse(TITLE), Art::parse(MESSAGE));
    blit(&logo, x1 + MARGIN, y1 + MARGIN, logo.width, put);
    blit(
        &title,
        x1 + MARGIN + logo.width + GAP,
        y1 + MARGIN + (logo.height - title.height) / 2,
        title.width,
        put,
    );

    let width = message.width.min(x2 - x1 + 1 - 2 * MARGIN);
    let height = (message.height * width.max(1) / message.width).max(1);
    blit(
        &message,
        x1 + (x2 - x1 + 1 - width) / 2,
        y1 + (y2 - y1 + 1 - height) / 2,
        width,
        put,
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // Tokyo Night's foreground, what `make_bank_pin_art.sh` draws the message in.
    const MESSAGE_COLOR: Rgba = Rgba {
        r: 0xc0,
        g: 0xca,
        b: 0xf5,
        a: 255,
    };

    fn painted(area: [i32; 4]) -> HashMap<(i32, i32), Rgba> {
        let mut canvas = HashMap::new();
        paint(area, &mut |x, y, color| {
            canvas.insert((x, y), color);
        });
        canvas
    }

    #[test]
    fn darkens_exactly_the_area() {
        let area = [100, 50, 499, 349];
        let canvas = painted(area);
        assert_eq!(canvas.len(), 400 * 300);
        assert!(
            canvas
                .keys()
                .all(|&(x, y)| (100..=499).contains(&x) && (50..=349).contains(&y))
        );
        assert_eq!(canvas[&(499, 349)], BORDER);
        assert_eq!(canvas[&(100 + BORDER_WIDTH, 50 + BORDER_WIDTH)], SHADE);
    }

    #[test]
    fn the_logo_is_in_the_top_left_and_the_text_in_the_middle() {
        let canvas = painted([100, 50, 499, 349]);
        let logo = Art::parse(LOGO);
        let in_logo = |&(x, y): &(i32, i32)| {
            x >= 100 + MARGIN
                && x < 100 + MARGIN + logo.width
                && y >= 50 + MARGIN
                && y < 50 + MARGIN + logo.height
        };
        assert!(
            canvas
                .iter()
                .any(|(at, color)| in_logo(at) && color.a == 255 && color.r > 200 && color.b < 120),
            "no flame in the corner"
        );
        let white: Vec<_> = canvas
            .iter()
            .filter(|(_, c)| **c == MESSAGE_COLOR)
            .map(|(&at, _)| at)
            .collect();
        assert!(!white.is_empty(), "no message");
        let middle_y = (50 + 349) / 2;
        assert!(
            white.iter().all(|&(_, y)| (y - middle_y).abs() < 20),
            "the message isn't in the middle"
        );
        let (left, right) = (
            white.iter().map(|p| p.0).min().unwrap(),
            white.iter().map(|p| p.0).max().unwrap(),
        );
        assert!(
            ((left + right) / 2 - (100 + 499) / 2).abs() <= 8,
            "the message isn't centred: {left}..{right}"
        );
    }

    #[test]
    fn a_narrow_keypad_shrinks_the_message_to_fit() {
        let area = [0, 0, 199, 149];
        let canvas = painted(area);
        assert!(
            canvas
                .keys()
                .all(|&(x, y)| (0..=199).contains(&x) && (0..=149).contains(&y))
        );
        assert!(canvas.values().any(|c| *c == MESSAGE_COLOR));
    }

    #[test]
    fn the_art_files_parse() {
        for data in [LOGO, TITLE, MESSAGE] {
            Art::parse(data);
        }
    }
}
