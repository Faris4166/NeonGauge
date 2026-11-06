//! OS integration: start at login, show/hide the window, system tray.

use eframe::egui;
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Arc,
};

/// Command-line flag the login entry passes so the app starts hidden.
pub const TRAY_ARG: &str = "--tray";

// ---------- start at login ----------

fn launcher(hidden: bool) -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_str().ok_or("executable path is not valid UTF-8")?;
    let args: &[&str] = if hidden { &[TRAY_ARG] } else { &[] };
    let mut b = auto_launch::AutoLaunchBuilder::new();
    b.set_app_name("NeonGauge").set_app_path(exe).set_args(args);
    #[cfg(target_os = "macos")]
    b.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    #[cfg(windows)]
    b.set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser);
    b.build().map_err(|e| e.to_string())
}

pub fn autostart_enabled() -> bool {
    launcher(false).and_then(|l| l.is_enabled().map_err(|e| e.to_string())).unwrap_or(false)
}

/// Turn start-at-login on or off. Re-registers so `hidden` takes effect.
pub fn set_autostart(enabled: bool, hidden: bool) -> Result<(), String> {
    let l = launcher(hidden)?;
    if l.is_enabled().unwrap_or(false) {
        l.disable().map_err(|e| e.to_string())?;
    }
    if enabled {
        l.enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- window visibility ----------

/// Native window handle (HWND on Windows), filled in on the first frame.
#[derive(Clone, Default)]
pub struct WindowCtl {
    pub hwnd: Arc<AtomicIsize>,
    pub visible: Arc<AtomicBool>,
}

impl WindowCtl {
    pub fn new() -> Self {
        let c = Self::default();
        c.visible.store(true, Ordering::Relaxed);
        c
    }

    pub fn capture(&self, frame: &eframe::Frame) {
        #[cfg(windows)]
        if self.hwnd.load(Ordering::Relaxed) == 0 {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = frame.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    self.hwnd.store(w.hwnd.get(), Ordering::Relaxed);
                }
            }
        }
        #[cfg(not(windows))]
        let _ = frame;
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Relaxed)
    }

    /// Windows hides the window completely (it lives on in the tray).
    /// Elsewhere the window is minimised, since a hidden winit window may
    /// stop receiving events.
    pub fn hide(&self, ctx: &egui::Context) {
        self.visible.store(false, Ordering::Relaxed);
        #[cfg(windows)]
        {
            let hwnd = self.hwnd.load(Ordering::Relaxed);
            if hwnd != 0 {
                use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
                // SAFETY: hwnd is our own top-level window.
                unsafe { ShowWindow(hwnd as _, SW_HIDE) };
                return;
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }

    /// Safe to call from any thread (tray callbacks).
    pub fn show(&self, ctx: &egui::Context) {
        self.visible.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        {
            let hwnd = self.hwnd.load(Ordering::Relaxed);
            if hwnd != 0 {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    SetForegroundWindow, ShowWindow, SW_SHOW,
                };
                // SAFETY: hwnd is our own top-level window.
                unsafe {
                    ShowWindow(hwnd as _, SW_SHOW);
                    SetForegroundWindow(hwnd as _);
                }
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
}

// ---------- tray ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayCmd {
    Refresh,
    History,
    Settings,
}

pub const TRAY_SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(any(windows, target_os = "macos"))]
mod tray_impl {
    use super::{TrayCmd, WindowCtl};
    use eframe::egui;
    use std::{
        cell::RefCell,
        sync::{mpsc, Arc, Mutex},
    };
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    thread_local! {
        static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    }

    pub fn create(
        ctx: egui::Context,
        win: WindowCtl,
        tx: mpsc::Sender<TrayCmd>,
        tooltip: Arc<Mutex<String>>,
    ) -> Result<(), String> {
        let show = MenuItem::new("Show / hide", true, None);
        let refresh = MenuItem::new("Refresh now", true, None);
        let history = MenuItem::new("History...", true, None);
        let settings = MenuItem::new("Settings...", true, None);
        let quit = MenuItem::new("Quit NeonGauge", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &show,
            &refresh,
            &PredefinedMenuItem::separator(),
            &history,
            &settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;

        let rgba = crate::icon::tray();
        let icon = tray_icon::Icon::from_rgba(rgba.rgba, rgba.width, rgba.height)
            .map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("NeonGauge")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        TRAY.with(|t| *t.borrow_mut() = Some(tray));

        let ids = (
            show.id().clone(),
            refresh.id().clone(),
            history.id().clone(),
            settings.id().clone(),
            quit.id().clone(),
        );
        {
            let (ctx, win, tx) = (ctx.clone(), win.clone(), tx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let id = &e.id;
                if *id == ids.0 {
                    toggle(&ctx, &win);
                } else if *id == ids.4 {
                    std::process::exit(0);
                } else {
                    let cmd = if *id == ids.1 {
                        TrayCmd::Refresh
                    } else if *id == ids.2 {
                        TrayCmd::History
                    } else if *id == ids.3 {
                        TrayCmd::Settings
                    } else {
                        return;
                    };
                    // History and settings windows need the main window running.
                    if cmd != TrayCmd::Refresh {
                        win.show(&ctx);
                    }
                    let _ = tx.send(cmd);
                    ctx.request_repaint();
                }
            }));
        }
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| match e {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => toggle(&ctx, &win),
            TrayIconEvent::Enter { .. } => {
                let text = tooltip.lock().map(|t| t.clone()).unwrap_or_default();
                set_tooltip(&text);
            }
            _ => {}
        }));
        Ok(())
    }

    fn toggle(ctx: &egui::Context, win: &WindowCtl) {
        if win.is_visible() {
            win.hide(ctx);
        } else {
            win.show(ctx);
        }
    }

    pub fn set_visible(visible: bool) {
        TRAY.with(|t| {
            if let Some(t) = t.borrow().as_ref() {
                let _ = t.set_visible(visible);
            }
        });
    }

    /// Only has an effect on the thread that created the tray (the UI thread).
    pub fn set_tooltip(text: &str) {
        TRAY.with(|t| {
            if let Some(t) = t.borrow().as_ref() {
                let _ = t.set_tooltip(Some(text));
            }
        });
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub use tray_impl::{create as create_tray, set_tooltip as set_tray_tooltip};

#[cfg(not(any(windows, target_os = "macos")))]
pub fn create_tray(
    _ctx: egui::Context,
    _win: WindowCtl,
    _tx: std::sync::mpsc::Sender<TrayCmd>,
    _tooltip: Arc<std::sync::Mutex<String>>,
) -> Result<(), String> {
    Err("the system tray is not supported on this platform yet".into())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_tray_tooltip(_text: &str) {}

#[cfg(any(windows, target_os = "macos"))]
pub fn set_tray_visible(visible: bool) {
    tray_impl::set_visible(visible);
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_tray_visible(_visible: bool) {}
