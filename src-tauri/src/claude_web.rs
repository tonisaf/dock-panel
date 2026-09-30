//! Experimental: Claude subscription limits for people who only use the
//! Claude apps, read from claude.ai's own (undocumented) usage endpoint.
//!
//! The user signs in to claude.ai in a regular window we open; the session
//! lives in our WebView2 profile, and we never see the password. To fetch, a
//! hidden window on the claude.ai origin runs `fetch()` with that session and
//! hands the result back by navigating to a sentinel URL that we intercept and
//! cancel. claude.ai pages get no access to Tauri IPC.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::ai_limits::{LimitWindow, Snapshot};

const LOGIN_LABEL: &str = "claude-login";
const FETCH_LABEL: &str = "claude-fetch";
const LOGIN_URL: &str = "https://claude.ai/login";
/// A tiny same-origin JSON page to run the fetch from (no SPA to load).
const FETCH_PAGE_URL: &str = "https://claude.ai/api/organizations";
const REPORT_HOST: &str = "dock-panel.invalid";
const STATE_FILE: &str = "claude-web.json";
const REFRESH_EVERY: Duration = Duration::from_secs(5 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

pub const CHANGED_EVENT: &str = "ai-limits:changed";

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    enabled: bool,
    snapshot: Option<Snapshot>,
}

#[derive(Default)]
struct State {
    persisted: Persisted,
    needs_login: bool,
    error: Option<String>,
    last_attempt: Option<Instant>,
    /// Id of the in-flight fetch, so a late timeout can't clobber a newer one.
    in_flight: Option<u64>,
    next_id: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(State::default))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebStatus {
    pub enabled: bool,
    pub needs_login: bool,
    pub fetching: bool,
    pub error: Option<String>,
}

pub fn status() -> WebStatus {
    with_state(|s| WebStatus {
        enabled: s.persisted.enabled,
        needs_login: s.needs_login,
        fetching: s.in_flight.is_some(),
        error: s.error.clone(),
    })
}

pub fn snapshot() -> Option<Snapshot> {
    with_state(|s| if s.persisted.enabled { s.persisted.snapshot.clone() } else { None })
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(STATE_FILE))
}

fn save(app: &AppHandle) {
    let Some(path) = state_path(app) else { return };
    let persisted = with_state(|s| s.persisted.clone());
    if let Ok(text) = serde_json::to_string_pretty(&persisted) {
        let _ = std::fs::write(path, text);
    }
}

pub fn init(app: &AppHandle) {
    let persisted = state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Persisted>(&t).ok())
        .unwrap_or_default();
    with_state(|s| s.persisted = persisted);
}

/// Enabled, signed in, nothing in flight, and the last fetch was `min_gap` or more ago.
fn fetch_due(min_gap: Duration) -> bool {
    with_state(|s| {
        s.persisted.enabled
            && !s.needs_login
            && s.in_flight.is_none()
            && s.last_attempt.is_none_or(|t| t.elapsed() >= min_gap)
    })
}

/// Refresh in the background if enabled and the data is older than `REFRESH_EVERY`.
pub fn refresh_if_stale(app: &AppHandle) {
    if fetch_due(REFRESH_EVERY) {
        spawn_refresh(app);
    }
}

/// Fetches claude.ai now instead of waiting out `REFRESH_EVERY`, unless that was
/// tried within `min_gap`. Either way the UI is told to reread, which also picks
/// up the local sources (Claude Code's status line, Codex) that need no fetch.
pub fn refresh_now(app: &AppHandle, min_gap: Duration) {
    if fetch_due(min_gap) {
        // Announces itself when it starts and when it ends.
        spawn_refresh(app);
    } else {
        let _ = app.emit(CHANGED_EVENT, ());
    }
}

/// Window creation must not run on the main thread from inside a callback.
fn spawn_refresh(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || refresh(&app));
}

fn refresh(app: &AppHandle) {
    let Some(id) = with_state(|s| {
        if !s.persisted.enabled || s.in_flight.is_some() {
            return None;
        }
        s.next_id += 1;
        s.in_flight = Some(s.next_id);
        s.last_attempt = Some(Instant::now());
        Some(s.next_id)
    }) else {
        return;
    };
    let _ = app.emit(CHANGED_EVENT, ());

    let page: Url = FETCH_PAGE_URL.parse().expect("valid url");
    let started = match app.get_webview_window(FETCH_LABEL) {
        // Left over from a fetch that timed out; reloading re-runs the fetch
        // script via on_page_load.
        Some(win) => win.navigate(page).map_err(|e| e.to_string()),
        None => build_fetch_window(app, page),
    };
    if let Err(e) = started {
        finish(app, id, Err(e));
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FETCH_TIMEOUT);
        finish(&app, id, Err("claude.ai не ответил вовремя".into()));
    });
}

fn build_fetch_window(app: &AppHandle, page: Url) -> Result<(), String> {
    let report_app = app.clone();
    WebviewWindowBuilder::new(app, FETCH_LABEL, WebviewUrl::External(page))
        .title("Claude usage")
        .visible(false)
        .skip_taskbar(true)
        .on_navigation(move |url| {
            if url.host_str() == Some(REPORT_HOST) {
                handle_report(&report_app, url);
                return false;
            }
            true
        })
        .on_page_load(|win, payload| {
            if payload.event() == PageLoadEvent::Finished
                && payload.url().host_str() == Some("claude.ai")
            {
                let _ = win.eval(FETCH_SCRIPT);
            }
        })
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

enum Outcome {
    Data(Snapshot),
    NeedsLogin,
}

fn handle_report(app: &AppHandle, url: &Url) {
    let Some(id) = with_state(|s| s.in_flight) else { return };
    let report: Option<Value> = url
        .query_pairs()
        .find(|(k, _)| k == "d")
        .and_then(|(_, v)| serde_json::from_str(&v).ok());
    let Some(report) = report else {
        finish(app, id, Err("непонятный ответ claude.ai".into()));
        return;
    };

    let outcome = if report["ok"].as_bool() == Some(true) {
        let windows: Vec<LimitWindow> =
            serde_json::from_value(report["windows"].clone()).unwrap_or_default();
        let updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        Ok(Outcome::Data(Snapshot {
            updated_at,
            plan: report["plan"].as_str().map(str::to_string),
            windows,
        }))
    } else if report["auth"].as_bool() == Some(true) {
        Ok(Outcome::NeedsLogin)
    } else {
        Err(report["error"].as_str().unwrap_or("ошибка claude.ai").to_string())
    };
    finish(app, id, outcome);
}

fn finish(app: &AppHandle, id: u64, outcome: Result<Outcome, String>) {
    let applied = with_state(|s| {
        if s.in_flight != Some(id) {
            return false;
        }
        s.in_flight = None;
        match outcome {
            Ok(Outcome::Data(snapshot)) => {
                s.persisted.snapshot = Some(snapshot);
                s.needs_login = false;
                s.error = None;
            }
            Ok(Outcome::NeedsLogin) => {
                s.needs_login = true;
                s.error = None;
            }
            Err(e) => s.error = Some(e),
        }
        true
    });
    if applied {
        save(app);
        let _ = app.emit(CHANGED_EVENT, ());
        // The hidden claude.ai page costs a WebView2 renderer (~40 MB) and is
        // needed once every few minutes: close it until the next fetch. Off
        // this thread, which may be the page's own navigation callback.
        let app = app.clone();
        std::thread::spawn(move || {
            if let Some(win) = app.get_webview_window(FETCH_LABEL) {
                let _ = win.destroy();
            }
        });
    }
}

// ---- commands -------------------------------------------------------------------

#[tauri::command]
pub async fn claude_web_login(app: AppHandle) -> Result<(), String> {
    with_state(|s| {
        s.persisted.enabled = true;
        s.needs_login = false;
        s.error = None;
    });
    save(&app);

    if let Some(win) = app.get_webview_window(LOGIN_LABEL) {
        let _ = win.show();
        return win.set_focus().map_err(|e| e.to_string());
    }

    let nav_app = app.clone();
    let win = WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(LOGIN_URL.parse().expect("valid url")))
        .title("Вход в Claude — Dock Panel")
        .inner_size(480.0, 760.0)
        .center()
        .on_navigation(move |url| {
            // Leaving /login for the app itself means the sign-in went through.
            let signed_in = url.host_str() == Some("claude.ai")
                && ["/new", "/recents", "/chat", "/project"].iter().any(|p| url.path().starts_with(p));
            if signed_in {
                let app = nav_app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(800));
                    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
                        let _ = w.close();
                    }
                });
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;

    let app_on_close = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            with_state(|s| s.needs_login = false);
            spawn_refresh(&app_on_close);
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn claude_web_refresh(app: AppHandle) -> Result<(), String> {
    with_state(|s| s.needs_login = false);
    refresh(&app);
    Ok(())
}

/// Disables the feature and signs out by clearing the WebView2 profile's
/// browsing data (claude.ai cookies live there; app prefs do not).
#[tauri::command]
pub async fn claude_web_logout(app: AppHandle) -> Result<(), String> {
    with_state(|s| *s = State::default());
    save(&app);
    for label in [FETCH_LABEL, LOGIN_LABEL] {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.clear_all_browsing_data();
            let _ = win.destroy();
        }
    }
    if let Some(main) = app.get_webview_window(crate::panel::LABEL) {
        let _ = main.clear_all_browsing_data();
    }
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

/// Runs on a claude.ai page; reports back via a cancelled navigation.
const FETCH_SCRIPT: &str = r#"
(async () => {
  const report = (o) => {
    location.href = "https://dock-panel.invalid/report?d=" + encodeURIComponent(JSON.stringify(o));
  };
  try {
    const r = await fetch("/api/organizations", { credentials: "include" });
    if (r.status === 401 || r.status === 403) return report({ ok: false, auth: true });
    if (!r.ok) return report({ ok: false, error: "organizations: HTTP " + r.status });
    const orgs = await r.json();
    const org = orgs.find((o) => (o.capabilities || []).includes("chat")) || orgs[0];
    if (!org) return report({ ok: false, error: "в аккаунте нет организации" });

    const u = await fetch("/api/organizations/" + org.uuid + "/usage", { credentials: "include" });
    if (u.status === 401 || u.status === 403) return report({ ok: false, auth: true });
    if (!u.ok) return report({ ok: false, error: "usage: HTTP " + u.status });
    const usage = await u.json();

    const windows = Object.entries(usage)
      .filter(([kind, v]) => v && typeof v.utilization === "number"
        && (v.resets_at || kind === "five_hour" || kind === "seven_day"))
      .map(([kind, v]) => ({
        kind,
        usedPercent: v.utilization,
        resetsAt: v.resets_at ? Math.floor(Date.parse(v.resets_at) / 1000) : 0,
      }));
    report({ ok: true, plan: org.rate_limit_tier || null, windows });
  } catch (e) {
    report({ ok: false, error: String((e && e.message) || e) });
  }
})();
"#;
