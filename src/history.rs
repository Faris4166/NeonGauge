//! Local usage history: a small CSV in the OS data directory, plus the
//! 24-hour / 7-day charts shown in the History window. Nothing is uploaded.

use crate::pixel::pal;
use chrono::{DateTime, Duration as ChronoDuration, Local, TimeZone, Timelike};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Shape, Stroke};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

/// Samples older than this are dropped when the file is loaded.
const KEEP_DAYS: i64 = 35;
/// Identical readings closer together than this are not stored again.
const DEDUP_SECS: i64 = 15 * 60;
/// Wider gaps break the line in the 24-hour chart (app was closed or offline).
const GAP_SECS: i64 = 20 * 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub ts: i64,
    pub session: f32,
    pub weekly: f32,
}

#[derive(Default)]
pub struct History {
    pub samples: Vec<Sample>,
    path: Option<PathBuf>,
}

pub fn path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("NeonGauge").join("history.csv"))
}

fn parse_line(line: &str) -> Option<Sample> {
    let mut it = line.split(',');
    Some(Sample {
        ts: it.next()?.trim().parse().ok()?,
        session: it.next()?.trim().parse().ok()?,
        weekly: it.next()?.trim().parse().ok()?,
    })
}

impl History {
    pub fn load() -> Self {
        let path = path();
        let cutoff = Local::now().timestamp() - KEEP_DAYS * 86_400;
        let raw = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .unwrap_or_default();
        let total = raw.lines().count();
        let mut samples: Vec<Sample> = raw
            .lines()
            .filter_map(parse_line)
            .filter(|s| s.ts >= cutoff)
            .collect();
        samples.sort_by_key(|s| s.ts);
        let h = Self { samples, path };
        // Rewrite the file when old rows were pruned so it does not grow forever.
        if h.samples.len() < total {
            h.rewrite();
        }
        h
    }

    fn rewrite(&self) {
        let Some(p) = &self.path else { return };
        let body: String = self
            .samples
            .iter()
            .map(|s| format!("{},{:.1},{:.1}\n", s.ts, s.session, s.weekly))
            .collect();
        let _ = fs::write(p, body);
    }

    /// Record a reading. Returns false when it was skipped as a duplicate.
    pub fn record(&mut self, s: Sample) -> bool {
        if let Some(last) = self.samples.last() {
            let same = (last.session - s.session).abs() < 0.05
                && (last.weekly - s.weekly).abs() < 0.05;
            if same && s.ts - last.ts < DEDUP_SECS {
                return false;
            }
        }
        self.samples.push(s);
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                let _ = fs::create_dir_all(dir);
            }
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{},{:.1},{:.1}", s.ts, s.session, s.weekly);
            }
        }
        true
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        if let Some(p) = &self.path {
            let _ = fs::remove_file(p);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Day,
    Week,
}

pub struct DayStat {
    pub date: chrono::NaiveDate,
    pub peak_session: f32,
    pub peak_weekly: f32,
    pub samples: usize,
}

/// Peak readings per local calendar day, oldest first, for the last `days` days.
pub fn daily_peaks(samples: &[Sample], days: i64, now: DateTime<Local>) -> Vec<DayStat> {
    let today = now.date_naive();
    (0..days)
        .rev()
        .map(|back| {
            let date = today - ChronoDuration::days(back);
            let mut stat = DayStat { date, peak_session: 0.0, peak_weekly: 0.0, samples: 0 };
            for s in samples {
                let Some(t) = Local.timestamp_opt(s.ts, 0).single() else { continue };
                if t.date_naive() == date {
                    stat.peak_session = stat.peak_session.max(s.session);
                    stat.peak_weekly = stat.peak_weekly.max(s.weekly);
                    stat.samples += 1;
                }
            }
            stat
        })
        .collect()
}

fn level_color(p: f32, warn: f32, crit: f32) -> Color32 {
    if p >= crit {
        pal::RED
    } else if p >= warn {
        pal::ORANGE
    } else if p >= warn * 0.6 {
        pal::YELLOW
    } else {
        pal::LIME
    }
}

const SESSION_COL: Color32 = pal::CYAN;
const WEEKLY_COL: Color32 = pal::MAGENTA;

fn small() -> FontId {
    FontId::monospace(10.0)
}

/// Background, y grid and threshold lines. Returns the plotting rect.
fn chart_frame(painter: &egui::Painter, outer: Rect, warn: f32, crit: f32) -> Rect {
    painter.rect_filled(outer, 2.0, pal::BG);
    let plot = Rect::from_min_max(outer.min + vec2(30.0, 10.0), outer.max - vec2(10.0, 20.0));
    let y_of = |p: f32| plot.bottom() - plot.height() * p / 100.0;
    for p in [0.0, 25.0, 50.0, 75.0, 100.0] {
        let y = y_of(p);
        painter.line_segment([pos2(plot.left(), y), pos2(plot.right(), y)], Stroke::new(1.0, pal::EMPTY));
        painter.text(pos2(plot.left() - 5.0, y), Align2::RIGHT_CENTER, format!("{p:.0}"), small(), pal::DIM);
    }
    for (p, col) in [(warn, pal::YELLOW), (crit, pal::RED)] {
        let y = y_of(p);
        painter.extend(Shape::dashed_line(
            &[pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, col.gamma_multiply(0.7)),
            4.0,
            3.0,
        ));
    }
    plot
}

fn legend(ui: &mut egui::Ui, warn: f32, crit: f32) {
    ui.horizontal_wrapped(|ui| {
        let chip = |ui: &mut egui::Ui, col: Color32, text: &str| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, col);
            ui.label(egui::RichText::new(text).small());
        };
        chip(ui, SESSION_COL, "5-hour session");
        chip(ui, WEEKLY_COL, "Weekly");
        chip(ui, pal::YELLOW, &format!("Warn {warn:.0}%"));
        chip(ui, pal::RED, &format!("Critical {crit:.0}%"));
    });
}

fn day_chart(ui: &mut egui::Ui, samples: &[Sample], warn: f32, crit: f32) {
    let now = Local::now();
    let t1 = now.timestamp();
    let t0 = t1 - 86_400;
    let (outer, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 200.0), Sense::hover());
    let painter = ui.painter_at(outer);
    let plot = chart_frame(&painter, outer, warn, crit);
    let x_of = |ts: i64| plot.left() + plot.width() * (ts - t0) as f32 / (t1 - t0) as f32;
    let y_of = |p: f32| plot.bottom() - plot.height() * p.clamp(0.0, 100.0) / 100.0;

    // Hour ticks every 6 hours, on local wall-clock hours.
    let first = now - ChronoDuration::hours(24);
    let mut tick = first
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .unwrap_or(first)
        + ChronoDuration::hours(1);
    while tick.timestamp() <= t1 {
        if tick.hour().is_multiple_of(6) {
            let x = x_of(tick.timestamp());
            painter.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.0, pal::EMPTY.gamma_multiply(0.6)));
            painter.text(pos2(x, plot.bottom() + 4.0), Align2::CENTER_TOP, tick.format("%H:00").to_string(), small(), pal::DIM);
        }
        tick += ChronoDuration::hours(1);
    }

    let visible: Vec<&Sample> = samples.iter().filter(|s| s.ts >= t0).collect();
    for (get, col) in [
        (Box::new(|s: &Sample| s.weekly) as Box<dyn Fn(&Sample) -> f32>, WEEKLY_COL),
        (Box::new(|s: &Sample| s.session), SESSION_COL),
    ] {
        for w in visible.windows(2) {
            if w[1].ts - w[0].ts > GAP_SECS {
                continue;
            }
            painter.line_segment(
                [pos2(x_of(w[0].ts), y_of(get(w[0]))), pos2(x_of(w[1].ts), y_of(get(w[1])))],
                Stroke::new(2.0, col),
            );
        }
        if let Some(last) = visible.last() {
            painter.circle_filled(pos2(x_of(last.ts), y_of(get(last))), 3.0, col);
        }
    }

    if visible.is_empty() {
        painter.text(plot.center(), Align2::CENTER_CENTER, "No readings in the last 24 hours", FontId::proportional(13.0), pal::DIM);
    }

    // Crosshair + readout for the nearest sample.
    if let Some(p) = resp.hover_pos().filter(|p| plot.x_range().contains(p.x)) {
        let ts = t0 + ((p.x - plot.left()) / plot.width() * (t1 - t0) as f32) as i64;
        if let Some(s) = visible.iter().min_by_key(|s| (s.ts - ts).abs()) {
            let x = x_of(s.ts);
            painter.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.0, pal::TEXT.gamma_multiply(0.5)));
            let when = Local.timestamp_opt(s.ts, 0).single().map(|t| t.format("%a %H:%M").to_string()).unwrap_or_default();
            let text = format!("{when}  5H {:.0}%  7D {:.0}%", s.session, s.weekly);
            let anchor = if x > plot.center().x { Align2::RIGHT_TOP } else { Align2::LEFT_TOP };
            let off = if x > plot.center().x { -6.0 } else { 6.0 };
            let galley_rect = painter.text(pos2(x + off, plot.top() + 2.0), anchor, &text, small(), pal::TEXT);
            painter.rect_filled(galley_rect.expand(3.0), 2.0, pal::PANEL.gamma_multiply(0.9));
            painter.text(pos2(x + off, plot.top() + 2.0), anchor, &text, small(), pal::TEXT);
        }
    }
}

fn week_chart(ui: &mut egui::Ui, samples: &[Sample], warn: f32, crit: f32) {
    let days = daily_peaks(samples, 7, Local::now());
    let (outer, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 200.0), Sense::hover());
    let painter = ui.painter_at(outer);
    let plot = chart_frame(&painter, outer, warn, crit);
    let y_of = |p: f32| plot.bottom() - plot.height() * p.clamp(0.0, 100.0) / 100.0;
    let slot = plot.width() / days.len() as f32;
    let hover = resp.hover_pos();
    let mut readout = None;

    for (i, d) in days.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let bw = (slot * 0.5).min(28.0);
        let col_rect = Rect::from_x_y_ranges(cx - slot / 2.0..=cx + slot / 2.0, plot.y_range());
        let hovered = hover.is_some_and(|p| col_rect.contains(p));
        if hovered {
            painter.rect_filled(col_rect, 0.0, pal::PANEL);
        }
        if d.samples > 0 {
            let bar = Rect::from_min_max(pos2(cx - bw / 2.0, y_of(d.peak_session)), pos2(cx + bw / 2.0, plot.bottom()));
            painter.rect_filled(bar, 0.0, level_color(d.peak_session, warn, crit));
            // Weekly limit peak as a magenta tick across the slot.
            let y = y_of(d.peak_weekly);
            painter.line_segment([pos2(cx - bw / 2.0 - 4.0, y), pos2(cx + bw / 2.0 + 4.0, y)], Stroke::new(3.0, WEEKLY_COL));
        } else {
            painter.text(pos2(cx, plot.bottom() - 8.0), Align2::CENTER_CENTER, "-", small(), pal::DIM);
        }
        let label = if i == days.len() - 1 { "Today".to_string() } else { d.date.format("%a").to_string() };
        painter.text(pos2(cx, plot.bottom() + 4.0), Align2::CENTER_TOP, label, small(), if hovered { pal::TEXT } else { pal::DIM });
        if hovered {
            readout = Some(d);
        }
    }

    if let Some(d) = readout {
        let text = if d.samples == 0 {
            format!("{}  no readings", d.date.format("%a %d %b"))
        } else {
            format!("{}  peak 5H {:.0}%  peak 7D {:.0}%", d.date.format("%a %d %b"), d.peak_session, d.peak_weekly)
        };
        let pos = pos2(plot.center().x, plot.top() + 2.0);
        let r = painter.text(pos, Align2::CENTER_TOP, &text, small(), pal::TEXT);
        painter.rect_filled(r.expand(3.0), 2.0, pal::PANEL.gamma_multiply(0.9));
        painter.text(pos, Align2::CENTER_TOP, &text, small(), pal::TEXT);
    }
}

/// The chart section of the History window.
pub fn ui(ui: &mut egui::Ui, history: &mut History, view: &mut View, warn: f32, crit: f32) {
    ui.horizontal(|ui| {
        ui.selectable_value(view, View::Day, "24 hours");
        ui.selectable_value(view, View::Week, "7 days");
    });
    ui.add_space(4.0);
    match view {
        View::Day => day_chart(ui, &history.samples, warn, crit),
        View::Week => week_chart(ui, &history.samples, warn, crit),
    }
    legend(ui, warn, crit);

    // Short summary under the chart.
    let days = daily_peaks(&history.samples, 7, Local::now());
    let active: Vec<&DayStat> = days.iter().filter(|d| d.samples > 0).collect();
    if !active.is_empty() {
        let avg = active.iter().map(|d| d.peak_session).sum::<f32>() / active.len() as f32;
        let hot = active.iter().filter(|d| d.peak_session >= warn).count();
        ui.label(format!(
            "Last 7 days: average daily 5H peak {avg:.0}%, {hot} day(s) at or above {warn:.0}%"
        ));
    }
    ui.horizontal(|ui| {
        ui.weak(format!("{} readings stored on this computer", history.samples.len()));
        if !history.samples.is_empty() && ui.small_button("Clear history").clicked() {
            history.clear();
        }
    });
    if let Some(p) = path() {
        ui.weak(p.display().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects_lines() {
        assert_eq!(
            parse_line("1700000000,42.0,13.5"),
            Some(Sample { ts: 1_700_000_000, session: 42.0, weekly: 13.5 })
        );
        assert_eq!(parse_line("garbage"), None);
        assert_eq!(parse_line("1,2"), None);
    }

    #[test]
    fn dedups_identical_readings() {
        let mut h = History::default();
        assert!(h.record(Sample { ts: 0, session: 10.0, weekly: 5.0 }));
        assert!(!h.record(Sample { ts: 60, session: 10.0, weekly: 5.0 }));
        assert!(h.record(Sample { ts: 120, session: 11.0, weekly: 5.0 }));
        assert!(h.record(Sample { ts: 120 + DEDUP_SECS, session: 11.0, weekly: 5.0 }));
        assert_eq!(h.samples.len(), 3);
    }

    #[test]
    fn daily_peaks_bucket_by_local_day() {
        let now = Local::now();
        let s = vec![
            Sample { ts: now.timestamp() - 10, session: 30.0, weekly: 10.0 },
            Sample { ts: now.timestamp() - 5, session: 70.0, weekly: 12.0 },
        ];
        let days = daily_peaks(&s, 7, now);
        assert_eq!(days.len(), 7);
        let today = days.last().unwrap();
        assert_eq!(today.samples, 2);
        assert_eq!(today.peak_session, 70.0);
        assert_eq!(today.peak_weekly, 12.0);
    }
}
