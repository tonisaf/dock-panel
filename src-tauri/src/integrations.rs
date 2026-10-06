use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub llm: String,
    pub notes: String,
    pub vault: String,
    pub tasks: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            llm: "lmstudio".into(),
            notes: "notion".into(),
            vault: String::new(),
            tasks: "inherit".into(),
        }
    }
}
static SETTINGS: OnceLock<Mutex<Settings>> = OnceLock::new();
static PATH: OnceLock<PathBuf> = OnceLock::new();
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let path = dir.join("integrations.json");
        let s = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let _ = PATH.set(path);
        let _ = SETTINGS.set(Mutex::new(s));
    }
}
#[tauri::command]
pub fn integrations_settings() -> Settings {
    SETTINGS
        .get_or_init(|| Mutex::new(Settings::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
pub fn ollama() -> bool {
    integrations_settings().llm == "ollama"
}
#[tauri::command]
pub fn integrations_save(mut settings: Settings) -> Result<Settings, String> {
    let _guard = crate::obsidian::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !["lmstudio", "ollama"].contains(&settings.llm.as_str())
        || !["notion", "obsidian", "local"].contains(&settings.notes.as_str())
    {
        return Err("Неизвестный источник".into());
    }
    if !["inherit", "notion", "obsidian"].contains(&settings.tasks.as_str()) {
        return Err("Неизвестный источник задач".into());
    }
    if settings.notes == "local" && settings.tasks == "inherit" {
        let previous = integrations_settings();
        settings.tasks = if previous.tasks == "inherit" {
            if previous.notes == "obsidian" {
                "obsidian"
            } else {
                "notion"
            }
            .into()
        } else {
            previous.tasks
        };
    }
    if settings.notes != "local" {
        settings.tasks = "inherit".into();
    }
    if settings.notes == "obsidian" || settings.tasks == "obsidian" {
        let path = std::fs::canonicalize(settings.vault.trim())
            .map_err(|_| "Укажите существующую папку хранилища Obsidian")?;
        if !path.is_dir() {
            return Err("Хранилище должно быть папкой".into());
        }
        settings.vault = path.to_string_lossy().into();
    }
    let path = PATH.get().ok_or("Настройки ещё не готовы")?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())?;
    *SETTINGS
        .get()
        .unwrap()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = settings.clone();
    Ok(settings)
}
