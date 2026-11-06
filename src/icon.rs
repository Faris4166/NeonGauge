//! App icons, rendered from `assets/logo.svg` (Gauge-chan) and embedded as PNG.
//! The Windows .exe icon is `assets/icon.ico`, embedded by `build.rs`.

use eframe::egui::IconData;

const WINDOW_PNG: &[u8] = include_bytes!("../assets/icon.png");
const TRAY_PNG: &[u8] = include_bytes!("../assets/icon-tray.png");

fn decode(png: &[u8]) -> IconData {
    eframe::icon_data::from_png_bytes(png).unwrap_or_default()
}

/// Window and taskbar icon (256x256).
pub fn icon() -> IconData {
    decode(WINDOW_PNG)
}

/// Smaller copy for the system tray (64x64), so the OS downscale stays sharp.
pub fn tray() -> IconData {
    decode(TRAY_PNG)
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_icons_decode() {
        let w = super::icon();
        assert_eq!((w.width, w.height), (256, 256));
        assert_eq!(w.rgba.len(), 256 * 256 * 4);
        let t = super::tray();
        assert_eq!((t.width, t.height), (64, 64));
    }
}
