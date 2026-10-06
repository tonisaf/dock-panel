use crate::lmctl::{Model, Overview};
use serde_json::{json, Value};
use std::{
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};
static SERVER: Mutex<Option<Child>> = Mutex::new(None);
async fn api(endpoint: &str, body: Option<Value>) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(if body.is_some() { 300 } else { 5 }))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("http://127.0.0.1:11434/api/{endpoint}");
    let request = if let Some(body) = body {
        client.post(url).json(&body)
    } else {
        client.get(url)
    };
    let response = request
        .send()
        .await
        .map_err(|e| format!("Ollama не отвечает: {e}"))?;
    let code = response.status();
    let value: Value = response.json().await.map_err(|e| e.to_string())?;
    if !code.is_success() {
        return Err(value["error"].as_str().unwrap_or("Ошибка Ollama").into());
    }
    Ok(value)
}
fn model(v: &Value, loaded: bool) -> Model {
    let key = v["name"]
        .as_str()
        .or_else(|| v["model"].as_str())
        .unwrap_or_default()
        .to_string();
    Model {
        identifier: loaded.then(|| key.clone()),
        name: key.clone(),
        key,
        kind: "llm".into(),
        params: v["details"]["parameter_size"].as_str().map(str::to_string),
        quantization: v["details"]["quantization_level"]
            .as_str()
            .map(str::to_string),
        size_bytes: v["size"].as_u64().unwrap_or(0),
        context_length: v["context_length"].as_u64(),
        max_context_length: None,
        ttl_ms: None,
        last_used_ms: None,
        status: None,
        vision: false,
        tool_use: false,
    }
}
pub async fn overview() -> Overview {
    let result = async {
        let tags = api("tags", None).await?;
        let ps = api("ps", None).await?;
        let loaded: Vec<Model> = ps["models"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| model(v, true))
            .collect();
        let available = tags["models"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| model(v, false))
            .filter(|m| !loaded.iter().any(|l| l.key == m.key))
            .collect();
        Ok::<_, String>((loaded, available))
    }
    .await;
    match result {
        Ok((loaded, available)) => Overview {
            running: true,
            loaded,
            available,
            error: None,
        },
        Err(e) => Overview {
            running: false,
            loaded: vec![],
            available: vec![],
            error: Some(e),
        },
    }
}
pub async fn load(key: String, minutes: Option<u32>) -> Result<(), String> {
    if key.is_empty() || key.len() > 300 || key.chars().any(char::is_control) {
        return Err("Неверное имя модели".into());
    }
    // Only installed models; loading must never silently download a multi-GB model.
    let tags = api("tags", None).await?;
    if !tags["models"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|m| m["name"].as_str() == Some(&key) || m["model"].as_str() == Some(&key))
    {
        return Err("Модель не установлена в Ollama. Сначала выполните ollama pull.".into());
    }
    let keep_alive = minutes
        .filter(|m| *m > 0)
        .map(|m| json!(u64::from(m) * 60))
        .unwrap_or(json!(-1));
    api(
        "generate",
        Some(json!({"model":key,"stream":false,"keep_alive":keep_alive})),
    )
    .await
    .map(|_| ())
}
pub async fn unload(id: Option<String>) -> Result<(), String> {
    let keys = match id {
        Some(id) => vec![id],
        None => api("ps", None).await?["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["name"].as_str().map(str::to_string))
            .collect(),
    };
    for key in keys {
        api(
            "generate",
            Some(json!({"model":key,"stream":false,"keep_alive":0})),
        )
        .await?;
    }
    Ok(())
}
pub async fn server(start: bool) -> Result<(), String> {
    if start && api("tags", None).await.is_ok() {
        return Ok(());
    }
    let mut owned = SERVER.lock().map_err(|e| e.to_string())?;
    if start {
        if let Some(child) = owned.as_mut() {
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                return Ok(());
            }
        }
        let mut command = Command::new("ollama");
        command
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        *owned = Some(command.spawn().map_err(|e| {
            format!("Не удалось запустить Ollama. Установите Ollama и добавьте её в PATH: {e}")
        })?);
    } else {
        let child=owned.as_mut().ok_or("Ollama запущена вне панели. Остановите её через значок в трее; здесь можно выгрузить модель.")?;
        child.kill().map_err(|e| e.to_string())?;
        child.wait().map_err(|e| e.to_string())?;
        *owned = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_installed_and_running_model_details() {
        let installed = model(
            &json!({"name":"qwen3:8b","size":5226537671u64,"details":{"parameter_size":"8.2B","quantization_level":"Q4_K_M"}}),
            false,
        );
        assert_eq!(installed.key, "qwen3:8b");
        assert_eq!(installed.identifier, None);
        assert_eq!(installed.quantization.as_deref(), Some("Q4_K_M"));
        let running = model(
            &json!({"model":"qwen3:8b","size":6442450944u64,"context_length":32768}),
            true,
        );
        assert_eq!(running.identifier.as_deref(), Some("qwen3:8b"));
        assert_eq!(running.context_length, Some(32768));
    }
}
