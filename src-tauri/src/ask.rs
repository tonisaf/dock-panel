//! Quick questions to Claude through the Claude Code CLI (`claude -p`), on the
//! user's own subscription: no API key of ours.
//!
//! The prompt goes in on stdin (no quoting through `cmd`), the answer streams
//! back as `stream-json` and reaches the UI as `ask:delta` / `ask:done` events.
//! Runs in an empty temp folder with no tools and only "local" settings, so
//! the user's hooks and CLAUDE.md files stay out of it, and with no session
//! persistence, so quick questions don't fill the history.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// The running question, so it can be cancelled.
static RUNNING: Mutex<Option<(u64, Child)>> = Mutex::new(None);

const NOT_LOGGED_IN: &str =
    "Claude Code в командной строке не вошёл в аккаунт. Откройте терминал и выполните: claude auth login";

#[derive(Serialize, Clone)]
struct Delta {
    id: u64,
    text: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Done {
    id: u64,
    /// The full answer, or the error message.
    text: String,
    error: bool,
    duration_ms: u64,
}

fn allowed_model(model: &str) -> bool {
    matches!(model, "haiku" | "sonnet" | "opus")
}

/// "openclaw" as the model sends the question to the OpenClaw Gateway instead of `claude -p`.
const OPENCLAW: &str = "openclaw";
/// "lmstudio" sends it to the local LM Studio server.
const LMSTUDIO: &str = "lmstudio";

/// Starts a question and returns its id; the answer arrives as events.
#[tauri::command]
pub fn ask_start(app: AppHandle, prompt: String, model: Option<String>) -> Result<u64, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Пустой вопрос".into());
    }
    let via_openclaw = model.as_deref() == Some(OPENCLAW);
    let via_lmstudio = model.as_deref() == Some(LMSTUDIO);
    let model = model
        .filter(|m| allowed_model(m))
        .unwrap_or_else(|| "sonnet".into());
    ask_cancel();
    if via_openclaw {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        crate::openclaw::ask(app, id, prompt)?;
        return Ok(id);
    }

    if via_lmstudio {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        crate::lmstudio::ask(app, id, prompt)?;
        return Ok(id);
    }

    let dir = std::env::temp_dir().join("dock-panel-ask");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("cmd");
    cmd.args([
        "/c",
        "claude",
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--tools",
        "",
        "--setting-sources",
        "local",
        "--no-session-persistence",
        "--model",
        &model,
        "--append-system-prompt",
        "Отвечай по делу и кратко, на языке вопроса. Это быстрый вопрос из панели, без доступа к файлам.",
    ])
    .current_dir(&dir)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить claude: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let stdout = child.stdout.take().ok_or("нет вывода")?;
    let stderr = child.stderr.take();

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, child));

    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut text = String::new();
        let mut result: Option<(String, bool)> = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v["type"].as_str() {
                Some("stream_event") => {
                    let delta = &v["event"]["delta"];
                    if delta["type"] == "text_delta" {
                        if let Some(t) = delta["text"].as_str() {
                            text.push_str(t);
                            let _ = app.emit(
                                "ask:delta",
                                Delta {
                                    id,
                                    text: t.to_string(),
                                },
                            );
                        }
                    }
                }
                Some("result") => {
                    let body = v["result"].as_str().unwrap_or_default().to_string();
                    result = Some((
                        body,
                        v["is_error"].as_bool().unwrap_or(false) || v["subtype"] != "success",
                    ));
                }
                _ => {}
            }
        }
        let stderr_text = stderr
            .map(|mut s| {
                let mut buf = String::new();
                let _ = std::io::Read::read_to_string(&mut s, &mut buf);
                buf
            })
            .unwrap_or_default();

        // Only the question that is still current reports; a cancelled one stays quiet.
        let current = {
            let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
            match running.as_mut() {
                Some((rid, child)) if *rid == id => {
                    let _ = child.wait();
                    *running = None;
                    true
                }
                _ => false,
            }
        };
        if !current {
            return;
        }
        let (body, error) = match result {
            Some((body, true)) => (friendly_error(&body), true),
            Some((body, false)) => (if body.is_empty() { text } else { body }, false),
            None if !text.is_empty() => (text, false),
            None => (friendly_error(stderr_text.trim()), true),
        };
        let done = Done {
            id,
            text: body,
            error,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        let _ = app.emit("ask:done", done);
    });
    Ok(id)
}

fn friendly_error(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("authenticate") || lower.contains("not logged in") || lower.contains("/login")
    {
        NOT_LOGGED_IN.into()
    } else if lower.contains("is not recognized") || lower.contains("не является внутренней")
    {
        "Claude Code не установлен: команда claude не найдена".into()
    } else if message.is_empty() {
        "Claude не ответил".into()
    } else {
        message.to_string()
    }
}

/// Stops the running question, if any.
#[tauri::command]
pub fn ask_cancel() {
    crate::openclaw::cancel();
    crate::lmstudio::cancel();
    if let Some((_, mut child)) = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).take() {
        // `cmd` started claude as a child: take the whole tree down.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &child.id().to_string()])
                .creation_flags(0x0800_0000)
                .status();
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// The clipboard's text, for the templates ("explain the code in the clipboard").
#[tauri::command]
pub fn clipboard_text() -> Option<String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::{HANDLE, HGLOBAL};
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;

        OpenClipboard(None).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|HANDLE(h)| {
            let global = HGLOBAL(h);
            let ptr = GlobalLock(global) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(global);
            Some(s)
        });
        let _ = CloseClipboard();
        text.filter(|t| !t.trim().is_empty())
    }
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_login_errors() {
        assert_eq!(
            friendly_error("Failed to authenticate: OAuth session expired"),
            NOT_LOGGED_IN
        );
        assert_eq!(friendly_error("Rate limited"), "Rate limited");
        assert!(allowed_model("haiku") && !allowed_model("--dangerously-skip-permissions"));
    }
}
