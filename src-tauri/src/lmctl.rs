//! Controls LM Studio from the panel through its `lms` command line tool
//! (`~/.lmstudio/bin/lms.exe`): which models are loaded and which are on disk,
//! load / unload, and the local server on / off.
//!
//! `lms ps --json` and `lms ls --json` are read leniently (the fields are
//! LM Studio's own); every model key that goes into a command line is checked
//! first, so the webview cannot smuggle in an option.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    /// What `lms load` takes.
    key: String,
    /// What `lms unload` takes (loaded models only).
    identifier: Option<String>,
    name: String,
    /// `llm`, `embedding`, ...
    kind: String,
    params: Option<String>,
    quantization: Option<String>,
    size_bytes: u64,
    /// Loaded context window.
    context_length: Option<u64>,
    max_context_length: Option<u64>,
    /// Auto-unload after this many idle ms.
    ttl_ms: Option<u64>,
    last_used_ms: Option<u64>,
    /// `idle` or `generating`, loaded models only.
    status: Option<String>,
    vision: bool,
    tool_use: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// The local server answers.
    running: bool,
    loaded: Vec<Model>,
    /// On disk and not loaded.
    available: Vec<Model>,
    /// The CLI is missing or failed.
    error: Option<String>,
}

fn lms_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    home.join(".lmstudio").join("bin").join("lms.exe")
}

/// Runs `lms` and returns its stdout.
fn lms(args: &[&str]) -> Result<String, String> {
    let exe = lms_path();
    if !exe.exists() {
        return Err("Не найден lms (~/.lmstudio/bin/lms.exe): запустите LM Studio один раз".into());
    }
    let mut cmd = Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("Не удалось запустить lms: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let text = String::from_utf8_lossy(&out.stderr);
        let text = if text.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { text };
        let line = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("lms завершился с ошибкой");
        Err(line.trim().to_string())
    }
}

fn parse_models(json: &str) -> Vec<Model> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json.trim()) else { return vec![] };
    items
        .iter()
        .filter_map(|m| {
            let key = m["modelKey"].as_str()?.to_string();
            Some(Model {
                identifier: m["identifier"].as_str().map(str::to_string),
                name: m["displayName"].as_str().unwrap_or(&key).to_string(),
                kind: m["type"].as_str().unwrap_or("llm").to_string(),
                params: m["paramsString"].as_str().map(str::to_string),
                quantization: m["quantization"]["name"].as_str().map(str::to_string),
                size_bytes: m["sizeBytes"].as_u64().unwrap_or(0),
                context_length: m["contextLength"].as_u64(),
                max_context_length: m["maxContextLength"].as_u64(),
                ttl_ms: m["ttlMs"].as_u64(),
                last_used_ms: m["lastUsedTime"].as_u64(),
                status: m["status"].as_str().map(str::to_string),
                vision: m["vision"] == true,
                tool_use: m["trainedForToolUse"] == true,
                key,
            })
        })
        .collect()
}

/// A model key is a path-like name; nothing that starts an option or breaks a command line.
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 300
        && !key.starts_with('-')
        && key.chars().all(|c| c.is_alphanumeric() || "_-./@:+() ".contains(c))
}

#[tauri::command]
pub async fn lmstudio_overview() -> Overview {
    let running = crate::lmstudio::is_running().await;
    let (loaded, available, error) = tauri::async_runtime::spawn_blocking(|| {
        let ps = lms(&["ps", "--json"]);
        let ls = lms(&["ls", "--json"]);
        match (ps, ls) {
            (Ok(ps), Ok(ls)) => {
                let loaded = parse_models(&ps);
                let available = parse_models(&ls).into_iter().filter(|m| !loaded.iter().any(|l| l.key == m.key)).collect();
                (loaded, available, None)
            }
            (Err(e), _) | (_, Err(e)) => (vec![], vec![], Some(e)),
        }
    })
    .await
    .unwrap_or_else(|e| (vec![], vec![], Some(e.to_string())));
    Overview { running, loaded, available, error }
}

/// Loads a model; `ttl_minutes` unloads it after that long without use.
#[tauri::command]
pub async fn lmstudio_load(key: String, ttl_minutes: Option<u32>) -> Result<(), String> {
    if !valid_key(&key) {
        return Err("Неверное имя модели".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let ttl = ttl_minutes.filter(|m| *m > 0).map(|m| (u64::from(m) * 60).to_string());
        let mut args = vec!["load", key.as_str(), "-y"];
        if let Some(seconds) = &ttl {
            args.extend(["--ttl", seconds]);
        }
        lms(&args).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Unloads one model, or all of them when no identifier is given.
#[tauri::command]
pub async fn lmstudio_unload(identifier: Option<String>) -> Result<(), String> {
    if identifier.as_deref().is_some_and(|i| !valid_key(i)) {
        return Err("Неверное имя модели".into());
    }
    tauri::async_runtime::spawn_blocking(move || match identifier {
        Some(id) => lms(&["unload", &id]).map(|_| ()),
        None => lms(&["unload", "--all"]).map(|_| ()),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn lmstudio_server(start: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || lms(&["server", if start { "start" } else { "stop" }]).map(|_| ()))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = r#"[{"type":"llm","modelKey":"qwen/qwen3.6-35b-a3b","displayName":"Qwen3.6 35B A3B","paramsString":"35B-A3B","quantization":{"name":"Q4_K_M","bits":4},"identifier":"qwen/qwen3.6-35b-a3b","sizeBytes":22069769172,"ttlMs":null,"lastUsedTime":1790799827217,"vision":true,"trainedForToolUse":true,"maxContextLength":262144,"contextLength":65536,"status":"idle"}]"#;

    #[test]
    fn reads_loaded_models() {
        let m = &parse_models(PS)[0];
        assert_eq!(m.key, "qwen/qwen3.6-35b-a3b");
        assert_eq!((m.size_bytes, m.context_length, m.ttl_ms), (22_069_769_172, Some(65536), None));
        assert_eq!((m.status.as_deref(), m.quantization.as_deref()), (Some("idle"), Some("Q4_K_M")));
        assert!(m.vision && m.tool_use);
    }

    #[test]
    fn reads_models_on_disk_and_survives_junk() {
        let ls = r#"[{"type":"embedding","modelKey":"text-embedding-nomic-embed-text-v1.5","displayName":"Nomic Embed Text v1.5","sizeBytes":84106624,"maxContextLength":2048}]"#;
        let m = &parse_models(ls)[0];
        assert_eq!((m.kind.as_str(), m.identifier.clone(), m.status.clone()), ("embedding", None, None));
        assert!(parse_models("").is_empty() && parse_models("[]").is_empty() && parse_models("not json").is_empty());
        assert!(parse_models(r#"[{"displayName":"no key"}]"#).is_empty());
    }

    #[test]
    fn keys_cannot_inject_options() {
        assert!(valid_key("qwen/qwen3.6-35b-a3b") && valid_key("nomic-ai/nomic-embed-text-v1.5-GGUF/x.Q4_K_M.gguf"));
        assert!(!valid_key("--all") && !valid_key("") && !valid_key("a; rm") && !valid_key("a\"b"));
    }
}
