//! User settings, persisted as JSON in the OS config directory.

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

pub const REFRESH_CHOICES: [u64; 6] = [30, 60, 120, 300, 600, 900];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Seconds between automatic refreshes.
    pub refresh_secs: u64,
    /// Window opacity, 0.3 ..= 1.0.
    pub opacity: f32,
    /// Size of one canvas dot in logical pixels, 2 ..= 6.
    pub scale: f32,
    pub always_on_top: bool,

    pub alerts_enabled: bool,
    pub warn_pct: f32,
    pub crit_pct: f32,
    pub alert_sound: bool,
    pub alert_notify: bool,
    /// Also notify when a limit resets back to low usage.
    pub notify_reset: bool,

    /// Click / open / refresh sounds.
    pub ui_sfx: bool,
    /// Master volume, 0.0 ..= 1.0.
    pub volume: f32,

    /// Show an icon in the system tray; the X button then hides to the tray.
    pub tray: bool,
    /// When launched at login, start hidden in the tray.
    pub start_hidden: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_secs: 60,
            opacity: 1.0,
            scale: 3.0,
            always_on_top: true,
            alerts_enabled: true,
            warn_pct: 80.0,
            crit_pct: 95.0,
            alert_sound: true,
            alert_notify: true,
            notify_reset: false,
            ui_sfx: true,
            volume: 0.6,
            tray: true,
            start_hidden: true,
        }
    }
}

impl Settings {
    /// Clamp values that may have been hand-edited into nonsense.
    pub fn sanitized(mut self) -> Self {
        self.refresh_secs = self.refresh_secs.clamp(15, 3600);
        self.opacity = self.opacity.clamp(0.3, 1.0);
        self.scale = self.scale.round().clamp(2.0, 6.0);
        self.warn_pct = self.warn_pct.clamp(1.0, 100.0);
        self.crit_pct = self.crit_pct.clamp(self.warn_pct, 100.0);
        self.volume = self.volume.clamp(0.0, 1.0);
        self
    }

    pub fn load() -> Self {
        path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str::<Settings>(&raw).ok())
            .unwrap_or_default()
            .sanitized()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("no config directory")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }
}

pub fn app_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("NeonGauge"))
}

pub fn path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("settings.json"))
}
