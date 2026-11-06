//! 8-bit sound effects, synthesised in code (square / triangle / noise) and
//! played without any audio crate:
//! - Windows: `PlaySoundW` from an in-memory WAV
//! - macOS: `afplay` on a temp file
//! - Linux: `paplay`, `pw-play` or `aplay` on a temp file

#[cfg(windows)]
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Startup,
    Click,
    Toggle,
    Open,
    Refresh,
    Warn,
    Critical,
    Reset,
    Error,
}

impl Sfx {
    pub const ALL: [Sfx; 9] = [
        Sfx::Startup,
        Sfx::Click,
        Sfx::Toggle,
        Sfx::Open,
        Sfx::Refresh,
        Sfx::Warn,
        Sfx::Critical,
        Sfx::Reset,
        Sfx::Error,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Sfx::Startup => "Startup",
            Sfx::Click => "Click",
            Sfx::Toggle => "Toggle",
            Sfx::Open => "Open window",
            Sfx::Refresh => "Refresh done",
            Sfx::Warn => "Warning alert",
            Sfx::Critical => "Critical alert",
            Sfx::Reset => "Limit reset",
            Sfx::Error => "Error",
        }
    }

    #[cfg_attr(windows, allow(dead_code))]
    fn file_name(self) -> &'static str {
        match self {
            Sfx::Startup => "startup",
            Sfx::Click => "click",
            Sfx::Toggle => "toggle",
            Sfx::Open => "open",
            Sfx::Refresh => "refresh",
            Sfx::Warn => "warn",
            Sfx::Critical => "critical",
            Sfx::Reset => "reset",
            Sfx::Error => "error",
        }
    }
}

const RATE: u32 = 22_050;

#[derive(Clone, Copy)]
enum Wave {
    /// Square wave with the given duty cycle.
    Square(f32),
    Triangle,
    Noise,
}

/// One note: pitch glides from `f0` to `f1` Hz over `ms`. `f0 == 0` is a rest.
#[derive(Clone, Copy)]
struct Note {
    f0: f32,
    f1: f32,
    ms: u32,
    wave: Wave,
    vol: f32,
}

const fn n(f: f32, ms: u32, wave: Wave) -> Note {
    Note { f0: f, f1: f, ms, wave, vol: 1.0 }
}
const fn slide(f0: f32, f1: f32, ms: u32, wave: Wave) -> Note {
    Note { f0, f1, ms, wave, vol: 1.0 }
}
const fn rest(ms: u32) -> Note {
    Note { f0: 0.0, f1: 0.0, ms, wave: Wave::Square(0.5), vol: 0.0 }
}

const SQ12: Wave = Wave::Square(0.125);
const SQ25: Wave = Wave::Square(0.25);
const SQ50: Wave = Wave::Square(0.5);
const TRI: Wave = Wave::Triangle;

// Note frequencies.
const C5: f32 = 523.25;
const E5: f32 = 659.25;
const G5: f32 = 783.99;
const A5: f32 = 880.0;
const B5: f32 = 987.77;
const C6: f32 = 1046.5;
const E6: f32 = 1318.5;
const G6: f32 = 1568.0;

fn score(s: Sfx) -> Vec<Note> {
    match s {
        Sfx::Startup => vec![
            n(C5, 55, SQ25),
            n(E5, 55, SQ25),
            n(G5, 55, SQ25),
            n(C6, 70, SQ25),
            rest(25),
            n(G6, 160, TRI),
        ],
        Sfx::Click => vec![n(1400.0, 14, SQ25), n(900.0, 12, SQ25)],
        Sfx::Toggle => vec![n(A5, 28, SQ50), n(E6, 40, SQ50)],
        Sfx::Open => vec![n(C5, 35, TRI), n(E5, 35, TRI), n(G5, 35, TRI), n(C6, 60, TRI)],
        Sfx::Refresh => vec![n(B5, 45, SQ12), n(E6, 110, SQ12)],
        Sfx::Warn => vec![n(A5, 120, SQ50), rest(60), n(A5, 120, SQ50), rest(40)],
        Sfx::Critical => {
            let mut v = Vec::new();
            for _ in 0..3 {
                v.push(n(E6, 85, SQ50));
                v.push(n(B5, 85, SQ50));
            }
            v.push(n(E6, 220, SQ50));
            v
        }
        Sfx::Reset => vec![
            slide(260.0, 1200.0, 260, SQ25),
            rest(20),
            n(C6, 60, SQ25),
            n(G6, 160, SQ25),
        ],
        Sfx::Error => vec![
            // Noise ignores pitch; any non-zero f0 just marks it as audible.
            Note { vol: 0.5, ..n(1.0, 50, Wave::Noise) },
            slide(420.0, 110.0, 280, SQ50),
        ],
    }
}

/// Render notes to 16-bit mono PCM.
fn render(notes: &[Note], volume: f32) -> Vec<i16> {
    // Square waves are loud; keep the ceiling well below full scale.
    let master = volume.clamp(0.0, 1.0) * 0.32;
    let mut out = Vec::new();
    let mut phase = 0.0f32;
    let mut noise: u32 = 0x1234_5678;
    for note in notes {
        let len = (RATE * note.ms / 1000) as usize;
        let attack = (RATE as usize / 500).min(len / 4).max(1); // ~2 ms
        let release = (len * 3 / 10).max(1);
        for i in 0..len {
            if note.f0 <= 0.0 || note.vol <= 0.0 {
                out.push(0);
                continue;
            }
            let u = i as f32 / len as f32;
            let f = note.f0 + (note.f1 - note.f0) * u;
            phase = (phase + f / RATE as f32).fract();
            let raw = match note.wave {
                Wave::Square(duty) => {
                    if phase < duty {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Noise => {
                    noise ^= noise << 13;
                    noise ^= noise >> 17;
                    noise ^= noise << 5;
                    (noise as f32 / u32::MAX as f32) * 2.0 - 1.0
                }
            };
            let env = if i < attack {
                i as f32 / attack as f32
            } else if i > len - release {
                (len - i) as f32 / release as f32
            } else {
                1.0
            };
            out.push((raw * env * note.vol * master * i16::MAX as f32) as i16);
        }
    }
    out
}

fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

pub fn wav_bytes(s: Sfx, volume: f32) -> Vec<u8> {
    wav(&render(&score(s), volume))
}

/// Keeps the buffer PlaySoundW is reading alive; replaced only after stopping it.
#[cfg(windows)]
static PLAYING: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Play a sound without blocking. A new sound cuts off the previous one.
pub fn play(s: Sfx, volume: f32) {
    if volume <= 0.0 {
        return;
    }
    let bytes = wav_bytes(s, volume);
    play_bytes(s, bytes);
}

#[cfg(windows)]
fn play_bytes(_s: Sfx, bytes: Vec<u8>) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    let mut guard = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: stop playback before freeing the old buffer; the new buffer lives
    // in the static until the next call stops it again.
    unsafe {
        PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
        *guard = bytes;
        PlaySoundW(
            guard.as_ptr() as *const u16,
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(not(windows))]
fn play_bytes(s: Sfx, bytes: Vec<u8>) {
    use std::process::{Command, Stdio};
    std::thread::spawn(move || {
        let path = std::env::temp_dir().join(format!("neongauge-{}.wav", s.file_name()));
        if std::fs::write(&path, bytes).is_err() {
            return;
        }
        let players: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
            &[("afplay", &[])]
        } else {
            &[("paplay", &[]), ("pw-play", &[]), ("aplay", &["-q"])]
        };
        for (cmd, args) in players {
            let ok = Command::new(cmd)
                .args(*args)
                .arg(&path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|st| st.success());
            if ok {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_renders_a_valid_wav() {
        for s in Sfx::ALL {
            let b = wav_bytes(s, 1.0);
            assert_eq!(&b[0..4], b"RIFF", "{s:?}");
            assert!(b.len() > 44 + 200, "{s:?} is too short");
            let _ = s.file_name();
        }
    }

    #[test]
    fn zero_volume_is_silent() {
        let pcm = render(&score(Sfx::Critical), 0.0);
        assert!(pcm.iter().all(|&s| s == 0));
    }
}
