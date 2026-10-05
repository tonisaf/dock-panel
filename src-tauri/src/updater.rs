//! Self-update from GitHub Releases of `tonisaf/dock-panel`.
//!
//! The updater endpoint is resolved per check: find the latest release's
//! `latest.json` asset and point tauri-plugin-updater at its API URL.
//! `latest.json` in turn points at the installer asset's API URL (written by
//! `scripts/release.mjs`); the plugin sends the same headers when downloading
//! it. Updates are signed; the public key is in tauri.conf.json.
//!
//! A public repo needs no credentials: the same API URLs answer anonymously.
//! While the repo is private (or if it goes private again) a fine-grained PAT
//! with read-only Contents, kept in Credential Manager, is sent as a bearer
//! token. A saved token the server rejects (expired, revoked) falls back to
//! anonymous access, which is enough for a public repo.

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

enum GithubError {
    /// GitHub rejected the token.
    BadToken,
    Other(String),
}

impl GithubError {
    fn message(self) -> String {
        match self {
            GithubError::BadToken => "Токен GitHub недействителен или истёк".into(),
            GithubError::Other(text) => text,
        }
    }
}

/// What a failed GitHub answer means for the user; `with_token` tells whether we sent one.
fn failure(status: StatusCode, with_token: bool) -> GithubError {
    match status {
        StatusCode::UNAUTHORIZED => GithubError::BadToken,
        StatusCode::NOT_FOUND if with_token => {
            GithubError::Other("Нет доступа к репозиторию или ещё нет ни одного релиза".into())
        }
        StatusCode::NOT_FOUND => GithubError::Other(
            "Репозиторий приватный или релизов ещё нет: для приватного укажите токен GitHub".into(),
        ),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => GithubError::Other(
            "GitHub отказал в доступе или ограничил запросы (403), попробуйте позже".into(),
        ),
        s => GithubError::Other(format!("GitHub ответил {s}")),
    }
}

async fn github_get(token: Option<&str>, path: &str) -> Result<Value, GithubError> {
    let mut req = net::client()
        .get(format!("https://api.github.com{path}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let res = req
        .send()
        .await
        .map_err(|e| GithubError::Other(format!("Нет связи с GitHub: {e}")))?;
    if res.status().is_success() {
        res.json()
            .await
            .map_err(|e| GithubError::Other(e.to_string()))
    } else {
        Err(failure(res.status(), token.is_some()))
    }
}

/// API URL of the `latest.json` asset of the newest release.
async fn manifest_url(token: Option<&str>) -> Result<String, GithubError> {
    let release = github_get(token, &format!("/repos/{REPO}/releases/latest")).await?;
    release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == "latest.json")
        .and_then(|a| a["url"].as_str())
        .map(str::to_string)
        .ok_or_else(|| GithubError::Other("В последнем релизе нет latest.json".into()))
}

/// One check with `token`, or anonymously when it is `None`.
async fn check_with(
    app: &AppHandle,
    token: Option<&str>,
) -> Result<Option<tauri_plugin_updater::Update>, GithubError> {
    let other = |e: String| GithubError::Other(e);
    let manifest = manifest_url(token).await?;
    let mut builder = app
        .updater_builder()
        .endpoints(vec![manifest.parse().map_err(|e| other(format!("{e}")))?])
        .map_err(|e| other(e.to_string()))?;
    if let Some(token) = token {
        builder = builder
            .header("Authorization", format!("Bearer {token}"))
            .map_err(|e| other(e.to_string()))?;
    }
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
    let token = secrets::read(SECRET);
    match check_with(app, token.as_deref()).await {
        // A stale token must not break updates from a public repo.
        Err(GithubError::BadToken) if token.is_some() => check_with(app, None)
            .await
            .map_err(|_| GithubError::BadToken.message()),
        other => other.map_err(GithubError::message),
    }
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
    github_get(Some(&token), &format!("/repos/{REPO}"))
        .await
        .map_err(GithubError::message)?;
    secrets::write(SECRET, &token)
}

#[tauri::command]
pub fn update_clear_token() {
    secrets::delete(SECRET);
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = (None, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(e: GithubError) -> String {
        e.message()
    }

    #[test]
    fn a_rejected_token_is_its_own_case() {
        assert!(matches!(
            failure(StatusCode::UNAUTHORIZED, true),
            GithubError::BadToken
        ));
        assert!(matches!(
            failure(StatusCode::UNAUTHORIZED, false),
            GithubError::BadToken
        ));
    }

    #[test]
    fn explains_a_404_by_whether_a_token_was_sent() {
        let with = text(failure(StatusCode::NOT_FOUND, true));
        let without = text(failure(StatusCode::NOT_FOUND, false));
        assert!(with.contains("Нет доступа"));
        assert!(without.contains("приватный") && without.contains("токен"));
    }

    #[test]
    fn other_statuses_are_reported() {
        assert!(text(failure(StatusCode::FORBIDDEN, false)).contains("403"));
        assert!(text(failure(StatusCode::TOO_MANY_REQUESTS, false)).contains("403"));
        assert_eq!(
            text(failure(StatusCode::BAD_GATEWAY, true)),
            "GitHub ответил 502 Bad Gateway"
        );
    }
}
