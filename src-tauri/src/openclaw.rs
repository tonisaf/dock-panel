//! OpenClaw Gateway on this PC (it lives in WSL, reachable on loopback).
//!
//! Questions go to its OpenAI-compatible `/v1/chat/completions` (must be enabled
//! in the Gateway's config) and stream back as the same `ask:delta` / `ask:done`
//! events the Claude Code questions use, so the Ask box needs nothing extra.
//! The Gateway token sits in Credential Manager; the webview never sees it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::secrets;

const SECRET: &str = "DockPanel/openclaw";
const BASE: &str = "http://127.0.0.1:18789";

/// The question that may still report; 0 when none (a new one or a cancel resets it).
static CURRENT: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Clone)]
struct Delta {
    id: u64,
    text: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Done {
    id: u64,
    text: String,
    error: bool,
    duration_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// A token is saved.
    connected: bool,
    /// The Gateway answers and accepts the token.
    reachable: bool,
    error: Option<String>,
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

fn unreachable_message(e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        "OpenClaw не отвечает: запущен ли Gateway (WSL-дистрибутив OpenClawGateway)?".into()
    } else {
        e.to_string()
    }
}

/// Checks the token against the Gateway; `Ok` when it is accepted.
async fn probe(token: &str) -> Result<(), String> {
    let res = client(Duration::from_secs(5))?
        .get(format!("{BASE}/v1/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| unreachable_message(&e))?;
    match res.status().as_u16() {
        200 => Ok(()),
        401 | 403 => Err("Gateway не принял токен".into()),
        404 => Err(
            "В Gateway выключен HTTP API: включите gateway.http.endpoints.chatCompletions".into(),
        ),
        code => Err(format!("Gateway ответил {code}")),
    }
}

#[tauri::command]
pub async fn openclaw_status() -> Status {
    let Some(token) = secrets::read(SECRET) else {
        return Status {
            connected: false,
            reachable: false,
            error: None,
        };
    };
    match probe(&token).await {
        Ok(()) => Status {
            connected: true,
            reachable: true,
            error: None,
        },
        Err(e) => Status {
            connected: true,
            reachable: false,
            error: Some(e),
        },
    }
}

#[tauri::command]
pub async fn openclaw_set_token(token: String) -> Result<(), String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("Вставьте токен Gateway".into());
    }
    probe(&token).await?;
    secrets::write(SECRET, &token)
}

#[tauri::command]
pub fn openclaw_disconnect() {
    secrets::delete(SECRET);
}

/// Starts a question to the default agent and returns its id.
pub fn ask(app: AppHandle, id: u64, prompt: String) -> Result<(), String> {
    let token = secrets::read(SECRET)
        .ok_or("OpenClaw не подключён: вставьте токен Gateway в настройках")?;
    CURRENT.store(id, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let outcome = stream(&app, id, &token, &prompt).await;
        if CURRENT.load(Ordering::SeqCst) != id {
            return;
        }
        CURRENT.store(0, Ordering::SeqCst);
        let (text, error) = match outcome {
            Ok(text) if text.is_empty() => ("OpenClaw не ответил".to_string(), true),
            Ok(text) => (text, false),
            Err(e) => (e, true),
        };
        let _ = app.emit(
            "ask:done",
            Done {
                id,
                text,
                error,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        );
    });
    Ok(())
}

/// Stops the running question, if it is an OpenClaw one.
pub fn cancel() {
    CURRENT.store(0, Ordering::SeqCst);
}

async fn stream(app: &AppHandle, id: u64, token: &str, prompt: &str) -> Result<String, String> {
    let body = json!({
        "model": "openclaw/default",
        "stream": true,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let mut res = client(Duration::from_secs(300))?
        .post(format!("{BASE}/v1/chat/completions"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| unreachable_message(&e))?;
    if !res.status().is_success() {
        let code = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string));
        return Err(match (code, message) {
            (401, Some(m)) if m.contains("No API key") => {
                "У агента OpenClaw нет доступа к модели: войдите в провайдера командой openclaw models auth login".into()
            }
            (401, _) => "Gateway не принял токен".into(),
            (404, _) => "В Gateway выключен HTTP API (gateway.http.endpoints.chatCompletions)".into(),
            (_, Some(m)) => m,
            (code, None) => format!("Gateway ответил {code}"),
        });
    }

    let mut full = String::new();
    let mut pending = String::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        if CURRENT.load(Ordering::SeqCst) != id {
            return Ok(full);
        }
        pending.push_str(&String::from_utf8_lossy(&chunk));
        // SSE events end at a newline; keep a trailing partial line for the next chunk.
        while let Some(end) = pending.find('\n') {
            let line: String = pending.drain(..=end).collect();
            if let Some(text) = delta_text(line.trim()) {
                full.push_str(&text);
                let _ = app.emit("ask:delta", Delta { id, text });
            }
        }
    }
    Ok(full)
}

/// The text of one `data: {...}` line of the stream, if it carries any.
fn delta_text(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    v["choices"][0]["delta"]["content"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_stream_lines() {
        assert_eq!(
            delta_text(r#"data: {"choices":[{"delta":{"content":"при"}}]}"#).as_deref(),
            Some("при")
        );
        assert_eq!(
            delta_text(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#),
            None
        );
        assert_eq!(delta_text("data: [DONE]"), None);
        assert_eq!(delta_text(": keep-alive"), None);
    }
}
