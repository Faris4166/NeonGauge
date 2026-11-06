//! A tiny pixel canvas: everything is drawn on a grid of square "dots",
//! plus a hand-made 3x5 bitmap font.

use eframe::egui::{vec2, Color32, Painter, Pos2, Rect};

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

pub struct Canvas {
    pub painter: Painter,
    pub origin: Pos2,
    pub dot: f32,
}

impl Canvas {
    /// Fill a `w` x `h` block of dots at grid position (`x`, `y`).
    pub fn px(&self, x: i32, y: i32, w: i32, h: i32, color: Color32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let min = self.origin + vec2(x as f32 * self.dot, y as f32 * self.dot);
        let size = vec2(w as f32 * self.dot, h as f32 * self.dot);
        self.painter
            .rect_filled(Rect::from_min_size(min, size), 0.0, color);
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

    /// Screen position -> grid position.
    pub fn to_dot(&self, p: Pos2) -> (i32, i32) {
        (
            ((p.x - self.origin.x) / self.dot).floor() as i32,
            ((p.y - self.origin.y) / self.dot).floor() as i32,
        )
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
