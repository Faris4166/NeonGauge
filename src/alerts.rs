//! Threshold alerts. Runs on the fetch thread so it keeps working while the
//! window is hidden in the tray.

use crate::api::{reset_clock, until_long, LimitWindow};
use crate::settings::Settings;
use crate::sfx::{self, Sfx};

/// A level only drops again once usage falls this far below the threshold,
/// so a reading that wobbles around 80% does not alert over and over.
const HYSTERESIS: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Default)]
pub enum Level {
    #[default]
    Ok,
    Warn,
    Critical,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// Usage climbed into a higher level.
    Raised(Level),
    /// Usage dropped back to below the warning threshold (the limit reset).
    Cleared,
}

/// Tracks one limit window (5-hour or weekly).
#[derive(Default, Debug)]
pub struct Tracker {
    level: Level,
}

impl Tracker {
    pub fn update(&mut self, pct: f32, warn: f32, crit: f32) -> Option<Event> {
        let raw = if pct >= crit {
            Level::Critical
        } else if pct >= warn {
            Level::Warn
        } else {
            Level::Ok
        };
        let prev = self.level;
        if raw > prev {
            self.level = raw;
            return Some(Event::Raised(raw));
        }
        // Step down only with some margin.
        let next = match prev {
            Level::Critical if pct < crit - HYSTERESIS => raw,
            Level::Warn if pct < warn - HYSTERESIS => raw,
            _ => prev,
        };
        self.level = next;
        (prev != Level::Ok && next == Level::Ok).then_some(Event::Cleared)
    }
}

#[derive(Default)]
pub struct Alerts {
    session: Tracker,
    weekly: Tracker,
}

impl Alerts {
    pub fn check(
        &mut self,
        session: Option<&LimitWindow>,
        weekly: Option<&LimitWindow>,
        s: &Settings,
    ) {
        if !s.alerts_enabled {
            return;
        }
        let rows = [
            ("5-hour session", session, &mut self.session, false),
            ("Weekly", weekly, &mut self.weekly, true),
        ];
        let mut loudest: Option<Sfx> = None;
        for (name, win, tracker, with_day) in rows {
            let Some(win) = win else { continue };
            let pct = win.percent();
            let Some(ev) = tracker.update(pct, s.warn_pct, s.crit_pct) else { continue };

            let reset = win
                .resets_at()
                .map(|r| format!("Resets in {} ({})", until_long(r), reset_clock(r, with_day)))
                .unwrap_or_default();
            let (title, sound, notify) = match ev {
                Event::Raised(Level::Critical) => (
                    format!("{name} limit at {pct:.0}% - almost out"),
                    Sfx::Critical,
                    true,
                ),
                Event::Raised(_) => (format!("{name} limit at {pct:.0}%"), Sfx::Warn, true),
                Event::Cleared => (
                    format!("{name} limit is back to {pct:.0}%"),
                    Sfx::Reset,
                    s.notify_reset,
                ),
            };
            if s.alert_notify && notify {
                notify_desktop(&title, &reset);
            }
            // Play one sound per check: the most urgent one.
            let rank = |x: Sfx| match x {
                Sfx::Critical => 3,
                Sfx::Warn => 2,
                _ => 1,
            };
            if loudest.is_none_or(|l| rank(sound) > rank(l)) {
                loudest = Some(sound);
            }
        }
        if let Some(sound) = loudest.filter(|_| s.alert_sound) {
            sfx::play(sound, s.volume);
        }
    }
}

pub fn notify_desktop(title: &str, body: &str) {
    let mut n = notify_rust::Notification::new();
    n.summary(title).body(body).appname("NeonGauge");
    #[cfg(target_os = "linux")]
    n.icon("dialog-warning");
    if let Err(e) = n.show() {
        eprintln!("notification failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raises_once_per_level() {
        let mut t = Tracker::default();
        assert_eq!(t.update(50.0, 80.0, 95.0), None);
        assert_eq!(t.update(81.0, 80.0, 95.0), Some(Event::Raised(Level::Warn)));
        assert_eq!(t.update(84.0, 80.0, 95.0), None);
        assert_eq!(t.update(96.0, 80.0, 95.0), Some(Event::Raised(Level::Critical)));
        assert_eq!(t.update(99.0, 80.0, 95.0), None);
    }

    #[test]
    fn jumps_straight_to_critical() {
        let mut t = Tracker::default();
        assert_eq!(t.update(97.0, 80.0, 95.0), Some(Event::Raised(Level::Critical)));
    }

    #[test]
    fn hysteresis_prevents_flapping() {
        let mut t = Tracker::default();
        t.update(80.0, 80.0, 95.0);
        assert_eq!(t.update(79.0, 80.0, 95.0), None);
        assert_eq!(t.update(80.0, 80.0, 95.0), None, "no repeat alert while wobbling");
        assert_eq!(t.update(10.0, 80.0, 95.0), Some(Event::Cleared));
        assert_eq!(t.update(81.0, 80.0, 95.0), Some(Event::Raised(Level::Warn)));
    }

    #[test]
    fn critical_steps_down_to_warn_silently() {
        let mut t = Tracker::default();
        t.update(96.0, 80.0, 95.0);
        assert_eq!(t.update(85.0, 80.0, 95.0), None);
        assert_eq!(t.update(96.0, 80.0, 95.0), Some(Event::Raised(Level::Critical)));
    }
}
