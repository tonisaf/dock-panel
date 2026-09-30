//! Self-update from GitHub Releases of the private `tonisaf/dock-panel` repo.
//!
//! Private release assets are only reachable through the GitHub API with a
//! token, so the updater endpoint is resolved per check: find the latest
//! release's `latest.json` asset and point tauri-plugin-updater at its API
//! URL with an `Authorization` header. `latest.json` in turn points at the
//! installer asset's API URL (written by `scripts/release.mjs`), and the
//! plugin sends the same headers when downloading it. Updates are signed; the
//! public key is in tauri.conf.json.
//!
//! The token is a fine-grained PAT with read-only Contents on the repo, kept in
//! Credential Manager.

use std::sync::Mutex;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::{net, secrets};

const REPO: &str = "tonisaf/dock-panel";
const SECRET: &str = "DockPanel/github";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(30);

pub const AVAILABLE_EVENT: &str = "update:available";
pub const PROGRESS_EVENT: &str = "update:progress";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    version: String,
    notes: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current: String,
    has_token: bool,
    available: Option<Available>,
    error: Option<String>,
}

static LAST: Mutex<(Option<Available>, Option<String>)> = Mutex::new((None, None));

fn token() -> Result<String, String> {
    secrets::read(SECRET).ok_or_else(|| "Не задан токен GitHub для обновлений".into())
}

async fn github_get(token: &str, path: &str) -> Result<Value, String> {
    let res = net::client()
        .get(format!("https://api.github.com{path}"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| format!("Нет связи с GitHub: {e}"))?;
    match res.status() {
        s if s.is_success() => res.json().await.map_err(|e| e.to_string()),
        StatusCode::UNAUTHORIZED => Err("Токен GitHub недействителен или истёк".into()),
        StatusCode::NOT_FOUND => {
            Err("Нет доступа к репозиторию или ещё нет ни одного релиза".into())
        }
        s => Err(format!("GitHub ответил {s}")),
    }
}

/// API URL of the `latest.json` asset of the newest release.
async fn manifest_url(token: &str) -> Result<String, String> {
    let release = github_get(token, &format!("/repos/{REPO}/releases/latest")).await?;
    release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == "latest.json")
        .and_then(|a| a["url"].as_str())
        .map(str::to_string)
        .ok_or_else(|| "В последнем релизе нет latest.json".into())
}

async fn find_update(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let token = token()?;
    let manifest = manifest_url(&token).await?;
    app.updater_builder()
        .endpoints(vec![manifest.parse().map_err(|e| format!("{e}"))?])
        .map_err(|e| e.to_string())?
        .header("Authorization", format!("Bearer {token}"))
        .map_err(|e| e.to_string())?
        .header("Accept", "application/octet-stream")
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("Проверка обновления не удалась: {e}"))
}

async fn check_and_remember(app: &AppHandle) -> UpdateStatus {
    let result = find_update(app).await;
    let (available, error) = match result {
        Ok(update) => (
            update.map(|u| Available {
                version: u.version,
                notes: u.body,
            }),
            None,
        ),
        Err(e) => (None, Some(e)),
    };
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = (available.clone(), error.clone());
    if let Some(a) = &available {
        let _ = app.emit(AVAILABLE_EVENT, a);
    }
    UpdateStatus {
        current: app.package_info().version.to_string(),
        has_token: secrets::read(SECRET).is_some(),
        available,
        error,
    }
}

/// Background checks: shortly after start, then every few hours.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_AFTER);
        loop {
            if secrets::read(SECRET).is_some() {
                tauri::async_runtime::block_on(check_and_remember(&app));
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    let (available, error) = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    UpdateStatus {
        current: app.package_info().version.to_string(),
        has_token: secrets::read(SECRET).is_some(),
        available,
        error,
    }
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateStatus {
    check_and_remember(&app).await
}

/// Downloads, verifies and installs; the NSIS installer closes and restarts the app.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let update = find_update(&app).await?.ok_or("Обновлений нет")?;
    let progress_app = app.clone();
    let mut received = 0usize;
    update
        .download_and_install(
            move |chunk, total| {
                received += chunk;
                let _ = progress_app.emit(PROGRESS_EVENT, (received, total));
            },
            || {},
        )
        .await
        .map_err(|e| format!("Не удалось установить обновление: {e}"))
}

/// Saves the token after checking it can read the repo.
#[tauri::command]
pub async fn update_set_token(token: String) -> Result<(), String> {
    let token = token.trim().to_string();
    github_get(&token, &format!("/repos/{REPO}")).await?;
    secrets::write(SECRET, &token)
}

#[tauri::command]
pub fn update_clear_token() {
    secrets::delete(SECRET);
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = (None, None);
}
