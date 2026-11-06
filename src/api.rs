//! Reads the Claude Code OAuth token and queries Anthropic's usage endpoint.

use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;
use std::{env, fs, path::PathBuf, time::Duration};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

#[derive(Deserialize, Clone, Debug)]
pub struct LimitWindow {
    pub utilization: Option<f64>,
    pub resets_at: Option<String>,
}

impl LimitWindow {
    pub fn percent(&self) -> f32 {
        self.utilization.unwrap_or(0.0).clamp(0.0, 100.0) as f32
    }

    pub fn resets_at(&self) -> Option<DateTime<FixedOffset>> {
        self.resets_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct BreakdownRow {
    pub display_name: String,
    pub percent: f64,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Breakdown {
    #[serde(default)]
    pub rows: Vec<BreakdownRow>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Usage {
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
    pub seven_day_opus: Option<LimitWindow>,
    pub seven_day_sonnet: Option<LimitWindow>,
    pub seven_day_breakdown: Option<Breakdown>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub usage: Usage,
    pub plan: Option<String>,
}

/// `short` fits on the pixel display; `detail` goes in the tooltip.
#[derive(Clone, Debug)]
pub struct FetchError {
    pub short: String,
    pub detail: String,
}

impl FetchError {
    fn new(short: &str, detail: impl Into<String>) -> Self {
        Self {
            short: short.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuth {
    access_token: String,
    expires_at: Option<i64>,
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsFile {
    claude_ai_oauth: Option<OAuth>,
}

fn credentials_path() -> Option<PathBuf> {
    if let Ok(dir) = env::var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join(".credentials.json"));
    }
    dirs::home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

/// macOS: Claude Code keeps the credentials JSON in the login Keychain.
/// The first read may show a Keychain prompt; choose "Always Allow".
#[cfg(target_os = "macos")]
fn keychain_credentials() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(not(target_os = "macos"))]
fn keychain_credentials() -> Option<String> {
    None
}

fn read_credentials() -> Result<String, FetchError> {
    // An explicit config dir wins; otherwise prefer the Keychain on macOS.
    if env::var_os("CLAUDE_CONFIG_DIR").is_none() {
        if let Some(raw) = keychain_credentials() {
            return Ok(raw);
        }
    }
    let path = credentials_path()
        .ok_or_else(|| FetchError::new("NO HOME DIR", "Could not locate the home directory"))?;
    fs::read_to_string(&path).map_err(|_| {
        let where_ = if cfg!(target_os = "macos") {
            format!("in the Keychain (\"Claude Code-credentials\") or at {}", path.display())
        } else {
            format!("at {}", path.display())
        };
        FetchError::new(
            "NO LOGIN",
            format!("No credentials {where_} - log in to Claude Code first"),
        )
    })
}

fn load_oauth() -> Result<OAuth, FetchError> {
    let raw = read_credentials()?;
    let file: CredentialsFile = serde_json::from_str(&raw)
        .map_err(|e| FetchError::new("BAD CREDS", format!("Could not parse credentials: {e}")))?;
    let oauth = file.claude_ai_oauth.ok_or_else(|| {
        FetchError::new("NO LOGIN", "Claude Code is not logged in with a claude.ai account")
    })?;

    if let Some(exp_ms) = oauth.expires_at {
        if exp_ms < Utc::now().timestamp_millis() {
            return Err(FetchError::new(
                "TOKEN EXPIRED",
                "OAuth token expired - open Claude Code once so it refreshes the login",
            ));
        }
    }
    Ok(oauth)
}

pub fn fetch() -> Result<Snapshot, FetchError> {
    let oauth = load_oauth()?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("claude-limit/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| FetchError::new("CLIENT ERROR", e.to_string()))?;

    let resp = client
        .get(USAGE_URL)
        .bearer_auth(&oauth.access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .map_err(|e| FetchError::new("OFFLINE", format!("Request failed: {e}")))?;

    match resp.status().as_u16() {
        200 => {}
        401 | 403 => {
            return Err(FetchError::new(
                "AUTH FAILED",
                "Token rejected - reopen Claude Code to refresh the login",
            ))
        }
        429 => {
            return Err(FetchError::new(
                "RATE LIMITED",
                "Too many requests - will retry automatically",
            ))
        }
        code => {
            return Err(FetchError::new(
                &format!("HTTP {code}"),
                format!("Server returned HTTP {code}"),
            ))
        }
    }

    let usage: Usage = resp
        .json()
        .map_err(|e| FetchError::new("BAD RESPONSE", format!("Could not parse usage: {e}")))?;

    Ok(Snapshot {
        usage,
        plan: oauth.subscription_type,
    })
}

fn until(when: DateTime<FixedOffset>) -> (i64, i64, i64) {
    let secs = (when.with_timezone(&Utc) - Utc::now()).num_seconds().max(0);
    (secs / 86_400, (secs % 86_400) / 3_600, (secs % 3_600) / 60)
}

/// Compact form for the pixel display, e.g. "2H39M" or "3D6H".
pub fn until_compact(when: DateTime<FixedOffset>) -> String {
    match until(when) {
        (d, h, _) if d > 0 => format!("{d}D{h}H"),
        (_, h, m) if h > 0 => format!("{h}H{m:02}M"),
        (_, _, m) => format!("{}M", m.max(1)),
    }
}

/// Local wall-clock reset time, e.g. "02:49" or "TUE 06:59".
pub fn reset_clock(when: DateTime<FixedOffset>, with_day: bool) -> String {
    let local = when.with_timezone(&chrono::Local);
    let fmt = if with_day { "%a %H:%M" } else { "%H:%M" };
    local.format(fmt).to_string().to_uppercase()
}

/// Human form for tooltips, e.g. "2h 39m".
pub fn until_long(when: DateTime<FixedOffset>) -> String {
    match until(when) {
        (d, h, _) if d > 0 => format!("{d}d {h}h"),
        (_, h, m) if h > 0 => format!("{h}h {m}m"),
        (_, _, m) => format!("{}m", m.max(1)),
    }
}
