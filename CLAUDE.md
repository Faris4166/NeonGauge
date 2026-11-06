# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

NeonGauge is a single-binary Rust + egui (eframe 0.29) desktop widget that shows Claude usage limits (5-hour session and weekly) as a pixel-art gauge. It reads the Claude Code OAuth token, never writes or refreshes it, and calls the undocumented `GET https://api.anthropic.com/api/oauth/usage` endpoint.

## Commands

```sh
cargo build                      # debug build (has a console window)
cargo run --release              # release build uses windows_subsystem = "windows" (no console, no stdout)
cargo test                       # unit tests live in #[cfg(test)] modules inside src/*.rs
cargo test alerts::tests::hysteresis_prevents_flapping   # single test (name filter)
cargo clippy --all-targets       # keep this warning-free
```

- On Windows without Visual Studio Build Tools, use the GNU toolchain: `cargo +stable-x86_64-pc-windows-gnu build --release` with `C:\msys64\ucrt64\bin` on `PATH`.
- Preview moods without real data: `NEONGAUGE_DEMO=85` pins the session %, `NEONGAUGE_DEMO=sweep` cycles 0 to 100. Demo values only change what is drawn. Alerts and history always use real API data.
- CI (`.github/workflows/rust.yml`) builds and tests on ubuntu, windows and macos. Cross-checking Linux or macOS from Windows fails locally because `ring` needs a C cross-compiler, so CI is the only check for the non-Windows `cfg` paths.

## Architecture

**Threads and shared state** (`main.rs`):
- `spawn_worker` runs the fetch loop on a background thread. Each cycle it calls `api::fetch()`, appends to `History`, runs `alerts::Alerts::check`, plays the refresh and error sounds, and updates the tray tooltip text.
- This work lives on the worker, not in `update()`, on purpose. On Windows, `update()` is not called while the window is hidden in the tray, so anything that must keep running in the background has to be on the worker.
- The UI and worker share `Arc<Mutex<Shared>>` (snapshot, error, timestamps), `Arc<Mutex<Settings>>` and `Arc<Mutex<History>>`.
- A manual refresh is a `()` sent on an mpsc channel. The worker waits in 1-second `recv_timeout` steps and re-reads `refresh_secs` each step, so interval changes apply immediately.
- Settings flow: `App.cfg` is edited directly by the settings UI. `apply_settings()` sanitises it, mirrors it to the worker's copy, applies window-level changes (always-on-top, tray visibility), and saves to disk after a 600 ms debounce.

**Rendering** (`pixel.rs`, `mascot.rs`, the `draw_*` functions in `main.rs`):
- The main window is a fixed 104x48 grid of "dots" drawn with `Canvas::px` and a hand-made 3x5 bitmap font. Every glyph advances 4 dots, and `text_width` gives a string's width.
- Dot size is `Settings::scale`, snapped to whole physical pixels. The window is resized whenever `(pixels_per_point, scale)` changes.
- Button hit-testing uses dot-space boxes (`BTN_*` constants). Hover details are deliberately shown only on the `?` button: the tooltip draws inside the tiny window and would otherwise cover the gauge.
- The window is transparent (`clear_color` returns zero alpha) and opacity is applied with `Painter::set_opacity`.
- Settings and History are separate native windows created with `ctx.show_viewport_immediate`, so they only render while the main window's `update()` runs.

**Platform layer** (`platform.rs`):
- Window show/hide: `WindowCtl` captures the HWND on the first frame and hides/shows through Win32 `ShowWindow`. This works from tray callbacks while egui is not running. Off Windows it falls back to minimise.
- Tray: `tray-icon` is a dependency only on `cfg(any(windows, target_os = "macos"))`, which keeps GTK out of Linux builds. The `TrayIcon` is kept in a main-thread `thread_local`. Its menu and click handlers call `WindowCtl` directly and send `TrayCmd` to the UI. The "Quit NeonGauge" menu item calls `process::exit`.
- Start at login uses `auto-launch` (Windows: HKCU Run key). Login entries pass `--tray` (`TRAY_ARG`), and the app then hides itself on frame 4, after eframe has first shown the window.

**Sound** (`sfx.rs`):
- Sounds are synthesised as square, triangle or noise WAV data in memory. There is no audio crate.
- Windows plays them with `PlaySoundW(SND_MEMORY | SND_ASYNC)`. The playing buffer must stay alive in the static `PLAYING` until playback is stopped.
- macOS and Linux write a temp file and shell out to `afplay`, or to `paplay` / `pw-play` / `aplay`.
- To add a sound: add an `Sfx` variant, a `score()` entry, `label()` and `file_name()` arms, and include it in `Sfx::ALL`.

**Alerts** (`alerts.rs`):
- There is one `Tracker` per limit window. It raises once per level increase (Warn, then Critical) and drops a level only after usage falls `HYSTERESIS` (3 points) below the threshold.
- `Cleared` (a limit reset) plays the reset sound and sends a notification only if `notify_reset` is on.
- Notifications use `notify-rust`.

**Credentials** (`api.rs`):
- The token comes from `$CLAUDE_CONFIG_DIR/.credentials.json`, or `~/.claude/.credentials.json` otherwise.
- On macOS, the Keychain item `Claude Code-credentials` is tried first through the `security` CLI, unless `CLAUDE_CONFIG_DIR` is set.
- Each `FetchError` has a `short` text for the pixel display (keep it to the 3x5 font's characters and short enough to fit) and a `detail` text for tooltips.

**Persistence:**
- Settings: `dirs::config_dir()/NeonGauge/settings.json`. Fields use `#[serde(default)]` so older files still load. `sanitized()` clamps hand-edited values.
- Last good reading: `dirs::data_local_dir()/NeonGauge/last-usage.json` (`api::save_cache` / `load_cache`). At startup the worker shows it right away and, if it is newer than `refresh_secs`, waits out the rest of the interval before the first request, so restarts do not hit the API. On HTTP 429 the worker waits `Retry-After`, or 2, 4, 8 minutes and so on up to 15.
- History: `dirs::data_local_dir()/NeonGauge/history.csv`, one `ts,session,weekly` row per reading. Readings are de-duplicated within 15 minutes, and rows older than 35 days are pruned on load.

## Gotchas

- The pixel font only has `0-9`, `A-Z` and `% : . ! ? - + / >` plus space. Any other character is silently skipped, and all text is uppercased.
- The plan badge is truncated to 3 characters (`PLAN_MAX_CHARS`) so it does not overlap the `?` button.
- The `Dockerfile` still references the binary as `claude-limit`, but the crate builds `neongauge`.
- Icons are not drawn in code: `assets/icon.png` (window, 256 px), `assets/icon-tray.png` (64 px) and `assets/icon.ico` (exe, embedded by `build.rs` through `assets/neongauge.rc`) are renders of `assets/logo.svg`. Re-render all three when the logo changes.
- `installer/Product.wxs` (WiX v4 MSI) and `dist/` exist locally but are gitignored.
