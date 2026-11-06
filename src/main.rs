#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod api;
mod history;
mod icon;
mod mascot;
mod pixel;
mod platform;
mod settings;
mod sfx;

use api::{reset_clock, until_compact, until_long, FetchError, LimitWindow, Snapshot};
use chrono::{DateTime, Local};
use eframe::egui::{self, vec2, Color32, Id, Pos2, Rect, Sense, ViewportBuilder, ViewportId};
use history::History;
use mascot::Mood;
use pixel::{pal, text_width, Canvas};
use platform::{TrayCmd, WindowCtl};
use settings::{Settings, REFRESH_CHOICES};
use sfx::Sfx;
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// Canvas size in dots. The size of one dot comes from `Settings::scale`.
const GRID_W: i32 = 104;
const GRID_H: i32 = 48;

/// Clickable buttons in dot coordinates: (x, y, w, h).
const BTN_INFO: (i32, i32, i32, i32) = (81, 3, 5, 7);
const BTN_REFRESH: (i32, i32, i32, i32) = (88, 3, 5, 7);
const BTN_CLOSE: (i32, i32, i32, i32) = (95, 3, 5, 7);
const BUTTONS: [(i32, i32, i32, i32); 3] = [BTN_INFO, BTN_REFRESH, BTN_CLOSE];

/// Layout of the info column.
const INFO_X: i32 = 41;
const BAR_X: i32 = 49;
const BAR_SEGMENTS: i32 = 16;
const PCT_RIGHT: i32 = 99;
/// The plan badge must end before the "?" button.
const PLAN_MAX_CHARS: usize = 3;

/// Settings are written to disk once they have been stable this long
/// (sliders would otherwise write on every frame).
const SAVE_DEBOUNCE: Duration = Duration::from_millis(600);

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
    /// What the worker thread reads; mirrors `cfg`.
    shared_cfg: Arc<Mutex<Settings>>,
    /// Edited by the settings window.
    cfg: Settings,
    saved_cfg: Settings,
    cfg_changed_at: Option<Instant>,
    save_error: Option<String>,
    history: Arc<Mutex<History>>,
    history_view: history::View,

    win: WindowCtl,
    tray_rx: mpsc::Receiver<TrayCmd>,
    tray_ok: bool,
    tray_error: Option<String>,
    tray_tip: Arc<Mutex<String>>,
    tray_tip_shown: String,
    tray_shown: bool,
    /// Launched by the login entry with `--tray`: hide after the first frames.
    start_hidden: bool,
    frame_no: u64,

    autostart: bool,
    autostart_error: Option<String>,

    show_settings: bool,
    show_history: bool,

    start: Instant,
    shown_session: f32,
    shown_weekly: f32,
    applied_on_top: bool,
    /// NEONGAUGE_DEMO=<number> pins the session percentage, =sweep cycles 0..100.
    demo: Option<String>,
    /// (pixels_per_point, scale) the window was last sized for.
    sized_for: (f32, f32),
}

fn tray_text(snap: &Snapshot) -> String {
    let row = |name: &str, w: &Option<LimitWindow>, with_day: bool| {
        w.as_ref().map(|w| {
            let reset = w
                .resets_at()
                .map(|r| format!(" - resets {}", reset_clock(r, with_day)))
                .unwrap_or_default();
            format!("\n{name} {:.0}%{reset}", w.percent())
        })
    };
    let mut s = String::from("NeonGauge");
    s += &row("5H", &snap.usage.five_hour, false).unwrap_or_default();
    s += &row("7D", &snap.usage.seven_day, true).unwrap_or_default();
    s
}

/// Fetch loop. Also records history, raises alerts and plays the refresh /
/// error sounds, so all of that keeps working while the window is hidden.
fn spawn_worker(
    shared: Arc<Mutex<Shared>>,
    cfg: Arc<Mutex<Settings>>,
    history: Arc<Mutex<History>>,
    tray_tip: Arc<Mutex<String>>,
    ctx: egui::Context,
) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut alerts = alerts::Alerts::default();
        let mut manual = false;
        let mut had_error = false;
        loop {
            shared.lock().unwrap().loading = true;
            ctx.request_repaint();

            let result = api::fetch();
            let s = cfg.lock().unwrap().clone();
            match &result {
                Ok(snap) => {
                    let u = &snap.usage;
                    if u.five_hour.is_some() || u.seven_day.is_some() {
                        history.lock().unwrap().record(history::Sample {
                            ts: Local::now().timestamp(),
                            session: u.five_hour.as_ref().map_or(0.0, |w| w.percent()),
                            weekly: u.seven_day.as_ref().map_or(0.0, |w| w.percent()),
                        });
                    }
                    alerts.check(u.five_hour.as_ref(), u.seven_day.as_ref(), &s);
                    if manual && s.ui_sfx {
                        sfx::play(Sfx::Refresh, s.volume);
                    }
                    *tray_tip.lock().unwrap() = tray_text(snap);
                    had_error = false;
                }
                Err(_) => {
                    if (!had_error || manual) && s.ui_sfx {
                        sfx::play(Sfx::Error, s.volume);
                    }
                    had_error = true;
                }
            }
            {
                let mut sh = shared.lock().unwrap();
                sh.loading = false;
                match result {
                    Ok(snap) => {
                        sh.snap = Some(snap);
                        sh.error = None;
                        sh.updated = Some(Local::now());
                    }
                    Err(e) => sh.error = Some(e),
                }
            }
            ctx.request_repaint();

            // Wait for the interval (re-read every second so changes apply
            // right away) or for a manual refresh.
            let waited = Instant::now();
            manual = loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(()) => break true,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let every = cfg.lock().unwrap().refresh_secs;
                        if waited.elapsed() >= Duration::from_secs(every) {
                            break false;
                        }
                    }
                }
            };
            while rx.try_recv().is_ok() {}
        }
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

fn neon_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = pal::PANEL;
    v.window_fill = pal::PANEL;
    v.extreme_bg_color = pal::BG;
    v.faint_bg_color = pal::BUTTON;
    v.hyperlink_color = pal::CYAN;
    v.selection.bg_fill = pal::CYAN.gamma_multiply(0.4);
    v.selection.stroke.color = pal::CYAN;
    v.widgets.inactive.bg_fill = pal::BUTTON;
    v.widgets.inactive.weak_bg_fill = pal::BUTTON;
    v.widgets.hovered.weak_bg_fill = pal::EMPTY;
    v.widgets.hovered.bg_fill = pal::EMPTY;
    v.widgets.active.weak_bg_fill = pal::CYAN.gamma_multiply(0.35);
    v
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, cfg: Settings, start_hidden: bool) -> Self {
        // Crisp pixel edges.
        cc.egui_ctx
            .options_mut(|o| o.tessellation_options.feathering = false);
        cc.egui_ctx.set_visuals(neon_visuals());

        let shared = Arc::new(Mutex::new(Shared::default()));
        let shared_cfg = Arc::new(Mutex::new(cfg.clone()));
        let history = Arc::new(Mutex::new(History::load()));
        let tray_tip = Arc::new(Mutex::new("NeonGauge".to_string()));
        let refresh_tx = spawn_worker(
            shared.clone(),
            shared_cfg.clone(),
            history.clone(),
            tray_tip.clone(),
            cc.egui_ctx.clone(),
        );

        let win = WindowCtl::new();
        let (tray_tx, tray_rx) = mpsc::channel();
        let (tray_ok, tray_error) = if platform::TRAY_SUPPORTED {
            match platform::create_tray(cc.egui_ctx.clone(), win.clone(), tray_tx, tray_tip.clone()) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            }
        } else {
            (false, None)
        };
        if tray_ok {
            platform::set_tray_visible(cfg.tray);
        }

        if cfg.ui_sfx && !start_hidden {
            sfx::play(Sfx::Startup, cfg.volume);
        }

        Self {
            shared,
            refresh_tx,
            shared_cfg,
            saved_cfg: cfg.clone(),
            applied_on_top: cfg.always_on_top,
            tray_shown: cfg.tray,
            cfg,
            cfg_changed_at: None,
            save_error: None,
            history,
            history_view: history::View::Day,
            win,
            tray_rx,
            tray_ok,
            tray_error,
            tray_tip,
            tray_tip_shown: String::new(),
            start_hidden,
            frame_no: 0,
            autostart: platform::autostart_enabled(),
            autostart_error: None,
            show_settings: false,
            show_history: false,
            start: Instant::now(),
            shown_session: 0.0,
            shown_weekly: 0.0,
            demo: std::env::var("NEONGAUGE_DEMO")
                .or_else(|_| std::env::var("CLAUDE_LIMIT_DEMO"))
                .ok(),
            sized_for: (0.0, 0.0),
        }
    }

    fn sfx(&self, s: Sfx) {
        if self.cfg.ui_sfx {
            sfx::play(s, self.cfg.volume);
        }
    }

    fn use_tray(&self) -> bool {
        self.tray_ok && self.cfg.tray
    }

    fn open_history(&mut self) {
        if !self.show_history {
            self.sfx(Sfx::Open);
        }
        self.show_history = true;
    }

    fn open_settings(&mut self) {
        if !self.show_settings {
            self.sfx(Sfx::Open);
            self.autostart = platform::autostart_enabled();
        }
        self.show_settings = true;
    }

    fn refresh(&self) {
        let _ = self.refresh_tx.send(());
    }

    /// The X button: hide to the tray when there is one, else quit.
    fn close_or_hide(&mut self, ctx: &egui::Context) {
        if self.use_tray() {
            self.show_settings = false;
            self.show_history = false;
            self.win.hide(ctx);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Push edited settings to the worker, apply window changes, save to disk.
    fn apply_settings(&mut self, ctx: &egui::Context) {
        self.cfg = std::mem::take(&mut self.cfg).sanitized();
        {
            let mut shared = self.shared_cfg.lock().unwrap();
            if *shared != self.cfg {
                *shared = self.cfg.clone();
                self.cfg_changed_at = Some(Instant::now());
            }
        }
        if self.applied_on_top != self.cfg.always_on_top {
            self.applied_on_top = self.cfg.always_on_top;
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.cfg.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
        }
        if self.tray_ok && self.tray_shown != self.cfg.tray {
            self.tray_shown = self.cfg.tray;
            platform::set_tray_visible(self.cfg.tray);
        }
        if let Some(at) = self.cfg_changed_at {
            if at.elapsed() >= SAVE_DEBOUNCE && self.cfg != self.saved_cfg {
                self.save_error = self.cfg.save().err();
                self.saved_cfg = self.cfg.clone();
                self.cfg_changed_at = None;
            } else {
                ctx.request_repaint_after(SAVE_DEBOUNCE);
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let toggled = std::cell::Cell::new(false);
        let check = |ui: &mut egui::Ui, v: &mut bool, text: &str| {
            if ui.checkbox(v, text).changed() {
                toggled.set(true);
            }
        };
        let heading = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).strong().color(pal::CYAN));
            ui.separator();
        };

        heading(ui, "DISPLAY");
        egui::Grid::new("display").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Refresh every");
            let label = |s: u64| if s < 60 { format!("{s} seconds") } else { format!("{} min", s / 60) };
            egui::ComboBox::from_id_salt("refresh")
                .selected_text(label(self.cfg.refresh_secs))
                .show_ui(ui, |ui| {
                    for s in REFRESH_CHOICES {
                        ui.selectable_value(&mut self.cfg.refresh_secs, s, label(s));
                    }
                });
            ui.end_row();
            ui.label("Window size");
            ui.add(egui::Slider::new(&mut self.cfg.scale, 2.0..=6.0).step_by(1.0).suffix("x"));
            ui.end_row();
            ui.label("Opacity");
            let mut pct = self.cfg.opacity * 100.0;
            if ui.add(egui::Slider::new(&mut pct, 30.0..=100.0).step_by(5.0).suffix("%")).changed() {
                self.cfg.opacity = pct / 100.0;
            }
            ui.end_row();
        });
        check(ui, &mut self.cfg.always_on_top, "Always on top");

        heading(ui, "ALERTS");
        check(ui, &mut self.cfg.alerts_enabled, "Alert me when usage gets high");
        ui.add_enabled_ui(self.cfg.alerts_enabled, |ui| {
            egui::Grid::new("alerts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.colored_label(pal::YELLOW, "Warning at");
                ui.add(egui::Slider::new(&mut self.cfg.warn_pct, 10.0..=99.0).step_by(1.0).suffix("%"));
                ui.end_row();
                ui.colored_label(pal::RED, "Critical at");
                let lo = (self.cfg.warn_pct + 1.0).min(100.0);
                ui.add(egui::Slider::new(&mut self.cfg.crit_pct, lo..=100.0).step_by(1.0).suffix("%"));
                ui.end_row();
            });
            check(ui, &mut self.cfg.alert_sound, "Play an alert sound");
            check(ui, &mut self.cfg.alert_notify, "Show a desktop notification");
            check(ui, &mut self.cfg.notify_reset, "Also notify when a limit resets");
            ui.horizontal(|ui| {
                if ui.button("Test warning").clicked() {
                    sfx::play(Sfx::Warn, self.cfg.volume);
                    if self.cfg.alert_notify {
                        let pct = self.cfg.warn_pct;
                        thread::spawn(move || {
                            alerts::notify_desktop(&format!("5-hour session limit at {pct:.0}%"), "This is a test alert from NeonGauge")
                        });
                    }
                }
                if ui.button("Test critical").clicked() {
                    sfx::play(Sfx::Critical, self.cfg.volume);
                    if self.cfg.alert_notify {
                        let pct = self.cfg.crit_pct;
                        thread::spawn(move || {
                            alerts::notify_desktop(&format!("5-hour session limit at {pct:.0}% - almost out"), "This is a test alert from NeonGauge")
                        });
                    }
                }
            });
        });

        heading(ui, "SOUND");
        check(ui, &mut self.cfg.ui_sfx, "Interface sounds (clicks, refresh, windows)");
        ui.horizontal(|ui| {
            ui.label("Volume");
            let mut pct = self.cfg.volume * 100.0;
            let r = ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).step_by(5.0).suffix("%"));
            if r.changed() {
                self.cfg.volume = pct / 100.0;
            }
            if r.drag_stopped() {
                sfx::play(Sfx::Click, self.cfg.volume);
            }
        });
        ui.label(egui::RichText::new("Preview").small().weak());
        ui.horizontal_wrapped(|ui| {
            for s in Sfx::ALL {
                if ui.small_button(s.label()).clicked() {
                    sfx::play(s, self.cfg.volume);
                }
            }
        });

        heading(ui, "SYSTEM");
        let login_label = if cfg!(windows) { "Start with Windows" } else { "Start at login" };
        let mut auto = self.autostart;
        if ui.checkbox(&mut auto, login_label).changed() {
            toggled.set(true);
            match platform::set_autostart(auto, self.cfg.start_hidden) {
                Ok(()) => {
                    self.autostart = auto;
                    self.autostart_error = None;
                }
                Err(e) => self.autostart_error = Some(e),
            }
        }
        ui.add_enabled_ui(self.tray_ok, |ui| {
            check(ui, &mut self.cfg.tray, "Show in the system tray (X hides to the tray)");
            let before = self.cfg.start_hidden;
            check(ui, &mut self.cfg.start_hidden, "When started at login, start hidden in the tray");
            if before != self.cfg.start_hidden && self.autostart {
                self.autostart_error = platform::set_autostart(true, self.cfg.start_hidden).err();
            }
        });
        if !platform::TRAY_SUPPORTED {
            ui.weak("The system tray is not supported on this platform yet.");
        }
        if let Some(e) = self.tray_error.as_ref().or(self.autostart_error.as_ref()) {
            ui.colored_label(pal::RED, e);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Reset to defaults").clicked() {
                self.cfg = Settings::default();
                toggled.set(true);
            }
        });
        if let Some(p) = settings::path() {
            ui.weak(format!("Saved to {}", p.display()));
        }
        if let Some(e) = &self.save_error {
            ui.colored_label(pal::RED, format!("Could not save settings: {e}"));
        }

        if toggled.get() {
            self.sfx(Sfx::Toggle);
        }
    }

    fn show_child_windows(&mut self, ctx: &egui::Context, snap: Option<&Snapshot>, error: Option<&FetchError>, updated: Option<DateTime<Local>>) {
        if self.show_settings {
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("neongauge-settings"),
                ViewportBuilder::default()
                    .with_title("NeonGauge settings")
                    .with_icon(icon::icon())
                    .with_inner_size([380.0, 560.0])
                    .with_min_inner_size([320.0, 300.0]),
                |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| self.settings_ui(ui));
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.show_settings = false;
                    }
                },
            );
        }
        if self.show_history {
            let (warn, crit) = (self.cfg.warn_pct, self.cfg.crit_pct);
            let refresh = self.cfg.refresh_secs;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("neongauge-history"),
                ViewportBuilder::default()
                    .with_title("NeonGauge details & history")
                    .with_icon(icon::icon())
                    .with_inner_size([560.0, 480.0])
                    .with_min_inner_size([420.0, 360.0]),
                |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.label(egui::RichText::new("NOW").strong().color(pal::CYAN));
                            ui.separator();
                            details(ui, snap, error, updated, refresh);
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("HISTORY").strong().color(pal::CYAN));
                            ui.separator();
                            let mut h = self.history.lock().unwrap();
                            history::ui(ui, &mut h, &mut self.history_view, warn, crit);
                        });
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.show_history = false;
                    }
                },
            );
        }
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

/// Full readout of the current limits; used by the "?" tooltip and the History window.
fn details(
    ui: &mut egui::Ui,
    snap: Option<&Snapshot>,
    error: Option<&FetchError>,
    updated: Option<DateTime<Local>>,
    refresh_secs: u64,
) {
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
        let every = if refresh_secs < 60 {
            format!("{refresh_secs}s")
        } else {
            format!("{} min", refresh_secs / 60)
        };
        ui.weak(format!("Updated {} - auto refresh every {every}", at.format("%H:%M:%S")));
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Transparent so the opacity setting can show the desktop through.
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.frame_no += 1;
        self.win.capture(frame);

        // Started at login: tuck away into the tray once the window exists.
        // (eframe shows the window after the first frame, so wait a few.)
        if self.start_hidden && self.frame_no == 4 {
            self.start_hidden = false;
            if self.use_tray() {
                self.win.hide(ctx);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }

        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::Refresh => self.refresh(),
                TrayCmd::History => self.open_history(),
                TrayCmd::Settings => self.open_settings(),
            }
        }

        let (snap, error, updated, loading) = {
            let s = self.shared.lock().unwrap();
            (s.snap.clone(), s.error.clone(), s.updated, s.loading)
        };
        let t = self.start.elapsed().as_secs_f32();
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        // Size the window so each dot lands on whole physical pixels.
        let ppp = ctx.pixels_per_point();
        let scale = self.cfg.scale;
        let dot = (scale * ppp).round().max(1.0) / ppp;
        if self.sized_for != (ppp, scale) {
            self.sized_for = (ppp, scale);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
                GRID_W as f32 * dot,
                GRID_H as f32 * dot,
            )));
        }

        let tip = self.tray_tip.lock().unwrap().clone();
        if tip != self.tray_tip_shown {
            platform::set_tray_tooltip(&tip);
            self.tray_tip_shown = tip;
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

        let mut open_history = false;
        let mut open_settings = false;
        let mut hide = false;

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let size = vec2(GRID_W as f32 * dot, GRID_H as f32 * dot);
                let snap_px = |v: f32| (v * ppp).round() / ppp;
                let origin = Pos2::new(snap_px(full.min.x), snap_px(full.min.y));
                let rect = Rect::from_min_size(origin, size);

                let resp = ui.interact(rect, Id::new("canvas"), Sense::click_and_drag());
                let mut painter = ui.painter().clone();
                painter.set_opacity(self.cfg.opacity);
                let canvas = Canvas { painter, origin, dot };
                let hover = resp.hover_pos().map(|p| canvas.to_dot(p));
                let over = |b| hover.is_some_and(|h| in_box(h, b));
                let over_any = BUTTONS.iter().any(|&b| over(b));

                // Frame pulses gently in the mood colour.
                let pulse = 0.65 + 0.35 * (t * 2.5).sin().abs();
                let accent = mood.accent();
                draw_frame(&canvas, accent.gamma_multiply(pulse));

                mascot::draw(&canvas, 4, 7, mood, t);

                // Title + plan badge (trimmed so it never runs into the buttons).
                canvas.text(INFO_X, 4, "CLAUDE", pal::CYAN);
                if let Some(plan) = snap.as_ref().and_then(|s| s.plan.as_deref()) {
                    let plan: String = plan.to_uppercase().chars().take(PLAN_MAX_CHARS).collect();
                    let bx = INFO_X + text_width("CLAUDE") + 3;
                    canvas.px(bx - 1, 3, text_width(&plan) + 2, 7, pal::MAGENTA);
                    canvas.text(bx, 4, &plan, pal::BG);
                }
                draw_button(&canvas, BTN_INFO, "?", over(BTN_INFO), pal::YELLOW);
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
                    // Tiny activity blip left of the buttons.
                    if (t * 6.0) as i32 % 2 == 0 {
                        canvas.px(BTN_INFO.0 - 2, 6, 1, 1, pal::CYAN);
                    }
                }

                draw_limit_row(&canvas, 21, "5H", self.shown_session, session.as_ref(), false, t);
                draw_limit_row(&canvas, 34, "7D", self.shown_weekly, weekly.as_ref(), true, t);

                draw_scanlines(&canvas);

                // Interaction.
                if resp.clicked() {
                    if over(BTN_INFO) {
                        self.sfx(Sfx::Click);
                        open_history = true;
                    } else if over(BTN_REFRESH) {
                        self.sfx(Sfx::Click);
                        self.refresh();
                    } else if over(BTN_CLOSE) {
                        self.sfx(Sfx::Click);
                        hide = true;
                    }
                }
                if resp.drag_started() && !over_any {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let use_tray = self.use_tray();
                resp.context_menu(|ui| {
                    if ui.button("Refresh now").clicked() {
                        self.refresh();
                        ui.close_menu();
                    }
                    if ui.button("Details & history...").clicked() {
                        open_history = true;
                        ui.close_menu();
                    }
                    if ui.button("Settings...").clicked() {
                        open_settings = true;
                        ui.close_menu();
                    }
                    if ui.checkbox(&mut self.cfg.always_on_top, "Always on top").changed() {
                        self.sfx(Sfx::Toggle);
                        ui.close_menu();
                    }
                    ui.separator();
                    if use_tray && ui.button("Hide to tray").clicked() {
                        hide = true;
                        ui.close_menu();
                    }
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                // Details only on the "?" button, so the gauge itself is never covered.
                if over(BTN_INFO) {
                    let refresh = self.cfg.refresh_secs;
                    resp.on_hover_ui_at_pointer(|ui| {
                        details(ui, snap.as_ref(), error.as_ref(), updated, refresh);
                        ui.weak("Click for history - right-click the gauge for options");
                    });
                }
            });

        if open_history {
            self.open_history();
        }
        if open_settings {
            self.open_settings();
        }
        if hide {
            self.close_or_hide(ctx);
        }

        self.show_child_windows(ctx, snap.as_ref(), error.as_ref(), updated);
        self.apply_settings(ctx);

        // ~30 fps for the animation.
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

fn main() -> eframe::Result<()> {
    let cfg = Settings::load();
    let start_hidden = std::env::args().any(|a| a == platform::TRAY_ARG);
    let mut viewport = ViewportBuilder::default()
        .with_title("NeonGauge")
        .with_icon(icon::icon())
        .with_inner_size([GRID_W as f32 * cfg.scale, GRID_H as f32 * cfg.scale])
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true);
    if cfg.always_on_top {
        viewport = viewport.with_always_on_top();
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "NeonGauge",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, cfg, start_hidden)))),
    )
}
