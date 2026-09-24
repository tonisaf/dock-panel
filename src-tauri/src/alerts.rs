//! Bell toggles on the AI cards: a Windows toast plus a sound when a
//! rate-limit window resets. Runs on a background thread, so it works while
//! the panel is hidden.
//!
//! Upcoming resets are remembered in a watch list rather than read straight
//! from the snapshot, because a fresh snapshot fetched right after a reset no
//! longer contains the window that just reset.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::ai_limits::{self, Snapshot};

const STATE_FILE: &str = "ai-alerts.json";
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// Resets older than this (e.g. the app was closed at the time) are dropped silently.
const MAX_LATE_SECS: i64 = 15 * 60;

#[derive(Serialize, Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct AlertSettings {
    pub claude: bool,
    pub codex: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Watched {
    provider: String,
    kind: String,
    resets_at: i64,
    used_percent: f64,
}

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    settings: AlertSettings,
    watched: Vec<Watched>,
}

static STATE: Mutex<Option<Persisted>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut Persisted) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Persisted::default))
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(STATE_FILE))
}

fn save(app: &AppHandle) {
    let Some(path) = state_path(app) else { return };
    let text = with_state(|s| serde_json::to_string_pretty(s).ok());
    if let Some(text) = text {
        let _ = std::fs::write(path, text);
    }
}

pub fn settings() -> AlertSettings {
    with_state(|s| s.settings)
}

pub fn init(app: &AppHandle) {
    let persisted = state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Persisted>(&t).ok())
        .unwrap_or_default();
    with_state(|s| *s = persisted);

    let app = app.clone();
    std::thread::spawn(move || loop {
        check(&app);
        std::thread::sleep(CHECK_EVERY);
    });
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn check(app: &AppHandle) {
    let settings = settings();
    if !settings.claude && !settings.codex {
        return;
    }
    let (claude, codex) = ai_limits::snapshots(app);
    let now = now_secs();

    let due = with_state(|s| {
        let before = s.watched.clone();
        for (provider, enabled, snapshot) in [("Claude", settings.claude, claude), ("Codex", settings.codex, codex)] {
            if enabled {
                watch(&mut s.watched, provider, snapshot.as_ref(), now);
            }
        }
        let (due, pending): (Vec<_>, Vec<_>) = s.watched.drain(..).partition(|w| w.resets_at <= now);
        s.watched = pending;
        let changed = s.watched != before;
        let due: Vec<Watched> = due.into_iter().filter(|w| now - w.resets_at <= MAX_LATE_SECS).collect();
        (due, changed)
    });

    let (due, changed) = due;
    for w in &due {
        let enabled = match w.provider.as_str() {
            "Claude" => settings.claude,
            _ => settings.codex,
        };
        if enabled {
            notify(
                app,
                &format!("{}: лимит сброшен", w.provider),
                &format!(
                    "Окно «{}» обнулилось. До сброса было использовано {}%.",
                    window_label(&w.kind),
                    w.used_percent.round()
                ),
            );
        }
    }
    if changed || !due.is_empty() {
        save(app);
    }
}

/// Adds upcoming resets from `snapshot` that actually have something to reset.
fn watch(watched: &mut Vec<Watched>, provider: &str, snapshot: Option<&Snapshot>, now: i64) {
    let Some(snapshot) = snapshot else { return };
    for w in &snapshot.windows {
        if w.used_percent <= 0.0 || w.resets_at <= now {
            continue;
        }
        let existing = watched
            .iter_mut()
            .find(|x| x.provider == provider && x.kind == w.kind && x.resets_at == w.resets_at);
        match existing {
            Some(x) => x.used_percent = w.used_percent,
            None => watched.push(Watched {
                provider: provider.into(),
                kind: w.kind.clone(),
                resets_at: w.resets_at,
                used_percent: w.used_percent,
            }),
        }
    }
}

fn window_label(kind: &str) -> &str {
    match kind {
        "five_hour" => "5 часов",
        "seven_day" => "Неделя",
        "seven_day_opus" => "Opus, неделя",
        "seven_day_sonnet" => "Sonnet, неделя",
        "spend_limit" => "Расходы",
        _ => "Доп. лимит",
    }
}

#[tauri::command]
pub async fn ai_alerts_set(app: AppHandle, provider: String, enabled: bool) -> Result<(), String> {
    let name = match provider.as_str() {
        "claude" => "Claude",
        "codex" => "Codex",
        _ => return Err(format!("unknown provider {provider}")),
    };
    with_state(|s| {
        match name {
            "Claude" => s.settings.claude = enabled,
            _ => s.settings.codex = enabled,
        }
        if !enabled {
            s.watched.retain(|w| w.provider != name);
        }
    });
    save(&app);

    if enabled {
        // Confirms that toasts and sound actually reach the user.
        notify(
            &app,
            &format!("{name}: уведомления включены"),
            "Сообщу со звуком, когда лимит сбросится.",
        );
        let app = app.clone();
        std::thread::spawn(move || check(&app));
    }
    Ok(())
}

#[cfg(windows)]
fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_winrt_notification::{Duration as ToastDuration, Toast};
    use windows::core::w;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC};

    // A dev build has no registered AppUserModelID; borrow PowerShell's.
    let app_id = if cfg!(debug_assertions) {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    };
    // The toast is silent and we play the sound ourselves, so the sound is
    // heard even when Windows mutes notification sounds.
    if let Err(e) = Toast::new(&app_id)
        .title(title)
        .text1(body)
        .sound(None)
        .duration(ToastDuration::Short)
        .show()
    {
        eprintln!("toast failed: {e}");
    }
    unsafe {
        let _ = PlaySoundW(w!("Notification.Reminder"), None, SND_ALIAS | SND_ASYNC);
    }
}
