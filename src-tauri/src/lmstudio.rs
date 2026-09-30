//! LM Studio's local server on this PC (Developer tab → Start Server, or `lms server start`).
//!
//! Questions go to its OpenAI-compatible `/v1/chat/completions` and stream back as
//! the same `ask:delta` / `ask:done` events the Claude Code questions use, so the
//! Ask box needs nothing extra. No token: the server is open on loopback. The
//! model is the one picked in settings, or the first one the server lists.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::secrets;

const MODEL_KEY: &str = "DockPanel/lmstudio-model";
const BASE: &str = "http://127.0.0.1:1234";

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
    /// The server answers.
    reachable: bool,
    /// Model ids the server lists.
    models: Vec<String>,
    /// The model picked in settings, if any.
    model: Option<String>,
    error: Option<String>,
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())
}

fn unreachable_message(e: &reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        "LM Studio не отвечает: запустите сервер (вкладка Developer → Start Server)".into()
    } else {
        e.to_string()
    }
}

/// The models the server lists.
async fn models() -> Result<Vec<String>, String> {
    let res = client(Duration::from_secs(5))?
        .get(format!("{BASE}/v1/models"))
        .send()
        .await
        .map_err(|e| unreachable_message(&e))?;
    if !res.status().is_success() {
        return Err(format!("LM Studio ответил {}", res.status().as_u16()));
    }
    let v: Value = res.json().await.map_err(|e| e.to_string())?;
    Ok(model_ids(&v))
}

fn model_ids(v: &Value) -> Vec<String> {
    v["data"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn lmstudio_status() -> Status {
    let model = secrets::read(MODEL_KEY);
    match models().await {
        Ok(models) => Status { reachable: true, models, model, error: None },
        Err(e) => Status { reachable: false, models: vec![], model, error: Some(e) },
    }
}

/// Saves the model to ask; an empty name goes back to the server's first one.
#[tauri::command]
pub fn lmstudio_set_model(model: String) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        secrets::delete(MODEL_KEY);
        Ok(())
    } else {
        secrets::write(MODEL_KEY, model)
    }
}

/// Starts a question and reports through the ask events.
pub fn ask(app: AppHandle, id: u64, prompt: String) -> Result<(), String> {
    CURRENT.store(id, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let outcome = stream(&app, id, &prompt).await;
        if CURRENT.load(Ordering::SeqCst) != id {
            return;
        }
        CURRENT.store(0, Ordering::SeqCst);
        let (text, error) = match outcome {
            Ok(text) if text.is_empty() => ("LM Studio не ответил".to_string(), true),
            Ok(text) => (text, false),
            Err(e) => (e, true),
        };
        let _ = app.emit("ask:done", Done { id, text, error, duration_ms: started.elapsed().as_millis() as u64 });
    });
    Ok(())
}

/// Stops the running question, if it is an LM Studio one.
pub fn cancel() {
    CURRENT.store(0, Ordering::SeqCst);
}

async fn stream(app: &AppHandle, id: u64, prompt: &str) -> Result<String, String> {
    let model = match secrets::read(MODEL_KEY) {
        Some(m) => m,
        None => models()
            .await?
            .into_iter()
            .next()
            .ok_or("В LM Studio нет моделей: скачайте и загрузите модель")?,
    };
    let body = json!({
        "model": model,
        "stream": true,
        "messages": [
            { "role": "system", "content": "Отвечай по делу и кратко, на языке вопроса." },
            { "role": "user", "content": prompt },
        ],
    });
    // A model that is not loaded yet loads on the first request (JIT loading), so allow for that.
    let mut res = client(Duration::from_secs(300))?
        .post(format!("{BASE}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| unreachable_message(&e))?;
    if !res.status().is_success() {
        let code = res.status().as_u16();
        let text = res.text().await.unwrap_or_default();
        let message = serde_json::from_str::<Value>(&text).ok().and_then(|v| {
            v["error"]["message"].as_str().or_else(|| v["error"].as_str()).map(str::to_string)
        });
        return Err(message.unwrap_or_else(|| format!("LM Studio ответил {code}")));
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
    v["choices"][0]["delta"]["content"].as_str().filter(|t| !t.is_empty()).map(str::to_string)
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
        assert_eq!(delta_text(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#), None);
        assert_eq!(delta_text("data: [DONE]"), None);
    }

    #[test]
    fn lists_models() {
        let v = json!({ "data": [{ "id": "qwen2.5-7b" }, { "id": "llama-3.2-3b" }] });
        assert_eq!(model_ids(&v), ["qwen2.5-7b", "llama-3.2-3b"]);
        assert!(model_ids(&json!({})).is_empty());
    }
}
