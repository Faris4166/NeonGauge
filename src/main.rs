#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod mascot;
mod pixel;

use api::{reset_clock, until_compact, until_long, FetchError, LimitWindow, Snapshot};
use chrono::{DateTime, Local};
use eframe::egui::{self, vec2, Color32, Id, Pos2, Rect, Sense};
use mascot::Mood;
use pixel::{pal, text_width, Canvas};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const REFRESH_SECS: u64 = 60;

/// Canvas size in dots, and the target size of one dot in logical pixels.
const GRID_W: i32 = 104;
const GRID_H: i32 = 48;
const DOT: f32 = 3.0;

/// Clickable buttons in dot coordinates: (x, y, w, h).
const BTN_REFRESH: (i32, i32, i32, i32) = (88, 3, 5, 7);
const BTN_CLOSE: (i32, i32, i32, i32) = (95, 3, 5, 7);

/// Layout of the info column.
const INFO_X: i32 = 41;
const BAR_X: i32 = 49;
const BAR_SEGMENTS: i32 = 16;
const PCT_RIGHT: i32 = 99;

#[derive(Default)]
struct Shared {
    snap: Option<Snapshot>,
    error: Option<FetchError>,
    updated: Option<DateTime<Local>>,
    loading: bool,
}

struct App {
    shared: Arc<Mutex<Shared>>,
    refresh_tx: mpsc::Sender<()>,
    start: Instant,
    shown_session: f32,
    shown_weekly: f32,
    pinned: bool,
    /// CLAUDE_LIMIT_DEMO=<number> pins the session percentage, =sweep cycles 0..100.
    demo: Option<String>,
    /// pixels_per_point the window was last sized for.
    sized_for: f32,
}

fn spawn_worker(shared: Arc<Mutex<Shared>>, ctx: egui::Context) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel::<()>();
    thread::spawn(move || loop {
        shared.lock().unwrap().loading = true;
        ctx.request_repaint();

        let result = api::fetch();
        {
            let mut s = shared.lock().unwrap();
            s.loading = false;
            match result {
                Ok(snap) => {
                    s.snap = Some(snap);
                    s.error = None;
                    s.updated = Some(Local::now());
                }
                Err(e) => s.error = Some(e),
            }
        }
        ctx.request_repaint();

        if let Err(mpsc::RecvTimeoutError::Disconnected) =
            rx.recv_timeout(Duration::from_secs(REFRESH_SECS))
        {
            break;
        }
        while rx.try_recv().is_ok() {}
    });
    tx
}

fn level_color(p: f32) -> Color32 {
    match p {
        p if p < 50.0 => pal::LIME,
        p if p < 75.0 => pal::YELLOW,
        p if p < 90.0 => pal::ORANGE,
        _ => pal::RED,
    }
}

fn mood_message(mood: Mood) -> &'static str {
    match mood {
        Mood::Chill => "> ALL CHILL",
        Mood::Happy => "> CRUISING",
        Mood::Worried => "> EASE UP...",
        Mood::Panic => "> DANGER ZONE!",
        Mood::Dead => "> BURNT OUT",
    }
}

fn in_box(p: (i32, i32), b: (i32, i32, i32, i32)) -> bool {
    p.0 >= b.0 && p.0 < b.0 + b.2 && p.1 >= b.1 && p.1 < b.1 + b.3
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Crisp pixel edges.
        cc.egui_ctx
            .options_mut(|o| o.tessellation_options.feathering = false);

        let shared = Arc::new(Mutex::new(Shared::default()));
        let refresh_tx = spawn_worker(shared.clone(), cc.egui_ctx.clone());
        Self {
            shared,
            refresh_tx,
            start: Instant::now(),
            shown_session: 0.0,
            shown_weekly: 0.0,
            pinned: true,
            demo: std::env::var("CLAUDE_LIMIT_DEMO").ok(),
            sized_for: 0.0,
        }
    }

    fn set_pinned(&mut self, ctx: &egui::Context, pinned: bool) {
        self.pinned = pinned;
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if pinned {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        }));
    }
}

/// Segmented neon bar: green -> yellow -> orange -> red along its length.
fn draw_bar(c: &Canvas, x: i32, y: i32, pct: f32, t: f32) {
    let filled = ((pct / 100.0) * BAR_SEGMENTS as f32).round() as i32;
    let blink = (t * 4.0) as i32 % 2 == 0;
    for i in 0..BAR_SEGMENTS {
        let frac = i as f32 / BAR_SEGMENTS as f32;
        let mut col = if i >= filled {
            pal::EMPTY
        } else if frac < 0.5 {
            pal::LIME
        } else if frac < 0.75 {
            pal::YELLOW
        } else if frac < 0.9 {
            pal::ORANGE
        } else {
            pal::RED
        };
        // The leading segment flashes when the limit is close.
        if i == filled - 1 && pct >= 80.0 && blink {
            col = Color32::WHITE;
        }
        c.px(x + i * 2, y, 1, 5, col);
    }
}

/// One limit: label, bar and percentage, then the exact reset time plus a countdown.
/// `with_day` adds the weekday, needed for the weekly limit.
fn draw_limit_row(
    c: &Canvas,
    y: i32,
    label: &str,
    pct: f32,
    window: Option<&LimitWindow>,
    with_day: bool,
    t: f32,
) {
    c.text(INFO_X, y, label, pal::DIM);
    draw_bar(c, BAR_X, y, pct, t);
    let pct_text = format!("{:.0}%", pct);
    c.text(PCT_RIGHT - text_width(&pct_text) + 1, y, &pct_text, level_color(pct));

    let Some(reset) = window.and_then(|w| w.resets_at()) else {
        c.text(INFO_X, y + 7, "RESET --:--", pal::DIM);
        return;
    };
    // Session: "AT 02:49 2H30M". Weekly: "TUE 06:59 3D6H".
    let clock = if with_day {
        reset_clock(reset, true)
    } else {
        format!("AT {}", reset_clock(reset, false))
    };
    c.text(INFO_X, y + 7, &clock, pal::TEXT);
    c.text(
        INFO_X + text_width(&clock) + 4,
        y + 7,
        &until_compact(reset),
        pal::DIM,
    );
}

fn draw_button(c: &Canvas, b: (i32, i32, i32, i32), glyph: &str, hovered: bool, accent: Color32) {
    let (bg, fg) = if hovered {
        (accent, pal::BG)
    } else {
        (pal::BUTTON, pal::TEXT)
    };
    c.px(b.0, b.1, b.2, b.3, bg);
    c.text(b.0 + 1, b.1 + 1, glyph, fg);
}

/// Pixel frame with notched corners.
fn draw_frame(c: &Canvas, col: Color32) {
    let (w, h) = (GRID_W, GRID_H);
    c.px(0, 0, w, h, pal::BG);
    c.px(1, 1, w - 2, h - 2, pal::PANEL);
    c.px(2, 0, w - 4, 1, col);
    c.px(2, h - 1, w - 4, 1, col);
    c.px(0, 2, 1, h - 4, col);
    c.px(w - 1, 2, 1, h - 4, col);
    for (x, y) in [(1, 1), (w - 2, 1), (1, h - 2), (w - 2, h - 2)] {
        c.px(x, y, 1, 1, col);
    }
    // Dotted divider between the mascot and the info column.
    for y in (4..h - 4).step_by(2) {
        c.px(38, y, 1, 1, pal::EMPTY);
    }
}

/// CRT-style scanlines over the whole canvas.
fn draw_scanlines(c: &Canvas) {
    let line = Color32::from_black_alpha(40);
    for y in (1..GRID_H).step_by(2) {
        let min = c.origin + vec2(0.0, y as f32 * c.dot);
        let size = vec2(GRID_W as f32 * c.dot, (c.dot * 0.34).max(1.0));
        c.painter
            .rect_filled(Rect::from_min_size(min, size), 0.0, line);
    }
}

fn tooltip(ui: &mut egui::Ui, snap: Option<&Snapshot>, error: Option<&FetchError>, updated: Option<DateTime<Local>>) {
    let line = |ui: &mut egui::Ui, name: &str, w: &LimitWindow| {
        let reset = w
            .resets_at()
            .map(|r| {
                format!(
                    " - resets in {} ({})",
                    until_long(r),
                    r.with_timezone(&Local).format("%a %H:%M")
                )
            })
            .unwrap_or_default();
        ui.label(format!("{name}: {:.0}%{reset}", w.percent()));
    };
    if let Some(s) = snap {
        let u = &s.usage;
        if let Some(plan) = &s.plan {
            ui.strong(format!("Claude {}", plan.to_uppercase()));
        }
        if let Some(w) = &u.five_hour {
            line(ui, "5-hour session", w);
        }
        if let Some(w) = &u.seven_day {
            line(ui, "Weekly (all models)", w);
        }
        if let Some(w) = &u.seven_day_opus {
            line(ui, "Weekly Opus", w);
        }
        if let Some(w) = &u.seven_day_sonnet {
            line(ui, "Weekly Sonnet", w);
        }
        if let Some(b) = &u.seven_day_breakdown {
            let parts: Vec<String> = b
                .rows
                .iter()
                .filter(|r| r.percent > 0.0)
                .map(|r| format!("{} {:.0}%", r.display_name, r.percent))
                .collect();
            if !parts.is_empty() {
                ui.label(format!("This week: {}", parts.join(" / ")));
            }
        }
    }
    if let Some(e) = error {
        ui.colored_label(pal::RED, &e.detail);
    }
    if let Some(at) = updated {
        ui.weak(format!(
            "Updated {} - auto refresh every {REFRESH_SECS}s",
            at.format("%H:%M:%S")
        ));
    }
    ui.weak("Drag to move - right-click for options");
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(pal::BG).to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (snap, error, updated, loading) = {
            let s = self.shared.lock().unwrap();
            (s.snap.clone(), s.error.clone(), s.updated, s.loading)
        };
        let t = self.start.elapsed().as_secs_f32();
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        // Size the window so each dot lands on whole physical pixels.
        let ppp = ctx.pixels_per_point();
        let dot = (DOT * ppp).round().max(1.0) / ppp;
        if (self.sized_for - ppp).abs() > f32::EPSILON {
            self.sized_for = ppp;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
                GRID_W as f32 * dot,
                GRID_H as f32 * dot,
            )));
        }

        let usage = snap.as_ref().map(|s| &s.usage);
        let session = usage.and_then(|u| u.five_hour.clone());
        let weekly = usage.and_then(|u| u.seven_day.clone());

        let mut target_s = session.as_ref().map_or(0.0, |w| w.percent());
        let mut target_w = weekly.as_ref().map_or(0.0, |w| w.percent());
        match self.demo.as_deref() {
            Some("sweep") => target_s = ((t * 5.0) % 115.0).min(100.0),
            Some(v) => target_s = v.parse().unwrap_or(target_s),
            None => {}
        }
        if self.demo.is_some() {
            target_w = target_s * 0.5;
        }

        // Ease the displayed numbers toward the real ones.
        let ease = (dt * 3.0).min(1.0);
        self.shown_session += (target_s - self.shown_session) * ease;
        self.shown_weekly += (target_w - self.shown_weekly) * ease;

        let governing = self.shown_session.max(self.shown_weekly);
        let has_data = snap.is_some() || self.demo.is_some();
        let mood = if !has_data && error.is_some() {
            Mood::Worried
        } else {
            Mood::from_percent(governing)
        };

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(pal::BG))
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let size = vec2(GRID_W as f32 * dot, GRID_H as f32 * dot);
                let snap_px = |v: f32| (v * ppp).round() / ppp;
                let origin = Pos2::new(snap_px(full.min.x), snap_px(full.min.y));
                let rect = Rect::from_min_size(origin, size);

                let resp = ui.interact(rect, Id::new("canvas"), Sense::click_and_drag());
                let canvas = Canvas {
                    painter: ui.painter().clone(),
                    origin,
                    dot,
                };
                let hover = resp.hover_pos().map(|p| canvas.to_dot(p));
                let over = |b| hover.is_some_and(|h| in_box(h, b));

                // Frame pulses gently in the mood colour.
                let pulse = 0.65 + 0.35 * (t * 2.5).sin().abs();
                let accent = mood.accent();
                draw_frame(&canvas, accent.gamma_multiply(pulse));

                mascot::draw(&canvas, 4, 7, mood, t);

                // Title + plan badge.
                canvas.text(INFO_X, 4, "CLAUDE", pal::CYAN);
                if let Some(plan) = snap.as_ref().and_then(|s| s.plan.as_deref()) {
                    let plan = plan.to_uppercase();
                    let bx = INFO_X + text_width("CLAUDE") + 3;
                    canvas.px(bx - 1, 3, text_width(&plan) + 2, 7, pal::MAGENTA);
                    canvas.text(bx, 4, &plan, pal::BG);
                }
                draw_button(&canvas, BTN_REFRESH, "R", over(BTN_REFRESH), pal::CYAN);
                draw_button(&canvas, BTN_CLOSE, "X", over(BTN_CLOSE), pal::RED);

                // Status line: mood, loading, or error (alternating with the mood if we have data).
                let (msg, col) = match (&error, has_data) {
                    (Some(e), false) => (format!("! {}", e.short), pal::RED),
                    (Some(e), true) if (t / 2.0) as i32 % 2 == 1 => (format!("! {}", e.short), pal::RED),
                    (None, false) => {
                        let dots = ".".repeat((t * 3.0) as usize % 4);
                        (format!("> SYNCING{dots}"), pal::CYAN)
                    }
                    // When burnt out, alternate with the time the governing limit comes back.
                    _ if mood == Mood::Dead && (t / 2.0) as i32 % 2 == 1 => {
                        let gov = if self.shown_weekly > self.shown_session {
                            weekly.as_ref().and_then(|w| w.resets_at()).map(|r| reset_clock(r, true))
                        } else {
                            session.as_ref().and_then(|w| w.resets_at()).map(|r| reset_clock(r, false))
                        };
                        match gov {
                            Some(at) => (format!("> BACK {at}"), accent),
                            None => (mood_message(mood).to_string(), accent),
                        }
                    }
                    _ => (mood_message(mood).to_string(), accent),
                };
                let visible = mood != Mood::Panic || (t * 3.0) as i32 % 3 != 0;
                if visible {
                    canvas.text(INFO_X, 13, &msg, col);
                }
                if loading && has_data {
                    // Tiny activity blip next to the refresh button.
                    if (t * 6.0) as i32 % 2 == 0 {
                        canvas.px(BTN_REFRESH.0 - 2, 6, 1, 1, pal::CYAN);
                    }
                }

                draw_limit_row(&canvas, 21, "5H", self.shown_session, session.as_ref(), false, t);
                draw_limit_row(&canvas, 34, "7D", self.shown_weekly, weekly.as_ref(), true, t);

                draw_scanlines(&canvas);

                // Interaction.
                if resp.clicked() {
                    if over(BTN_REFRESH) {
                        let _ = self.refresh_tx.send(());
                    } else if over(BTN_CLOSE) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                if resp.drag_started() && !over(BTN_REFRESH) && !over(BTN_CLOSE) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                resp.context_menu(|ui| {
                    if ui.button("Refresh now").clicked() {
                        let _ = self.refresh_tx.send(());
                        ui.close_menu();
                    }
                    let mut pinned = self.pinned;
                    if ui.checkbox(&mut pinned, "Always on top").changed() {
                        self.set_pinned(ctx, pinned);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                if !over(BTN_REFRESH) && !over(BTN_CLOSE) {
                    resp.on_hover_ui_at_pointer(|ui| {
                        tooltip(ui, snap.as_ref(), error.as_ref(), updated)
                    });
                }
            });

        // ~30 fps for the animation.
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Claude Limit")
            .with_inner_size([GRID_W as f32 * DOT, GRID_H as f32 * DOT])
            .with_decorations(false)
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };
    eframe::run_native(
        "Claude Limit",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
