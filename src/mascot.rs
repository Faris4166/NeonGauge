//! 16x16 pixel-art mascot. The face, colours and animation change with usage.

use crate::pixel::{pal, Canvas};
use eframe::egui::Color32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Chill,   // < 30%
    Happy,   // < 60%
    Worried, // < 80%
    Panic,   // < 95%
    Dead,    // >= 95%
}

impl Mood {
    pub fn from_percent(p: f32) -> Self {
        match p {
            p if p < 30.0 => Mood::Chill,
            p if p < 60.0 => Mood::Happy,
            p if p < 80.0 => Mood::Worried,
            p if p < 95.0 => Mood::Panic,
            _ => Mood::Dead,
        }
    }

    /// Accent colour used for the status line and the frame.
    pub fn accent(self) -> Color32 {
        match self {
            Mood::Chill => pal::LIME,
            Mood::Happy => pal::CYAN,
            Mood::Worried => pal::YELLOW,
            Mood::Panic => pal::RED,
            Mood::Dead => pal::MAGENTA,
        }
    }
}

/// Size of one sprite pixel, in canvas dots.
pub const SCALE: i32 = 2;
pub const SIZE: i32 = 16 * SCALE;

const OUTLINE: Color32 = Color32::from_rgb(27, 16, 48);
const EYE: Color32 = pal::CYAN;
const PINK: Color32 = Color32::from_rgb(255, 95, 162);
const SWEAT: Color32 = Color32::from_rgb(125, 249, 255);

// O = outline, B = body, H = highlight, S = shade. Rows 0-2 hold the antenna spark.
const BASE: [&str; 16] = [
    "................",
    "................",
    "................",
    ".......OO.......",
    "...OOOOOOOOOO...",
    "..OHHHBBBBBBBO..",
    ".OHHBBBBBBBBBSO.",
    ".OHBBBBBBBBBBSO.",
    "OOBBBBBBBBBBBBOO",
    "OSBBBBBBBBBBBBSO",
    "OSBBBBBBBBBBBBSO",
    "OOBBBBBBBBBBBBOO",
    ".OSBBBBBBBBBBSO.",
    "..OSSSSSSSSSSO..",
    "...OSSO..OSSO...",
    "...OOOO..OOOO...",
];

struct Skin {
    body: Color32,
    hi: Color32,
    shade: Color32,
}

fn skin(mood: Mood) -> Skin {
    let rgb = Color32::from_rgb;
    match mood {
        Mood::Panic => Skin {
            body: rgb(255, 61, 110),
            hi: rgb(255, 150, 175),
            shade: rgb(192, 16, 63),
        },
        Mood::Dead => Skin {
            body: rgb(138, 111, 143),
            hi: rgb(170, 146, 175),
            shade: rgb(94, 72, 99),
        },
        _ => Skin {
            body: rgb(255, 122, 61),
            hi: rgb(255, 182, 107),
            shade: rgb(217, 72, 28),
        },
    }
}

/// Draws the mascot with its top-left corner at canvas dot (`x0`, `y0`).
pub fn draw(c: &Canvas, x0: i32, y0: i32, mood: Mood, t: f32) {
    let frame = |fps: f32| (t * fps) as i32;

    // Whole-sprite offset in sprite pixels: snappy, frame-based motion.
    let (dx, dy) = match mood {
        Mood::Chill => (0, frame(1.5) % 2),
        Mood::Happy => (0, [0, -1, -2, -1][(frame(6.0) % 4) as usize]),
        Mood::Worried => ([0, 0, 1, 0, 0, -1][(frame(3.0) % 6) as usize], 0),
        Mood::Panic => (if frame(14.0) % 2 == 0 { 1 } else { -1 }, 0),
        Mood::Dead => (0, 1),
    };
    let put = |sx: i32, sy: i32, col: Color32| {
        c.px(x0 + (sx + dx) * SCALE, y0 + (sy + dy) * SCALE, SCALE, SCALE, col);
    };

    // Shadow shrinks while airborne.
    let shrink = (-dy).max(0);
    c.px(
        x0 + (3 + shrink) * SCALE,
        y0 + 16 * SCALE,
        (10 - 2 * shrink) * SCALE,
        1,
        Color32::from_rgb(30, 38, 66),
    );

    let s = skin(mood);
    for (sy, row) in BASE.iter().enumerate() {
        for (sx, ch) in row.chars().enumerate() {
            let col = match ch {
                'O' => OUTLINE,
                'B' => s.body,
                'H' => s.hi,
                'S' => s.shade,
                _ => continue,
            };
            put(sx as i32, sy as i32, col);
        }
    }

    draw_spark(&put, mood, t);
    draw_arms(&put, mood, &s, frame(8.0));
    draw_face(&put, mood, t);

    if matches!(mood, Mood::Worried | Mood::Panic) {
        let speed = if mood == Mood::Panic { 8.0 } else { 3.0 };
        let fall = frame(speed) % 5;
        put(14, 3 + fall, SWEAT);
        put(14, 4 + fall, SWEAT);
    }
    if mood == Mood::Dead {
        // Floating "Z"s drawn in the pixel font.
        for i in 0..3 {
            let phase = (t * 0.6 + i as f32 / 3.0).fract();
            let zx = x0 + SIZE - 6 + (phase * 6.0) as i32;
            let zy = y0 + 6 - (phase * 10.0) as i32;
            c.text(zx, zy, "Z", if phase < 0.7 { pal::MAGENTA } else { pal::DIM });
        }
    }
}

fn draw_spark(put: &impl Fn(i32, i32, Color32), mood: Mood, t: f32) {
    if mood == Mood::Dead {
        put(7, 2, pal::DIM);
        put(8, 2, pal::DIM);
        return;
    }
    // Cycle through neon colours; faster when stressed.
    let speed = match mood {
        Mood::Chill => 1.0,
        Mood::Happy => 2.0,
        Mood::Worried => 3.0,
        _ => 8.0,
    };
    let colors = [pal::CYAN, pal::MAGENTA, pal::YELLOW, pal::LIME];
    let col = colors[(t * speed) as usize % colors.len()];
    for (x, y) in [(7, 1), (8, 1), (7, 2), (8, 2)] {
        put(x, y, col);
    }
    put(7, 1, Color32::WHITE);
    if (t * speed * 2.0) as i32 % 2 == 0 {
        for (x, y) in [(7, 0), (8, 0), (6, 1), (6, 2), (9, 1), (9, 2)] {
            put(x, y, col);
        }
    }
}

fn draw_arms(put: &impl Fn(i32, i32, Color32), mood: Mood, s: &Skin, f: i32) {
    match mood {
        // Waving right hand.
        Mood::Happy if f % 2 == 0 => {
            put(15, 6, OUTLINE);
            put(15, 7, s.shade);
        }
        // Both hands flailing.
        Mood::Panic => {
            let y = if f % 2 == 0 { 5 } else { 6 };
            for x in [0, 15] {
                put(x, y, OUTLINE);
                put(x, y + 1, s.shade);
            }
        }
        _ => {}
    }
}

fn draw_face(put: &impl Fn(i32, i32, Color32), mood: Mood, t: f32) {
    let blink = (t % 3.2) < 0.15;
    let eyes_2x2 = |hl: bool| {
        for ex in [4, 10] {
            for (x, y) in [(ex, 7), (ex + 1, 7), (ex, 8), (ex + 1, 8)] {
                put(x, y, EYE);
            }
            if hl {
                put(ex + 1, 7, Color32::WHITE);
            }
        }
    };
    let closed = || {
        for ex in [4, 10] {
            put(ex, 8, OUTLINE);
            put(ex + 1, 8, OUTLINE);
        }
    };
    let cheeks = || {
        put(3, 10, PINK);
        put(12, 10, PINK);
    };

    match mood {
        Mood::Chill => {
            // ^ ^ eyes and a small smile.
            for ex in [4, 10] {
                put(ex, 8, OUTLINE);
                put(ex + 1, 7, OUTLINE);
                put(ex + 2, 8, OUTLINE);
            }
            for (x, y) in [(6, 10), (7, 11), (8, 11), (9, 10)] {
                put(x, y, OUTLINE);
            }
            cheeks();
        }
        Mood::Happy => {
            if blink { closed() } else { eyes_2x2(true) }
            for x in 6..=9 {
                put(x, 10, OUTLINE);
            }
            put(7, 11, PINK);
            put(8, 11, PINK);
            cheeks();
        }
        Mood::Worried => {
            if blink { closed() } else { eyes_2x2(false) }
            // Brows: inner ends raised.
            for (x, y) in [(4, 6), (5, 5), (10, 5), (11, 6)] {
                put(x, y, OUTLINE);
            }
            for (x, y) in [(6, 11), (7, 10), (8, 11), (9, 10)] {
                put(x, y, OUTLINE);
            }
        }
        Mood::Panic => {
            // Big white eyes with jittering pupils.
            let jitter = ((t * 12.0) as i32) % 2;
            for ex in [3, 10] {
                for y in 6..=8 {
                    for x in ex..ex + 3 {
                        put(x, y, Color32::WHITE);
                    }
                }
                put(ex + jitter, 7, OUTLINE);
            }
            for (x, y) in [(7, 10), (8, 10), (7, 11), (8, 11)] {
                put(x, y, OUTLINE);
            }
        }
        Mood::Dead => {
            // X X eyes, flat mouth, tongue out.
            for ex in [4, 9] {
                for (x, y) in [(ex, 7), (ex + 2, 7), (ex + 1, 8), (ex, 9), (ex + 2, 9)] {
                    put(x, y, OUTLINE);
                }
            }
            for x in 6..=9 {
                put(x, 11, OUTLINE);
            }
            put(8, 12, PINK);
        }
    }
}
