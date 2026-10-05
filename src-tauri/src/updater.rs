//! Self-update from GitHub Releases of `tonisaf/dock-panel`.
//!
//! The updater endpoint is resolved per check: find the latest release's
//! `latest.json` asset and point tauri-plugin-updater at its API URL.
//! `latest.json` in turn points at the installer asset's API URL (written by
//! `scripts/release.mjs`); the plugin sends the same headers when downloading
//! it. Updates are signed; the public key is in tauri.conf.json.
//!
//! Updates use anonymous access to the public repository.

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
    available: Option<Available>,
    error: Option<String>,
}

static LAST: Mutex<(Option<Available>, Option<String>)> = Mutex::new((None, None));

enum GithubError {
    Other(String),
}

impl GithubError {
    fn message(self) -> String {
        match self {
            GithubError::Other(text) => text,
        }
    }
}

fn failure(status: StatusCode) -> GithubError {
    match status {
        StatusCode::NOT_FOUND => GithubError::Other("Репозиторий или релиз не найден".into()),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
            GithubError::Other("GitHub ограничил запросы, попробуйте позже".into())
        }
        s => GithubError::Other(format!("GitHub ответил {s}")),
    }
}

async fn github_get(path: &str) -> Result<Value, GithubError> {
    let req = net::client()
        .get(format!("https://api.github.com{path}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    let res = req
        .send()
        .await
        .map_err(|e| GithubError::Other(format!("Нет связи с GitHub: {e}")))?;
    if res.status().is_success() {
        res.json()
            .await
            .map_err(|e| GithubError::Other(e.to_string()))
    } else {
        Err(failure(res.status()))
    }
}

/// API URL of the `latest.json` asset of the newest release.
async fn manifest_url() -> Result<String, GithubError> {
    let release = github_get(&format!("/repos/{REPO}/releases/latest")).await?;
    release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == "latest.json")
        .and_then(|a| a["url"].as_str())
        .map(str::to_string)
        .ok_or_else(|| GithubError::Other("В последнем релизе нет latest.json".into()))
}

async fn check_public(
    app: &AppHandle,
) -> Result<Option<tauri_plugin_updater::Update>, GithubError> {
    let other = |e: String| GithubError::Other(e);
    let manifest = manifest_url().await?;
    let builder = app
        .updater_builder()
        .endpoints(vec![manifest.parse().map_err(|e| other(format!("{e}")))?])
        .map_err(|e| other(e.to_string()))?;
    builder
        .header("Accept", "application/octet-stream")
        .map_err(|e| other(e.to_string()))?
        .build()
        .map_err(|e| other(e.to_string()))?
        .check()
        .await
        .map_err(|e| other(format!("Проверка обновления не удалась: {e}")))
}

async fn find_update(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    check_public(app).await.map_err(GithubError::message)
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
        available,
        error,
    }
}

/// Background checks: shortly after start, then every few hours.
pub fn init(app: &AppHandle) {
    // Remove the credential used by older versions for the private repository.
    secrets::delete(SECRET);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_AFTER);
        loop {
            tauri::async_runtime::block_on(check_and_remember(&app));
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    let (available, error) = LAST.lock().unwrap_or_else(|e| e.into_inner()).clone();
    UpdateStatus {
        current: app.package_info().version.to_string(),
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
