//! YouTube videos in a small always-on-top mpv window instead of the browser:
//! no ads (mpv plays the streams yt-dlp finds, not YouTube's player), and the
//! panel controls it through mpv's JSON IPC pipe.
//!
//! - A click in the YouTube widget plays the video; a second one replaces it
//!   in the same window.
//! - The video counts as watched at 90 %, not on the click.
//! - The window's place and size are remembered (`player.json`).
//! - yt-dlp updates itself at most once a day, since YouTube keeps changing.
//! - The taskbar button's player shows what mpv plays and pauses it.
//!
//! mpv and yt-dlp are separate programs the user installs (winget:
//! `shinchiro.mpv`, `yt-dlp.yt-dlp`); without them the widget opens the browser.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

const PIPE: &str = r"\\.\pipe\dock-panel-mpv";
const SETTINGS_FILE: &str = "player.json";
const WATCHED_AT: f64 = 90.0;
const YTDLP_UPDATE_EVERY: u64 = 24 * 3600;
/// Default window: 480×270 in the bottom-right corner, clear of the taskbar.
const DEFAULT_GEOMETRY: &str = "480x270-24-72";

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    /// `WxH+X+Y` of the window when it last closed.
    geometry: Option<String>,
    /// Unix seconds of the last `yt-dlp -U`.
    ytdlp_updated: u64,
}

struct Session {
    child: Child,
    video_id: String,
    title: String,
    channel: String,
    marked: bool,
    paused: bool,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(SETTINGS_FILE))
}

fn load(app: &AppHandle) -> Saved {
    settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, saved: &Saved) {
    if let (Some(p), Ok(text)) = (settings_path(app), serde_json::to_string(saved)) {
        let _ = std::fs::write(p, text);
    }
}

/// `exe` on PATH, or in one of `extra` folders.
fn find(exe: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(extra.iter().cloned())
        .map(|dir| dir.join(exe))
        .find(|p| p.is_file())
}

fn mpv_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
    find(
        "mpv.exe",
        &[
            env("ProgramFiles").join("MPV Player"),
            env("ProgramFiles").join("mpv"),
            env("LOCALAPPDATA").join("Programs").join("mpv"),
            env("LOCALAPPDATA").join("mpv"),
        ],
    )
}

fn ytdlp_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    let winget = local.join("Microsoft").join("WinGet");
    let mut extra = vec![winget.join("Links")];
    // winget keeps the real exe in a per-package folder.
    if let Ok(dir) = std::fs::read_dir(winget.join("Packages")) {
        extra.extend(dir.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains("yt-dlp")));
    }
    find("yt-dlp.exe", &extra)
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000)
}

#[cfg(not(windows))]
fn no_window(cmd: &mut Command) -> &mut Command {
    cmd
}

/// Sends one command over mpv's IPC pipe and returns its `data`.
fn ipc(command: Value) -> Result<Value, String> {
    let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(PIPE).map_err(|e| e.to_string())?;
    let request = json!({ "command": command, "request_id": 1 });
    pipe.write_all(format!("{request}\n").as_bytes()).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    // mpv also pushes events on the pipe; skip to our reply.
    for _ in 0..64 {
        line.clear();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["request_id"] == 1 {
            return if v["error"] == "success" { Ok(v["data"].clone()) } else { Err(v["error"].to_string()) };
        }
    }
    Err("mpv не ответил".into())
}

fn alive(session: &mut Session) -> bool {
    matches!(session.child.try_wait(), Ok(None))
}

/// The session, if mpv is still running.
fn running(slot: &mut Option<Session>) -> Option<&mut Session> {
    let session = slot.as_mut()?;
    if alive(session) { Some(session) } else { None }
}

/// Updates yt-dlp in the background when the last update is a day old.
fn update_ytdlp(app: &AppHandle, ytdlp: &Path) {
    let mut saved = load(app);
    if now_secs().saturating_sub(saved.ytdlp_updated) < YTDLP_UPDATE_EVERY {
        return;
    }
    saved.ytdlp_updated = now_secs();
    save(app, &saved);
    let ytdlp = ytdlp.to_path_buf();
    std::thread::spawn(move || {
        let _ = no_window(Command::new(ytdlp).arg("-U")).status();
    });
}

/// Plays a video in the mpv window (opening it, or replacing what it plays).
/// `start` is in seconds. Errors when mpv or yt-dlp is missing: the widget
/// then opens the browser instead.
#[tauri::command]
pub fn player_play(
    app: AppHandle,
    id: String,
    url: String,
    title: String,
    channel: String,
    start: Option<u32>,
) -> Result<(), String> {
    let mpv = mpv_path().ok_or("mpv не установлен")?;
    let ytdlp = ytdlp_path().ok_or("yt-dlp не установлен")?;
    update_ytdlp(&app, &ytdlp);

    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(session) = running(&mut guard) {
        // Same window, new video.
        let _ = ipc(json!(["set_property", "start", start.map_or("none".to_string(), |s| s.to_string())]));
        ipc(json!(["loadfile", url, "replace"]))?;
        let _ = ipc(json!(["set_property", "pause", false]));
        session.video_id = id;
        session.title = title;
        session.channel = channel;
        session.marked = false;
        session.paused = false;
        return Ok(());
    }

    let geometry = load(&app).geometry.unwrap_or_else(|| DEFAULT_GEOMETRY.into());
    let mut cmd = Command::new(mpv);
    cmd.arg(&url)
        .arg("--ontop")
        .arg("--title-bar=no")
        .arg("--force-window=immediate")
        .arg("--keep-open=no")
        .arg(format!("--geometry={geometry}"))
        .arg(format!("--input-ipc-server={PIPE}"))
        // A slim one-line control bar that shows on hover only; no title or
        // window buttons drawn on top of the video.
        .arg(format!(
            "--script-opts=ytdl_hook-ytdl_path={},osc-layout=slimbottombar,osc-windowcontrols=no,osc-hidetimeout=700,osc-fadeduration=150,osc-deadzonesize=0.6",
            ytdlp.display()
        ))
        .arg("--osd-bar=no")
        .arg("--ytdl-format=bestvideo[height<=?1080]+bestaudio/best")
        .arg(format!("--title=${{media-title}} — {channel}"));
    if let Some(s) = start {
        cmd.arg(format!("--start={s}"));
    }
    let child = no_window(&mut cmd).spawn().map_err(|e| format!("Не удалось запустить mpv: {e}"))?;
    *guard = Some(Session { child, video_id: id, title, channel, marked: false, paused: false });
    drop(guard);

    let app = app.clone();
    std::thread::spawn(move || watch(app));
    Ok(())
}

/// While mpv runs: marks the video watched at 90 %, follows pause, and keeps
/// the window's place for next time.
fn watch(app: AppHandle) {
    let mut last_geometry: Option<String> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let pid = {
            let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
            let Some(session) = guard.as_mut() else { return };
            if !alive(session) {
                *guard = None;
                break;
            }
            session.child.id()
        };
        #[cfg(windows)]
        if let Some(g) = window::geometry(pid) {
            last_geometry = Some(g);
        }
        #[cfg(not(windows))]
        let _ = pid;

        let percent = ipc(json!(["get_property", "percent-pos"])).ok().and_then(|v| v.as_f64());
        let paused = ipc(json!(["get_property", "pause"])).ok().and_then(|v| v.as_bool());
        let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        let Some(session) = guard.as_mut() else { return };
        if let Some(p) = paused {
            session.paused = p;
        }
        if !session.marked && percent.is_some_and(|p| p >= WATCHED_AT) {
            session.marked = true;
            let id = session.video_id.clone();
            drop(guard);
            let _ = crate::youtube::youtube_set_watched(app.clone(), id, true);
            let _ = app.emit("youtube:changed", ());
        }
    }
    if let Some(g) = last_geometry {
        let mut saved = load(&app);
        saved.geometry = Some(g);
        save(&app, &saved);
    }
}

/// What mpv plays, for the taskbar button's player: (title, channel, playing).
pub fn now_playing() -> Option<(String, String, bool)> {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let session = running(&mut guard)?;
    Some((session.title.clone(), session.channel.clone(), !session.paused))
}

/// Play / pause from the taskbar button; false when mpv isn't running.
pub fn toggle() -> bool {
    let running = {
        let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        match running(&mut guard) {
            Some(s) => {
                s.paused = !s.paused;
                true
            }
            None => false,
        }
    };
    running && ipc(json!(["cycle", "pause"])).is_ok()
}

/// Whether mpv and yt-dlp are there, for the widget.
#[tauri::command]
pub fn player_available() -> bool {
    mpv_path().is_some() && ytdlp_path().is_some()
}

#[cfg(windows)]
mod window {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed,
    };

    /// `WxH+X+Y` of the visible top-level window of `pid`, unless minimised or
    /// maximised (those shouldn't become the next start size).
    pub fn geometry(pid: u32) -> Option<String> {
        struct Find {
            pid: u32,
            hwnd: Option<HWND>,
        }
        unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
            let find = unsafe { &mut *(data.0 as *mut Find) };
            let mut owner = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
            if owner == find.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
                find.hwnd = Some(hwnd);
                return false.into();
            }
            true.into()
        }
        let mut find = Find { pid, hwnd: None };
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut find as *mut _ as isize));
        }
        let hwnd = find.hwnd?;
        unsafe {
            if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(hwnd, &mut r).ok()?;
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            (w > 100 && h > 60).then(|| format!("{w}x{h}+{}+{}", r.left, r.top))
        }
    }
}
