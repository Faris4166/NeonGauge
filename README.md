# Claude Limit

A tiny always-on-top pixel-art tab (Rust + egui) that shows your Claude usage limits in real time.
A 16x16 pixel mascot reacts to how much of your limit you have used.

| Usage | Mood | Mascot |
|---|---|---|
| 0-29% | `ALL CHILL` | happy `^ ^` eyes, slow bob |
| 30-59% | `CRUISING` | glowing cyan eyes, hops and waves |
| 60-79% | `EASE UP...` | worried brows, sweat drop |
| 80-94% | `DANGER ZONE!` | turns hot pink, wide eyes, shakes and flails |
| 95-100% | `BURNT OUT` | greyed out, `X X` eyes, floating Zzz |

The mood follows whichever limit is higher: the 5-hour session or the weekly limit.

## Features

- Segmented neon bars for the **5-hour session** (`5H`) and **weekly** (`7D`) limits, with reset countdowns
- Hover for details: Opus/Sonnet weekly limits, per-product breakdown, exact reset times
- Auto refresh every 60 s; `R` refreshes now, `X` closes
- Drag anywhere to move; right-click for *Refresh now*, *Always on top* and *Quit*
- Everything is drawn as pixels: a custom 3x5 bitmap font, pixel-snapped dots, CRT scanlines

## How it works

The app reads the OAuth token Claude Code stores in `~/.claude/.credentials.json`
(or `$CLAUDE_CONFIG_DIR/.credentials.json`) and calls `GET https://api.anthropic.com/api/oauth/usage`.
It only reads the token. It never modifies or refreshes it.

- You must be logged in to Claude Code with a claude.ai (Pro/Max) account. If the token expires, open Claude Code once to refresh it.
- The usage endpoint is not a documented public API and may change.
- On macOS, Claude Code keeps the token in the Keychain, so this version supports Windows and Linux only.

## Build and run

```sh
cargo run --release
```

On Windows without Visual Studio Build Tools (`link.exe not found`), use the GNU toolchain with MSYS2's gcc:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
$env:PATH = "C:\msys64\ucrt64\bin;$env:PATH"
cargo +stable-x86_64-pc-windows-gnu build --release
.\target\release\claude-limit.exe
```

## Environment variables

| Variable | Effect |
|---|---|
| `CLAUDE_LIMIT_DEMO=85` | Pin the session percentage to preview a mood |
| `CLAUDE_LIMIT_DEMO=sweep` | Cycle the percentage 0 to 100 |
| `CLAUDE_CONFIG_DIR` | Claude Code config directory if it is not `~/.claude` |

## Layout

- `src/api.rs`: credentials, usage API, reset-time formatting
- `src/pixel.rs`: dot canvas, neon palette, 3x5 bitmap font
- `src/mascot.rs`: sprite, mood faces and frame-based animation
- `src/main.rs`: window, background fetch thread, bars, interaction
