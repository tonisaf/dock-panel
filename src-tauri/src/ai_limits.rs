//! Subscription rate limits for Claude Code and Codex, read from local files.
//! No credentials are touched.
//!
//! - Claude: Claude Code passes `rate_limits` to its status line command. We
//!   install a tiny status line script that snapshots that object to a JSON
//!   file in our app data dir (see `claude_connect`).
//! - Codex: the CLI logs `rate_limits` on every `token_count` event in its
//!   session files under `~/.codex/sessions`.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const SNAPSHOT_FILE: &str = "claude-limits.json";
const SCRIPT_FILE: &str = "claude-statusline.js";
/// How much of a Codex session log to scan from the end.
const CODEX_TAIL_BYTES: u64 = 512 * 1024;
/// Recently modified session files to read. Codex also writes to old sessions
/// now and then, so the latest limits need not be in the newest file.
const CODEX_FILES_TO_TRY: usize = 8;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    /// `five_hour`, `seven_day`, `spend_limit`, or `window_<minutes>`.
    pub kind: String,
    pub used_percent: f64,
    /// Unix seconds.
    pub resets_at: i64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Unix milliseconds of when the source recorded this data.
    pub updated_at: i64,
    pub plan: Option<String>,
    pub windows: Vec<LimitWindow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiLimits {
    pub claude: Option<Snapshot>,
    pub claude_connected: bool,
    pub claude_web: crate::claude_web::WebStatus,
    pub alerts: crate::alerts::AlertSettings,
    pub codex: Option<Snapshot>,
}

#[tauri::command]
pub async fn ai_limits(app: AppHandle) -> Result<AiLimits, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    crate::claude_web::refresh_if_stale(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let (claude, codex) = snapshots(&app);
        AiLimits {
            claude,
            claude_web: crate::claude_web::status(),
            claude_connected: claude_status_line(&home).is_some_and(|cmd| cmd.contains(SCRIPT_FILE)),
            alerts: crate::alerts::settings(),
            codex,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Current `(claude, codex)` snapshots from every local source.
pub fn snapshots(app: &AppHandle) -> (Option<Snapshot>, Option<Snapshot>) {
    // Signed-in claude.ai data wins over the Claude Code status line snapshot.
    let claude = crate::claude_web::snapshot().or_else(|| {
        let data_dir = app.path().app_data_dir().ok()?;
        read_claude_snapshot(&data_dir.join(SNAPSHOT_FILE))
    });
    let codex = app.path().home_dir().ok().and_then(|home| read_codex(&codex_home(&home)));
    (claude, codex)
}

// ---- Claude ---------------------------------------------------------------------

fn claude_settings_path(home: &Path) -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("settings.json")
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn claude_status_line(home: &Path) -> Option<String> {
    let settings = read_json(&claude_settings_path(home))?;
    Some(settings.get("statusLine")?.get("command")?.as_str()?.to_string())
}

fn read_claude_snapshot(path: &Path) -> Option<Snapshot> {
    let v = read_json(path)?;
    let limits = v.get("rateLimits")?.as_object()?;
    let windows = limits
        .iter()
        .filter_map(|(kind, w)| {
            Some(LimitWindow {
                kind: kind.clone(),
                used_percent: w.get("used_percentage")?.as_f64()?,
                resets_at: w.get("resets_at")?.as_i64()?,
            })
        })
        .collect();
    Some(Snapshot { updated_at: v.get("updatedAt")?.as_i64()?, plan: None, windows })
}

/// Writes the status line script and points Claude Code's `statusLine` at it.
/// Refuses to replace a status line the user configured themselves.
#[tauri::command]
pub async fn claude_connect(app: AppHandle) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;

    if std::process::Command::new("node").arg("--version").output().is_err() {
        return Err("Не найден Node.js: он нужен для скрипта статус-строки Claude Code".into());
    }

    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let snapshot = data_dir.join(SNAPSHOT_FILE);
    let script = data_dir.join(SCRIPT_FILE);
    let snapshot_literal = serde_json::to_string(&snapshot.to_string_lossy()).expect("path serializes");
    fs::write(&script, STATUS_LINE_SCRIPT.replace("__SNAPSHOT_PATH__", &snapshot_literal))
        .map_err(|e| e.to_string())?;

    let settings_path = claude_settings_path(&home);
    let mut settings = read_json(&settings_path).unwrap_or_else(|| json!({}));
    if let Some(existing) = settings.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str) {
        if !existing.contains(SCRIPT_FILE) {
            return Err(format!(
                "У Claude Code уже настроена своя статус-строка ({existing}). Замените её вручную или удалите, чтобы подключить панель."
            ));
        }
    }
    if settings_path.exists() {
        let _ = fs::copy(&settings_path, settings_path.with_extension("json.dock-panel.bak"));
    }
    settings
        .as_object_mut()
        .ok_or("settings.json Claude Code повреждён")?
        .insert(
            "statusLine".into(),
            json!({ "type": "command", "command": format!("node \"{}\"", script.display()) }),
        );
    write_json(&settings_path, &settings)
}

#[tauri::command]
pub async fn claude_disconnect(app: AppHandle) -> Result<(), String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let settings_path = claude_settings_path(&home);
    let Some(mut settings) = read_json(&settings_path) else { return Ok(()) };
    let ours = settings
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(SCRIPT_FILE));
    if ours {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("statusLine");
        }
        write_json(&settings_path, &settings)?;
    }
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, text + "\n").map_err(|e| e.to_string())
}

const STATUS_LINE_SCRIPT: &str = r#"// Dock Panel: Claude Code status line.
// Saves subscription rate limits for the panel's AI widget and prints a
// compact status line. Remove it from the panel's AI tab.
const fs = require("fs");
const OUT = __SNAPSHOT_PATH__;

let raw = "";
process.stdin.on("data", (d) => (raw += d));
process.stdin.on("end", () => {
  let data = {};
  try { data = JSON.parse(raw); } catch {}

  const limits = data.rate_limits;
  if (limits && Object.keys(limits).length) {
    try {
      fs.writeFileSync(OUT + ".tmp", JSON.stringify({ updatedAt: Date.now(), rateLimits: limits }));
      fs.renameSync(OUT + ".tmp", OUT);
    } catch {}
  }

  const pct = (w) => (w && w.used_percentage != null ? Math.round(w.used_percentage) + "%" : null);
  const parts = [];
  if (data.model && data.model.display_name) parts.push(data.model.display_name);
  if (limits) {
    if (pct(limits.five_hour)) parts.push("5ч " + pct(limits.five_hour));
    if (pct(limits.seven_day)) parts.push("неделя " + pct(limits.seven_day));
  }
  const ctx = data.context_window && data.context_window.used_percentage;
  if (ctx != null) parts.push("контекст " + Math.round(ctx) + "%");
  process.stdout.write(parts.join(" · "));
});
"#;

// ---- Codex ----------------------------------------------------------------------

fn codex_home(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"))
}

/// Each file's latest limits by its modification time, so an unchanged log
/// isn't read again on every poll.
type CodexCache = HashMap<PathBuf, (u128, Option<Snapshot>)>;
static CODEX_CACHE: Mutex<Option<CodexCache>> = Mutex::new(None);

/// The most recently recorded limits among the recently modified session
/// files: by the record's own time, not the file's, since touching an old
/// session (its last limits long reset) makes it the newest file.
fn read_codex(codex_home: &Path) -> Option<Snapshot> {
    let mut files = Vec::new();
    collect_jsonl(&codex_home.join("sessions"), &mut files);
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.truncate(CODEX_FILES_TO_TRY);

    let mut guard = CODEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    cache.retain(|path, _| files.iter().any(|(_, p)| p == path));
    let snapshots: Vec<Snapshot> = files
        .iter()
        .filter_map(|(modified, path)| {
            let fresh = cache.get(path).is_some_and(|(m, _)| m == modified);
            if !fresh {
                cache.insert(path.clone(), (*modified, last_codex_limits(path)));
            }
            cache.get(path)?.1.clone()
        })
        .collect();
    latest(snapshots)
}

fn latest(snapshots: impl IntoIterator<Item = Snapshot>) -> Option<Snapshot> {
    snapshots.into_iter().max_by_key(|s| s.updated_at)
}

fn collect_jsonl(dir: &Path, out: &mut Vec<(u128, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_jsonl(&path, out);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis());
            out.push((modified, path));
        }
    }
}

/// Latest `rate_limits` in a session log, scanning only its tail.
fn last_codex_limits(path: &Path) -> Option<Snapshot> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(CODEX_TAIL_BYTES))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let tail = String::from_utf8_lossy(&tail);

    tail.lines().rev().filter(|l| l.contains("\"rate_limits\"")).find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        let limits = v.get("payload")?.get("rate_limits")?;
        let windows: Vec<LimitWindow> = ["primary", "secondary"]
            .iter()
            .filter_map(|key| {
                let w = limits.get(*key)?;
                let minutes = w.get("window_minutes")?.as_i64()?;
                Some(LimitWindow {
                    kind: match minutes {
                        300 => "five_hour".into(),
                        10080 => "seven_day".into(),
                        m => format!("window_{m}"),
                    },
                    used_percent: w.get("used_percent")?.as_f64()?,
                    resets_at: w.get("resets_at")?.as_i64()?,
                })
            })
            .collect();
        if windows.is_empty() {
            return None;
        }
        let updated_at = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
            .unwrap_or(0);
        let plan = limits.get("plan_type").and_then(Value::as_str).map(str::to_string);
        Some(Snapshot { updated_at, plan, windows })
    })
}

/// Minimal parser for Codex's `2026-09-24T12:27:17.879Z` UTC timestamps.
fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    let ms: i64 = format!("{frac:0<3}")[..3].parse().ok()?;

    // Days from civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400) + hh * 3600 + mm * 60 + ss) * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::{latest, parse_rfc3339_ms, Snapshot};

    #[test]
    fn codex_limits_come_from_the_latest_record_not_the_newest_file() {
        let at = |updated_at| Snapshot { updated_at, plan: None, windows: Vec::new() };
        // Files newest first: an old session touched today, then today's.
        assert_eq!(latest([at(100), at(300), at(200)]).map(|s| s.updated_at), Some(300));
        assert!(latest([]).is_none());
    }

    #[test]
    fn parses_codex_timestamps() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_rfc3339_ms("2026-09-24T12:27:17.879Z"), Some(1_790_252_837_879));
    }
}
