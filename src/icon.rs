//! Window / taskbar icon: the NeonGauge arc logo, drawn in code as pixel art.

use eframe::egui::IconData;

const GRID: usize = 32; // logical pixels per side
const SCALE: usize = 4; // each logical pixel becomes a 4x4 block -> 128x128
const LEVEL: f32 = 0.62;

const BG: [u8; 4] = [11, 11, 20, 255];
const EMPTY: [u8; 4] = [28, 32, 48, 255];
const WHITE: [u8; 4] = [245, 247, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// Cyan -> yellow -> pink, same ramp as the logo.
fn ramp(t: f32) -> [u8; 4] {
    let stops = [[34.0, 240.0, 255.0], [255.0, 225.0, 77.0], [255.0, 46.0, 136.0]];
    let (lo, hi, u) = if t < 0.5 {
        (stops[0], stops[1], t / 0.5)
    } else {
        (stops[1], stops[2], (t - 0.5) / 0.5)
    };
    let mix = |i: usize| (lo[i] + (hi[i] - lo[i]) * u).round() as u8;
    [mix(0), mix(1), mix(2), 255]
}

fn pixel(x: usize, y: usize) -> [u8; 4] {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);

    // Rounded-square tile.
    let corner = 6.0;
    let n = GRID as f32;
    let qx = (px.min(n - px) - corner).min(0.0);
    let qy = (py.min(n - py) - corner).min(0.0);
    if qx * qx + qy * qy > corner * corner {
        return CLEAR;
    }

    let (cx, cy) = (16.0, 22.0);
    let (dx, dy) = (px - cx, cy - py);
    let r = dx.hypot(dy);

    // Segmented arc.
    if (9.0..=13.6).contains(&r) && dy > 0.0 {
        let t = 1.0 - dy.atan2(dx) / std::f32::consts::PI;
        let s = t * 8.0;
        if s - s.floor() < 0.15 {
            return BG;
        }
        return if t <= LEVEL { ramp(t) } else { EMPTY };
    }

    // Needle + hub.
    let a = std::f32::consts::PI * (1.0 - LEVEL);
    let (nx, ny) = (a.cos(), a.sin());
    let proj = dx * nx + dy * ny;
    let perp = (dx * ny - dy * nx).abs();
    if r < 1.9 || (proj > 0.0 && proj < 7.8 && perp < 0.6) {
        return WHITE;
    }
    BG
}

pub fn icon() -> IconData {
    let size = GRID * SCALE;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) * 4;
            rgba[i..i + 4].copy_from_slice(&pixel(x / SCALE, y / SCALE));
        }
    }
    IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}
