//! Claude Code and Codex sessions on this PC: which are running, which just
//! finished and are waiting for the user, and a way back to their window.
//!
//! Read-only, from the agents' own files; their settings and hooks stay untouched.
//! - Claude Code keeps `~/.claude/sessions/<pid>.json` for every live session,
//!   with its title, folder, host (`entrypoint`) and a `busy`/`idle` status.
//! - Codex writes a rollout log per thread under `~/.codex/sessions/Y/M/D`:
//!   `session_meta` first, then `task_started` / `task_complete` per turn (the
//!   latter with the agent's last message). Titles are in `session_index.jsonl`.
//!
//! A session that goes from busy to anything else is "waiting": it gets a
//! toast (optional) and counts on the taskbar button until it works again, the
//! user opens it from the panel, or dismisses it.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

const POLL: Duration = Duration::from_secs(2);
/// Codex threads with no activity for this long drop off the list.
const CODEX_RECENT: Duration = Duration::from_secs(6 * 3600);
/// A Codex turn silent for this long is treated as over (the app may have died mid-turn).
const CODEX_STALE_TURN: Duration = Duration::from_secs(15 * 60);
const SETTINGS_FILE: &str = "agents.json";
/// Tells the windows the list changed; they poll no more.
const CHANGED_EVENT: &str = "agents:changed";

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    /// `claude:<session id>` or `codex:<thread id>`.
    id: String,
    kind: &'static str,
    name: String,
    /// Project folder name.
    project: String,
    /// Where it runs: "Claude", "Codex", "VS Code", "терминал".
    host: String,
    busy: bool,
    /// Finished (or stopped to ask) and not looked at yet.
    waiting: bool,
    /// Unix ms of the last status change.
    since: u64,
    /// The agent's last message, when the log has it (Codex).
    last_message: Option<String>,
    /// What a working session is doing: its latest tool call, or that it waits for an answer.
    activity: Option<String>,
    #[serde(skip)]
    pid: Option<u32>,
    #[serde(skip)]
    host_exe: Option<&'static str>,
}

#[derive(Default)]
struct Tracker {
    agents: Vec<Agent>,
    waiting: HashSet<String>,
    /// Busy state from the previous poll, to see transitions.
    was_busy: HashMap<String, bool>,
    first_poll_done: bool,
    codex_files: HashMap<PathBuf, CodexFile>,
    codex_titles: (Option<SystemTime>, HashMap<String, String>),
}

#[derive(Default)]
struct CodexFile {
    offset: u64,
    id: Option<String>,
    cwd: String,
    host: String,
    busy: bool,
    since: u64,
    last_message: Option<String>,
    activity: Option<String>,
}

static TRACKER: Mutex<Option<Tracker>> = Mutex::new(None);
static NOTIFY: AtomicBool = AtomicBool::new(true);
static APP: OnceLock<AppHandle> = OnceLock::new();

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

fn folder_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

// ---- Claude Code --------------------------------------------------------------

fn claude_sessions() -> Vec<Agent> {
    let Ok(dir) = std::fs::read_dir(home().join(".claude").join("sessions")) else { return Vec::new() };
    dir.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str::<Value>(&std::fs::read_to_string(e.path()).ok()?).ok())
        // Only sessions a person works in; `claude -p` runs (like the panel's quick questions) aren't.
        .filter(|v| v["kind"].as_str().is_none_or(|k| k == "interactive"))
        .filter_map(|v| {
            let pid = v["pid"].as_u64()? as u32;
            if !process::alive(pid) {
                return None;
            }
            let session = v["sessionId"].as_str()?;
            let cwd = v["cwd"].as_str().unwrap_or_default();
            let (host, host_exe) = match v["entrypoint"].as_str().unwrap_or_default() {
                "claude-desktop" => ("Claude", Some("claude.exe")),
                e if e.contains("vscode") => ("VS Code", Some("code.exe")),
                e if e.contains("jetbrains") || e.contains("ide") => ("IDE", None),
                _ => ("терминал", None),
            };
            let busy = v["status"] == "busy";
            Some(Agent {
                id: format!("claude:{session}"),
                kind: "claude",
                name: v["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("Claude Code").to_string(),
                project: folder_name(cwd),
                host: host.to_string(),
                busy,
                waiting: false,
                since: v["statusUpdatedAt"].as_u64().or(v["updatedAt"].as_u64()).unwrap_or(0),
                last_message: None,
                activity: if busy { claude_activity(session, cwd) } else { None },
                pid: Some(pid),
                host_exe,
            })
        })
        .collect()
}

/// Shown for a session stopped on a question to the user.
const ASKING: &str = "ждёт вашего ответа";

/// One line for a tool call: `Bash: cargo test`, `Edit: agents.rs`.
fn describe_tool(name: &str, input: &Value) -> String {
    let text = |key: &str| input[key].as_str().filter(|s| !s.is_empty());
    let file = |key: &str| text(key).map(|p| folder_name(&p.replace('\\', "/")));
    let detail = match name {
        "Bash" | "PowerShell" => text("description").or_else(|| text("command")).map(str::to_string),
        "Read" | "Edit" | "Write" | "MultiEdit" => file("file_path"),
        "NotebookEdit" => file("notebook_path"),
        "Grep" | "Glob" => text("pattern").map(str::to_string),
        "WebFetch" => text("url").map(str::to_string),
        "WebSearch" => text("query").map(str::to_string),
        "Agent" | "Task" => text("description").map(str::to_string),
        "Skill" => text("skill").map(str::to_string),
        _ => None,
    };
    // MCP tools are `mcp__server__tool`; the tool is what matters.
    let name = name.rsplit("__").next().unwrap_or(name);
    match detail {
        Some(d) => format!("{name}: {}", preview(&d, 70)),
        None => name.to_string(),
    }
}

/// Codex names its tools `exec`, `js`, `wait`…; their arguments are code, so
/// only a title (when it gives one) is worth showing.
fn codex_activity(name: &str, arguments: &Value) -> String {
    match name {
        "request_user_input_async" | "request_user_input" => ASKING.to_string(),
        "exec" | "exec_command" | "shell" | "local_shell" => "выполняет команды".to_string(),
        "wait" => "ждёт завершения команды".to_string(),
        "apply_patch" => "правит файлы".to_string(),
        "js" => arguments
            .as_str()
            .and_then(|a| serde_json::from_str::<Value>(a).ok())
            .and_then(|a| a["title"].as_str().map(|t| preview(t, 70)))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "выполняет код".to_string()),
        other => other.to_string(),
    }
}

// ---- Claude Code: what a session is doing ------------------------------------

/// How much of a transcript's end to look at for the latest tool call.
const TRANSCRIPT_TAIL: u64 = 256 * 1024;

#[derive(Default)]
struct TranscriptCache {
    path: Option<PathBuf>,
    len: u64,
    activity: Option<String>,
}

static TRANSCRIPTS: Mutex<Option<HashMap<String, TranscriptCache>>> = Mutex::new(None);

/// `~/.claude/projects/<folder>/<session>.jsonl`, the folder being the cwd with
/// everything but letters and digits turned into dashes (found by search when
/// that guess misses, e.g. for non-Latin paths).
fn find_transcript(session: &str, cwd: &str) -> Option<PathBuf> {
    let projects = home().join(".claude").join("projects");
    let file = format!("{session}.jsonl");
    let slug: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let guess = projects.join(slug).join(&file);
    if guess.is_file() {
        return Some(guess);
    }
    std::fs::read_dir(projects).ok()?.flatten().map(|d| d.path().join(&file)).find(|p| p.is_file())
}

fn claude_activity(session: &str, cwd: &str) -> Option<String> {
    let mut guard = TRANSCRIPTS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if cache.len() > 64 {
        cache.clear();
    }
    let entry = cache.entry(session.to_string()).or_default();
    if entry.path.is_none() {
        entry.path = find_transcript(session, cwd);
    }
    let path = entry.path.clone()?;
    let len = std::fs::metadata(&path).ok()?.len();
    if len != entry.len {
        entry.activity = transcript_activity(&path, len);
        entry.len = len;
    }
    entry.activity.clone()
}

/// The latest tool call near the end of a transcript, or that it waits for the
/// user (a question or a plan to approve that no result has followed yet).
fn transcript_activity(path: &Path, len: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let start = len.saturating_sub(TRANSCRIPT_TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // cut mid-line
    }
    // (tool_use id, name, input, answered)
    let mut last: Option<(String, String, Value, bool)> = None;
    for line in lines {
        if line.contains("\"tool_use\"") {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let call = v["message"]["content"].as_array().and_then(|c| c.iter().rev().find(|b| b["type"] == "tool_use"));
            if let Some(b) = call {
                last = Some((
                    b["id"].as_str().unwrap_or_default().to_string(),
                    b["name"].as_str().unwrap_or_default().to_string(),
                    b["input"].clone(),
                    false,
                ));
            }
        } else if let Some((id, _, _, answered)) = last.as_mut() {
            if !*answered && !id.is_empty() && line.contains("\"tool_result\"") && line.contains(id.as_str()) {
                *answered = true;
            }
        }
    }
    let (_, name, input, answered) = last?;
    Some(match (name.as_str(), answered) {
        ("AskUserQuestion", false) => ASKING.to_string(),
        ("ExitPlanMode", false) => "ждёт одобрения плана".to_string(),
        _ => describe_tool(&name, &input),
    })
}

// ---- Codex --------------------------------------------------------------------

/// Session day folders for today and the two days before (the logs are dated in UTC).
fn codex_day_dirs() -> Vec<PathBuf> {
    let root = home().join(".codex").join("sessions");
    let today = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 86_400;
    (0..3)
        .map(|back| {
            let (y, m, d) = civil_from_days(today as i64 - back);
            root.join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{d:02}"))
        })
        .collect()
}

/// Days since 1970-01-01 → (year, month, day); Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn rfc3339_ms(ts: &str) -> Option<u64> {
    // "2026-09-26T21:18:23.506Z"
    let (date, time) = ts.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let time = time.trim_end_matches('Z');
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    // Days from civil, the inverse of `civil_from_days`.
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms: i64 = frac.chars().take(3).collect::<String>().parse::<i64>().unwrap_or(0)
        * 10_i64.pow(3 - frac.len().min(3) as u32);
    Some((((days * 24 + hh) * 60 + mm) * 60 + ss) as u64 * 1000 + ms as u64)
}

/// Reads what was appended to a rollout log since last time.
fn read_codex_file(path: &Path, f: &mut CodexFile) {
    let Ok(mut file) = std::fs::File::open(path) else { return };
    let Ok(len) = file.metadata().map(|m| m.len()) else { return };
    if len < f.offset {
        *f = CodexFile::default();
    }
    if len == f.offset || file.seek(SeekFrom::Start(f.offset)).is_err() {
        return;
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return;
    }
    // Only whole lines; a half-written last line is read next time.
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return };
    f.offset += end as u64 + 1;
    for line in buf[..end].split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(line);
        let kind = if line.contains("\"session_meta\"") {
            "meta"
        } else if line.contains("\"task_started\"") {
            "started"
        } else if line.contains("\"task_complete\"") {
            "complete"
        } else if line.contains("\"function_call\"") || line.contains("\"custom_tool_call\"") {
            "call"
        } else if line.contains("\"function_call_output\"") {
            "output"
        } else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let p = &v["payload"];
        let at = v["timestamp"].as_str().and_then(rfc3339_ms).unwrap_or(0);
        match kind {
            "meta" => {
                f.id = p["id"].as_str().map(str::to_string);
                f.cwd = p["cwd"].as_str().unwrap_or_default().to_string();
                f.host = match (p["originator"].as_str().unwrap_or_default(), p["source"].as_str().unwrap_or_default()) {
                    (o, _) if o.contains("Desktop") => "Codex",
                    (_, "vscode") => "VS Code",
                    _ => "терминал",
                }
                .to_string();
                f.since = at;
            }
            "started" if p["type"] == "task_started" => {
                f.busy = true;
                f.since = at;
                f.activity = None;
            }
            "call" if matches!(p["type"].as_str(), Some("function_call" | "custom_tool_call")) => {
                f.activity = Some(codex_activity(p["name"].as_str().unwrap_or_default(), &p["arguments"]));
            }
            // The answer to a question came back; what happens next is unknown until the next call.
            "output" if p["type"] == "function_call_output" && f.activity.as_deref() == Some(ASKING) => {
                f.activity = None;
            }
            "complete" if p["type"] == "task_complete" => {
                f.busy = false;
                f.since = at;
                f.activity = None;
                f.last_message = p["last_agent_message"].as_str().map(|m| preview(m, 240));
            }
            _ => {}
        }
    }
}

fn codex_titles(t: &mut Tracker) {
    let path = home().join(".codex").join("session_index.jsonl");
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if modified.is_none() || modified == t.codex_titles.0 {
        return;
    }
    let titles = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| Some((v["id"].as_str()?.to_string(), v["thread_name"].as_str()?.to_string())))
        .collect();
    t.codex_titles = (modified, titles);
}

fn codex_sessions(t: &mut Tracker) -> Vec<Agent> {
    codex_titles(t);
    let now = SystemTime::now();
    let recent: Vec<PathBuf> = codex_day_dirs()
        .into_iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|d| d.flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .is_ok_and(|m| now.duration_since(m).unwrap_or_default() < CODEX_RECENT)
        })
        .collect();
    t.codex_files.retain(|p, _| recent.contains(p));
    let mut out = Vec::new();
    for path in recent {
        let f = t.codex_files.entry(path.clone()).or_default();
        read_codex_file(&path, f);
        let Some(id) = f.id.clone() else { continue };
        let silent = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|m| now.duration_since(m).unwrap_or_default())
            .unwrap_or_default();
        out.push(Agent {
            id: format!("codex:{id}"),
            kind: "codex",
            name: t.codex_titles.1.get(&id).cloned().unwrap_or_else(|| "Codex".into()),
            project: folder_name(&f.cwd),
            host: f.host.clone(),
            busy: f.busy && silent < CODEX_STALE_TURN,
            waiting: false,
            since: f.since,
            last_message: f.last_message.clone(),
            activity: f.activity.clone().filter(|_| f.busy && silent < CODEX_STALE_TURN),
            pid: None,
            host_exe: match f.host.as_str() {
                // The Codex desktop app (package OpenAI.Codex) runs as ChatGPT.exe.
                "Codex" => Some("chatgpt.exe"),
                "VS Code" => Some("code.exe"),
                _ => None,
            },
        });
    }
    out
}

// ---- polling ------------------------------------------------------------------

fn poll(app: &AppHandle) {
    let mut guard = TRACKER.lock().unwrap_or_else(|e| e.into_inner());
    let t = guard.get_or_insert_with(Tracker::default);
    let mut agents = claude_sessions();
    agents.extend(codex_sessions(t));

    let mut finished = Vec::new();
    for a in &agents {
        let was = t.was_busy.insert(a.id.clone(), a.busy);
        if a.busy {
            t.waiting.remove(&a.id);
        } else if t.first_poll_done && was == Some(true) {
            t.waiting.insert(a.id.clone());
            finished.push(a.clone());
        }
    }
    let live: HashSet<&String> = agents.iter().map(|a| &a.id).collect();
    t.waiting.retain(|id| live.contains(id));
    t.was_busy.retain(|id, _| live.contains(id));
    for a in &mut agents {
        a.waiting = t.waiting.contains(&a.id);
    }
    // Busy first, then waiting, then the rest; newest first within each.
    agents.sort_by_key(|a| (!a.busy, !a.waiting, std::cmp::Reverse(a.since)));
    let changed = t.agents != agents || !t.first_poll_done;
    t.agents = agents;
    t.first_poll_done = true;
    drop(guard);
    if changed {
        let _ = app.emit(CHANGED_EVENT, ());
    }

    if NOTIFY.load(Ordering::SeqCst) {
        for a in finished {
            notify(app, &a);
        }
    }
}

fn notify(app: &AppHandle, a: &Agent) {
    let who = if a.kind == "claude" { "Claude" } else { "Codex" };
    let title = format!("{who} закончил · {}", a.project);
    let body = match &a.last_message {
        Some(m) => format!("{}\n{}", a.name, preview(m, 140)),
        None => a.name.clone(),
    };
    #[cfg(windows)]
    {
        let id = a.id.clone();
        let open = move || {
            agents_dismiss(Some(id.clone()));
            focus(&id);
        };
        crate::alerts::notify_then(app, &title, &body, Some(Box::new(open)));
    }
    #[cfg(not(windows))]
    let _ = (app, title, body);
}

/// Sessions waiting for the user, for the taskbar button.
pub fn waiting_count() -> usize {
    TRACKER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(0, |t| t.waiting.len())
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(SETTINGS_FILE))
}

/// Starts the background poller.
pub fn init(app: &AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    let saved = settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_default();
    NOTIFY.store(saved["notify"].as_bool().unwrap_or(true), Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || loop {
        poll(&app);
        std::thread::sleep(POLL);
    });
}

#[derive(Serialize)]
pub struct AgentsState {
    agents: Vec<Agent>,
    notify: bool,
}

#[tauri::command]
pub fn agents_list() -> AgentsState {
    let agents = TRACKER.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|t| t.agents.clone()).unwrap_or_default();
    AgentsState { agents, notify: NOTIFY.load(Ordering::SeqCst) }
}

/// Clears "waiting" for one session, or for all with `None`.
#[tauri::command]
pub fn agents_dismiss(id: Option<String>) {
    if let Some(t) = TRACKER.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        match &id {
            Some(id) => {
                t.waiting.remove(id);
            }
            None => t.waiting.clear(),
        }
        for a in &mut t.agents {
            a.waiting = t.waiting.contains(&a.id);
        }
    }
    changed();
}

/// For changes made here rather than found by the poller.
fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(CHANGED_EVENT, ());
    }
}

#[tauri::command]
pub fn agents_set_notify(app: AppHandle, on: bool) {
    NOTIFY.store(on, Ordering::SeqCst);
    if let Some(p) = settings_path(&app) {
        let _ = std::fs::write(p, serde_json::json!({ "notify": on }).to_string());
    }
    changed();
}

/// Brings the session's window to the front and marks it seen.
#[tauri::command]
pub fn agents_focus(app: AppHandle, id: String) -> Result<(), String> {
    agents_dismiss(Some(id.clone()));
    crate::panel::request_hide(&app);
    if focus(&id) { Ok(()) } else { Err("Не нашёл окно этой сессии".into()) }
}

fn focus(id: &str) -> bool {
    let target = TRACKER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|t| t.agents.iter().find(|a| a.id == id).map(|a| (a.pid, a.host_exe)));
    let Some((pid, exe)) = target else { return false };
    #[cfg(windows)]
    {
        process::focus(pid, exe)
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, exe);
        false
    }
}

// ---- processes and windows ----------------------------------------------------

#[cfg(windows)]
mod process {
    use std::collections::HashMap;

    use windows::core::BOOL;
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, STILL_ACTIVE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
    };

    pub fn alive(pid: u32) -> bool {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
            let _ = CloseHandle(h);
            ok
        }
    }

    /// pid → (lowercase exe name, parent pid).
    fn processes() -> HashMap<u32, (String, u32)> {
        let mut out = HashMap::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut more = Process32FirstW(snap, &mut e).is_ok();
            while more {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
                out.insert(e.th32ProcessID, (name, e.th32ParentProcessID));
                more = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    /// Visible, titled, unowned top-level windows by process id.
    fn app_windows() -> HashMap<u32, HWND> {
        unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
            let map = unsafe { &mut *(data.0 as *mut HashMap<u32, HWND>) };
            unsafe {
                if IsWindowVisible(hwnd).as_bool()
                    && GetWindowTextLengthW(hwnd) > 0
                    && GetWindow(hwnd, GW_OWNER).map_or(true, |o| o.is_invalid())
                {
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    map.entry(pid).or_insert(hwnd);
                }
            }
            true.into()
        }
        let mut map = HashMap::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut map as *mut _ as isize));
        }
        map
    }

    /// The window of `exe` (an app hosting sessions), or else the nearest
    /// ancestor of `pid` that has a window (the terminal a CLI session runs in).
    pub fn focus(pid: Option<u32>, exe: Option<&str>) -> bool {
        let procs = processes();
        let windows = app_windows();
        let by_exe = exe.and_then(|exe| {
            windows.iter().find(|(p, _)| procs.get(p).is_some_and(|(name, _)| name == exe)).map(|(_, &h)| h)
        });
        let by_parent = || {
            let mut cur = pid?;
            for _ in 0..16 {
                if let Some(&h) = windows.get(&cur) {
                    return Some(h);
                }
                cur = procs.get(&cur)?.1;
            }
            None
        };
        let Some(hwnd) = by_exe.or_else(by_parent) else { return false };
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd).as_bool()
        }
    }
}

#[cfg(not(windows))]
mod process {
    pub fn alive(_pid: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lists the sessions on this machine: `cargo test -- --ignored real_agents --nocapture`.
    #[test]
    #[ignore]
    fn real_agents() {
        let mut t = Tracker::default();
        let mut all = claude_sessions();
        all.extend(codex_sessions(&mut t));
        for a in all {
            println!("{} | {} | busy={} | activity={:?}", a.kind, a.name, a.busy, a.activity);
        }
    }

    #[test]
    fn describes_claude_tool_calls() {
        let d = |name, input: Value| describe_tool(name, &input);
        assert_eq!(d("Bash", serde_json::json!({"command": "cargo test", "description": "Run tests"})), "Bash: Run tests");
        assert_eq!(d("Bash", serde_json::json!({"command": "cargo test"})), "Bash: cargo test");
        assert_eq!(d("Edit", serde_json::json!({"file_path": "C:\\a\\b\\agents.rs"})), "Edit: agents.rs");
        assert_eq!(d("Grep", serde_json::json!({"pattern": "fn poll"})), "Grep: fn poll");
        assert_eq!(d("mcp__srv__do_thing", serde_json::json!({})), "do_thing");
        assert_eq!(d("Read", serde_json::json!({})), "Read");
    }

    #[test]
    fn reads_claude_activity_from_a_transcript() {
        let dir = std::env::temp_dir().join(format!("agents-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let call = |id: &str, name: &str, input: &str| {
            format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}]}}}}"#)
        };
        let result = |id: &str| format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}"}}]}}}}"#);
        let activity = |lines: &[String]| {
            std::fs::write(&path, lines.join("\n") + "\n").unwrap();
            transcript_activity(&path, std::fs::metadata(&path).unwrap().len())
        };

        let read = call("t1", "Read", r#"{"file_path":"/x/lib.rs"}"#);
        assert_eq!(activity(&[read.clone()]).as_deref(), Some("Read: lib.rs"));
        assert_eq!(activity(&[read.clone(), result("t1")]).as_deref(), Some("Read: lib.rs"));

        let ask = call("t2", "AskUserQuestion", "{}");
        assert_eq!(activity(&[read.clone(), result("t1"), ask.clone()]).as_deref(), Some(ASKING));
        // Once answered it is just the latest tool again.
        assert_eq!(activity(&[ask.clone(), result("t2")]).as_deref(), Some("AskUserQuestion"));
        assert_eq!(activity(&[call("t3", "ExitPlanMode", "{}")]).as_deref(), Some("ждёт одобрения плана"));
        assert_eq!(activity(&[r#"{"type":"user"}"#.to_string()]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracks_codex_activity() {
        let dir = std::env::temp_dir().join(format!("agents-codex-act-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        let lines = [
            r#"{"timestamp":"2026-09-26T21:00:00.000Z","type":"event_msg","payload":{"type":"task_started"}}"#,
            r#"{"timestamp":"2026-09-26T21:00:01.000Z","type":"response_item","payload":{"type":"custom_tool_call","name":"exec","input":"1+1"}}"#,
            r#"{"timestamp":"2026-09-26T21:00:02.000Z","type":"response_item","payload":{"type":"function_call","name":"request_user_input_async","arguments":"{}"}}"#,
        ];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        let mut f = CodexFile::default();
        read_codex_file(&path, &mut f);
        assert_eq!(f.activity.as_deref(), Some(ASKING));

        let more = r#"{"timestamp":"2026-09-26T21:00:03.000Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c"}}"#;
        std::fs::write(&path, lines.join("\n") + "\n" + more + "\n").unwrap();
        read_codex_file(&path, &mut f);
        assert_eq!(f.activity, None);

        let done = r#"{"timestamp":"2026-09-26T21:00:04.000Z","type":"event_msg","payload":{"type":"task_complete","last_agent_message":"ok"}}"#;
        std::fs::write(&path, lines.join("\n") + "\n" + more + "\n" + done + "\n").unwrap();
        read_codex_file(&path, &mut f);
        assert!(!f.busy);
        assert_eq!(f.activity, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_log_timestamps() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:01.500Z"), Some(1500));
        assert_eq!(rfc3339_ms("2026-09-26T21:18:23.506Z"), Some(1_790_457_503_506));
    }

    #[test]
    fn days_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_722), (2026, 9, 26));
    }

    #[test]
    fn reads_codex_turns_incrementally() {
        let dir = std::env::temp_dir().join(format!("agents-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        let meta = r#"{"timestamp":"2026-09-26T21:00:00.000Z","type":"session_meta","payload":{"id":"t1","cwd":"D:\\WickFlow","originator":"Codex Desktop","source":"vscode"}}"#;
        let started = r#"{"timestamp":"2026-09-26T21:01:00.000Z","type":"event_msg","payload":{"type":"task_started"}}"#;
        std::fs::write(&path, format!("{meta}\n{started}\n")).unwrap();
        let mut f = CodexFile::default();
        read_codex_file(&path, &mut f);
        assert_eq!(f.id.as_deref(), Some("t1"));
        assert_eq!(f.host, "Codex");
        assert!(f.busy);

        let complete = r#"{"timestamp":"2026-09-26T21:02:00.000Z","type":"event_msg","payload":{"type":"task_complete","last_agent_message":"PR   открыт,\nCI прошёл"}}"#;
        let mut all = std::fs::read_to_string(&path).unwrap();
        all.push_str(complete);
        all.push('\n');
        std::fs::write(&path, all).unwrap();
        read_codex_file(&path, &mut f);
        assert!(!f.busy);
        assert_eq!(f.last_message.as_deref(), Some("PR открыт, CI прошёл"));
        std::fs::remove_dir_all(dir).ok();
    }
}
