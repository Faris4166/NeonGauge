//! A tiny pixel canvas: everything is drawn on a grid of square "dots",
//! plus a hand-made 3x5 bitmap font.

use eframe::egui::Color32;
use std::cell::RefCell;

/// Neon tech palette.
pub mod pal {
    use eframe::egui::Color32;

    pub const BG: Color32 = Color32::from_rgb(10, 13, 26);
    pub const PANEL: Color32 = Color32::from_rgb(17, 23, 42);
    pub const DIM: Color32 = Color32::from_rgb(70, 88, 128);
    pub const TEXT: Color32 = Color32::from_rgb(226, 238, 255);
    pub const EMPTY: Color32 = Color32::from_rgb(34, 44, 72);
    pub const BUTTON: Color32 = Color32::from_rgb(30, 42, 70);

    pub const CYAN: Color32 = Color32::from_rgb(0, 240, 255);
    pub const MAGENTA: Color32 = Color32::from_rgb(255, 43, 214);
    pub const LIME: Color32 = Color32::from_rgb(166, 255, 0);
    pub const YELLOW: Color32 = Color32::from_rgb(255, 230, 0);
    pub const ORANGE: Color32 = Color32::from_rgb(255, 138, 0);
    pub const RED: Color32 = Color32::from_rgb(255, 46, 77);
}

/// A CPU framebuffer of `w` x `h` "dots". Everything is drawn here first, then
/// uploaded as one small texture per frame. Far cheaper than handing egui
/// thousands of separate rectangles to tessellate every frame.
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    buf: RefCell<Vec<Color32>>,
}

/// `src` over `dst`, both premultiplied (as `Color32` is).
fn blend(dst: Color32, src: Color32) -> Color32 {
    let a = src.a() as u32;
    if a == 255 {
        return src;
    }
    if a == 0 && src == Color32::TRANSPARENT {
        return dst;
    }
    let inv = 255 - a;
    let mix = |s: u8, d: u8| (s as u32 + (d as u32 * inv + 127) / 255).min(255) as u8;
    Color32::from_rgba_premultiplied(
        mix(src.r(), dst.r()),
        mix(src.g(), dst.g()),
        mix(src.b(), dst.b()),
        mix(src.a(), dst.a()),
    )
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            buf: RefCell::new(vec![Color32::TRANSPARENT; (w * h) as usize]),
        }
    }

    pub fn clear(&self) {
        self.buf.borrow_mut().fill(Color32::TRANSPARENT);
    }

    /// Fill a `w` x `h` block of dots at grid position (`x`, `y`), clipped to the canvas.
    pub fn px(&self, x: i32, y: i32, w: i32, h: i32, color: Color32) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(self.w), (y + h).min(self.h));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let mut buf = self.buf.borrow_mut();
        for yy in y0..y1 {
            let row = (yy * self.w) as usize;
            for p in &mut buf[row + x0 as usize..row + x1 as usize] {
                *p = blend(*p, color);
            }
        }
    }

    /// Draw text with the 3x5 font; each glyph advances 4 dots.
    pub fn text(&self, x: i32, y: i32, s: &str, color: Color32) {
        for (i, ch) in s.chars().enumerate() {
            let Some(rows) = glyph(ch.to_ascii_uppercase()) else {
                continue;
            };
            let gx = x + i as i32 * 4;
            for (ry, bits) in rows.iter().enumerate() {
                for rx in 0..3 {
                    if (bits >> (2 - rx)) & 1 == 1 {
                        self.px(gx + rx, y + ry as i32, 1, 1, color);
                    }
                }
            }
        }
    }

    pub fn pixels(&self) -> std::cell::Ref<'_, Vec<Color32>> {
        self.buf.borrow()
    }
}

pub fn text_width(s: &str) -> i32 {
    (s.chars().count() as i32 * 4 - 1).max(0)
}

/// 3x5 glyphs, one byte per row, the low 3 bits are the pixels.
fn glyph(c: char) -> Option<[u8; 5]> {
    Some(match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b111, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        '?' => [0b110, 0b001, 0b010, 0b000, 0b010],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '>' => [0b100, 0b010, 0b001, 0b010, 0b100],
        ' ' => [0; 5],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn px_clips_and_overwrites_opaque() {
        let c = Canvas::new(4, 3);
        c.px(-2, -2, 4, 4, pal::CYAN);
        c.px(3, 2, 10, 10, pal::RED);
        let p = c.pixels();
        assert_eq!(p[0], pal::CYAN);
        assert_eq!(p[5], pal::CYAN);
        assert_eq!(p[2], Color32::TRANSPARENT);
        assert_eq!(p[11], pal::RED);
    }

    #[test]
    fn blend_matches_premultiplied_over() {
        let half = pal::CYAN.gamma_multiply(0.5);
        let out = blend(pal::BG, half);
        assert_eq!(out.a(), 255);
        // Halfway-ish between BG and CYAN on every channel.
        assert!(out.g() > pal::BG.g() && out.g() < pal::CYAN.g());
        assert_eq!(blend(pal::BG, Color32::TRANSPARENT), pal::BG);
    }
}
