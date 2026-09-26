//! Google Calendar and Google Tasks for the calendar tab: read and edit
//! events, list and complete tasks.
//!
//! Sign-in is OAuth 2.0 for installed apps with the user's own "Desktop app"
//! client (Client ID + secret, which Google requires even with PKCE for this
//! client type) and a loopback redirect on any free port. The refresh token and
//! the client live in Credential Manager. In an OAuth app left in "Testing",
//! Google expires refresh tokens after 7 days; publishing it ("In production",
//! no verification needed for personal use) avoids that.

use std::collections::HashSet;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::{net, oauth, secrets};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_API: &str = "https://www.googleapis.com/calendar/v3";
const TASKS_API: &str = "https://tasks.googleapis.com/tasks/v1";
const SCOPES: &str = "https://www.googleapis.com/auth/calendar https://www.googleapis.com/auth/tasks";
const SECRET: &str = "DockPanel/google-calendar";
const FILE: &str = "gcal.json";

/// Google's event colour palette (`colorId` 1–11), as the Colors API reports it.
const EVENT_COLORS: [&str; 11] = [
    "#a4bdfc", "#7ae7bf", "#dbadff", "#ff887c", "#fbd75b", "#ffb878", "#46d6db", "#e1e1e1", "#5484ed", "#51b749",
    "#dc2127",
];

#[derive(Serialize, Deserialize)]
struct Stored {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

/// Calendars the user hid in the panel (Google's own "selected" is the default).
#[derive(Serialize, Deserialize, Default)]
struct Local {
    hidden: HashSet<String>,
    shown: HashSet<String>,
}

static ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn stored() -> Option<Stored> {
    serde_json::from_str(&secrets::read(SECRET)?).ok()
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(FILE))
}

fn load_local(app: &AppHandle) -> Local {
    path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_local(app: &AppHandle, local: &Local) -> Result<(), String> {
    let p = path(app)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_string(local).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn enc(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

// ---- tokens ------------------------------------------------------------------------

async fn token_request(form: &[(&str, &str)]) -> Result<Value, String> {
    let res = net::client()
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("Нет связи с Google: {e}"))?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if ok {
        return Ok(v);
    }
    Err(match v["error"].as_str() {
        Some("invalid_grant") => {
            "Вход в Google истёк. Войдите снова; чтобы это не повторялось каждые 7 дней, переведите OAuth-приложение в режим «В продакшене».".into()
        }
        Some("invalid_client") => "Неверный Client ID или Client Secret.".into(),
        _ => format!("Google: {}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("ошибка входа")),
    })
}

fn remember_access(v: &Value) {
    if let Some(token) = v["access_token"].as_str() {
        let ttl = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
        *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = Some((token.to_string(), Instant::now() + Duration::from_secs(ttl)));
    }
}

async fn access_token() -> Result<String, String> {
    if let Some((token, until)) = ACCESS.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if Instant::now() < until {
            return Ok(token);
        }
    }
    let s = stored().ok_or("Google Календарь не подключён")?;
    let v = token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &s.refresh_token),
        ("client_id", &s.client_id),
        ("client_secret", &s.client_secret),
    ])
    .await?;
    remember_access(&v);
    v["access_token"].as_str().map(str::to_string).ok_or("Google не выдал токен".into())
}

async fn api(method: Method, url: &str, body: Option<Value>) -> Result<Value, String> {
    let token = access_token().await?;
    let mut req = net::client().request(method, url).bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await.map_err(|e| format!("Нет связи с Google: {e}"))?;
    let status = res.status();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(v);
    }
    if status == StatusCode::UNAUTHORIZED {
        *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    let msg = v["error"]["message"].as_str().unwrap_or("");
    Err(match status {
        StatusCode::FORBIDDEN if msg.contains("has not been used") || msg.contains("disabled") => {
            "В Google Cloud не включён Calendar API или Tasks API для этого проекта.".into()
        }
        StatusCode::FORBIDDEN => format!("Google отказал в доступе: {msg}"),
        StatusCode::NOT_FOUND => "Событие не найдено: возможно, его уже удалили".into(),
        StatusCode::GONE => "Событие уже удалено".into(),
        s => format!("Google {}: {msg}", s.as_u16()),
    })
}

// ---- sign-in -----------------------------------------------------------------------

#[tauri::command]
pub async fn gcal_login(app: AppHandle, client_id: String, client_secret: String) -> Result<String, String> {
    let (client_id, client_secret) = (client_id.trim().to_string(), client_secret.trim().to_string());
    if !client_id.ends_with(".apps.googleusercontent.com") || client_secret.is_empty() {
        return Err("Вставьте Client ID (…apps.googleusercontent.com) и Client Secret из Google Cloud".into());
    }
    // Desktop clients accept any loopback port.
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let redirect = format!("http://127.0.0.1:{}/callback", listener.local_addr().map_err(|e| e.to_string())?.port());
    let (verifier, challenge) = oauth::pkce()?;
    let state = oauth::random_urlsafe(16)?;
    let auth = reqwest::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            // A refresh token every time, even when the app was allowed before.
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map_err(|e| e.to_string())?;
    tauri_plugin_opener::OpenerExt::opener(&app).open_url(auth.as_str(), None::<&str>).map_err(|e| e.to_string())?;

    let query = tauri::async_runtime::spawn_blocking(move || oauth::wait_for_redirect(listener, "/callback"))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(err) = oauth::query_param(&query, "error") {
        return Err(if err == "access_denied" { "Вход отменён".into() } else { format!("Google: {err}") });
    }
    if oauth::query_param(&query, "state").as_deref() != Some(state.as_str()) {
        return Err("Ответ Google не прошёл проверку, попробуйте ещё раз".into());
    }
    let code = oauth::query_param(&query, "code").ok_or("Google не вернул код входа")?;
    let v = token_request(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("client_id", &client_id),
        ("client_secret", &client_secret),
        ("code_verifier", &verifier),
    ])
    .await?;
    let refresh_token = v["refresh_token"].as_str().ok_or("Google не выдал refresh token")?.to_string();
    let stored = Stored { client_id, client_secret, refresh_token };
    secrets::write(SECRET, &serde_json::to_string(&stored).map_err(|e| e.to_string())?)?;
    remember_access(&v);
    primary_email().await
}

/// The primary calendar's ID is the account's address.
async fn primary_email() -> Result<String, String> {
    let v = api(Method::GET, &format!("{CALENDAR_API}/calendars/primary"), None).await?;
    Ok(v["id"].as_str().unwrap_or("Google").to_string())
}

#[derive(Serialize)]
pub struct Status {
    connected: bool,
    email: Option<String>,
    error: Option<String>,
}

#[tauri::command]
pub async fn gcal_status() -> Status {
    if stored().is_none() {
        return Status { connected: false, email: None, error: None };
    }
    match primary_email().await {
        Ok(email) => Status { connected: true, email: Some(email), error: None },
        Err(e) => Status { connected: true, email: None, error: Some(e) },
    }
}

#[tauri::command]
pub fn gcal_logout() {
    secrets::delete(SECRET);
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// ---- calendars ---------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    id: String,
    name: String,
    color: String,
    primary: bool,
    writable: bool,
    visible: bool,
}

async fn calendars(app: &AppHandle) -> Result<Vec<Calendar>, String> {
    let v = api(Method::GET, &format!("{CALENDAR_API}/users/me/calendarList?maxResults=250"), None).await?;
    let local = load_local(app);
    let mut list: Vec<Calendar> = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["deleted"] != true)
        .filter_map(|c| {
            let id = c["id"].as_str()?.to_string();
            let default = c["selected"] == true || c["primary"] == true;
            Some(Calendar {
                visible: if local.hidden.contains(&id) { false } else { default || local.shown.contains(&id) },
                name: c["summaryOverride"].as_str().or(c["summary"].as_str()).unwrap_or(&id).to_string(),
                color: c["backgroundColor"].as_str().unwrap_or("#4285f4").to_string(),
                primary: c["primary"] == true,
                writable: matches!(c["accessRole"].as_str(), Some("owner" | "writer")),
                id,
            })
        })
        .collect();
    list.sort_by_key(|c| (!c.primary, c.name.to_lowercase()));
    Ok(list)
}

#[tauri::command]
pub async fn gcal_calendars(app: AppHandle) -> Result<Vec<Calendar>, String> {
    calendars(&app).await
}

#[tauri::command]
pub fn gcal_set_visible(app: AppHandle, id: String, visible: bool) -> Result<(), String> {
    let mut local = load_local(&app);
    local.hidden.remove(&id);
    local.shown.remove(&id);
    if visible {
        local.shown.insert(id);
    } else {
        local.hidden.insert(id);
    }
    save_local(&app, &local)
}

// ---- events ------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    calendar_id: String,
    id: String,
    title: String,
    /// RFC 3339 date-time, or YYYY-MM-DD for all-day events.
    start: String,
    /// Exclusive, same format as `start`.
    end: String,
    all_day: bool,
    color: String,
    location: Option<String>,
    description: Option<String>,
    html_link: Option<String>,
    meet_link: Option<String>,
    recurring: bool,
    editable: bool,
}

fn event_from(v: &Value, cal: &Calendar) -> Option<Event> {
    if v["status"] == "cancelled" {
        return None;
    }
    let all_day = v["start"]["date"].is_string();
    let field = |k: &str| v[k]["dateTime"].as_str().or(v[k]["date"].as_str()).map(str::to_string);
    let color = v["colorId"]
        .as_str()
        .and_then(|c| c.parse::<usize>().ok())
        .and_then(|i| EVENT_COLORS.get(i.wrapping_sub(1)))
        .map(|c| c.to_string())
        .unwrap_or_else(|| cal.color.clone());
    let meet = v["hangoutLink"].as_str().map(str::to_string).or_else(|| {
        v["conferenceData"]["entryPoints"]
            .as_array()?
            .iter()
            .find(|e| e["entryPointType"] == "video")?["uri"]
            .as_str()
            .map(str::to_string)
    });
    Some(Event {
        calendar_id: cal.id.clone(),
        id: v["id"].as_str()?.to_string(),
        title: v["summary"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or("(Без названия)").to_string(),
        start: field("start")?,
        end: field("end")?,
        all_day,
        color,
        location: v["location"].as_str().map(str::to_string),
        description: v["description"].as_str().map(str::to_string),
        html_link: v["htmlLink"].as_str().map(str::to_string),
        meet_link: meet,
        recurring: v["recurringEventId"].is_string(),
        editable: cal.writable && v["locked"] != true,
    })
}

async fn calendar_events(cal: &Calendar, time_min: &str, time_max: &str) -> Result<Vec<Event>, String> {
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut url = format!(
            "{CALENDAR_API}/calendars/{}/events?singleEvents=true&orderBy=startTime&maxResults=2500&timeMin={}&timeMax={}",
            enc(&cal.id),
            enc(time_min),
            enc(time_max)
        );
        if let Some(p) = &page {
            url.push_str(&format!("&pageToken={}", enc(p)));
        }
        let v = api(Method::GET, &url, None).await?;
        out.extend(v["items"].as_array().into_iter().flatten().filter_map(|e| event_from(e, cal)));
        match v["nextPageToken"].as_str() {
            Some(p) => page = Some(p.to_string()),
            None => return Ok(out),
        }
    }
}

/// Events of every visible calendar between two RFC 3339 instants.
#[tauri::command]
pub async fn gcal_events(app: AppHandle, time_min: String, time_max: String) -> Result<Vec<Event>, String> {
    let cals: Vec<Calendar> = calendars(&app).await?.into_iter().filter(|c| c.visible).collect();
    let results = futures_util::future::join_all(cals.iter().map(|c| calendar_events(c, &time_min, &time_max))).await;
    let mut events = Vec::new();
    for r in results {
        events.extend(r?);
    }
    events.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(events)
}

async fn calendar(app: &AppHandle, id: &str) -> Result<Calendar, String> {
    calendars(app).await?.into_iter().find(|c| c.id == id).ok_or_else(|| "Календарь не найден".into())
}

/// Creates an event from a Google event body (summary, start, end...).
#[tauri::command]
pub async fn gcal_create(app: AppHandle, calendar_id: String, event: Value) -> Result<Event, String> {
    let cal = calendar(&app, &calendar_id).await?;
    let v = api(Method::POST, &format!("{CALENDAR_API}/calendars/{}/events", enc(&calendar_id)), Some(event)).await?;
    event_from(&v, &cal).ok_or_else(|| "Google вернул непонятное событие".into())
}

/// Changes some fields of an event (a single occurrence, for recurring ones).
#[tauri::command]
pub async fn gcal_update(app: AppHandle, calendar_id: String, event_id: String, patch: Value) -> Result<Event, String> {
    let cal = calendar(&app, &calendar_id).await?;
    let url = format!("{CALENDAR_API}/calendars/{}/events/{}", enc(&calendar_id), enc(&event_id));
    let v = api(Method::PATCH, &url, Some(patch)).await?;
    event_from(&v, &cal).ok_or_else(|| "Google вернул непонятное событие".into())
}

#[tauri::command]
pub async fn gcal_delete(calendar_id: String, event_id: String) -> Result<(), String> {
    let url = format!("{CALENDAR_API}/calendars/{}/events/{}", enc(&calendar_id), enc(&event_id));
    match api(Method::DELETE, &url, None).await {
        // Already gone is what we wanted.
        Err(e) if e.contains("удален") => Ok(()),
        r => r.map(drop),
    }
}

// ---- tasks -------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    list_id: String,
    list: String,
    id: String,
    title: String,
    /// YYYY-MM-DD; Google Tasks keeps only the date.
    due: Option<String>,
    notes: Option<String>,
}

/// Unfinished tasks of every list.
#[tauri::command]
pub async fn gcal_tasks() -> Result<Vec<Task>, String> {
    let lists = api(Method::GET, &format!("{TASKS_API}/users/@me/lists?maxResults=100"), None).await?;
    let lists: Vec<(String, String)> = lists["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["id"].as_str()?.to_string(), l["title"].as_str().unwrap_or_default().to_string())))
        .collect();
    let urls: Vec<String> = lists
        .iter()
        .map(|(id, _)| format!("{TASKS_API}/lists/{}/tasks?showCompleted=false&showHidden=false&maxResults=100", enc(id)))
        .collect();
    let results = futures_util::future::join_all(urls.iter().map(|u| api(Method::GET, u, None))).await;
    let mut tasks = Vec::new();
    for ((list_id, list), r) in lists.iter().zip(results) {
        for t in r?["items"].as_array().into_iter().flatten() {
            let (Some(id), Some(title)) = (t["id"].as_str(), t["title"].as_str()) else { continue };
            if title.trim().is_empty() {
                continue;
            }
            tasks.push(Task {
                list_id: list_id.clone(),
                list: list.clone(),
                id: id.to_string(),
                title: title.to_string(),
                due: t["due"].as_str().map(|d| d.chars().take(10).collect()),
                notes: t["notes"].as_str().map(str::to_string),
            });
        }
    }
    tasks.sort_by(|a, b| a.due.cmp(&b.due));
    Ok(tasks)
}

#[tauri::command]
pub async fn gcal_task_done(list_id: String, task_id: String, done: bool) -> Result<(), String> {
    let url = format!("{TASKS_API}/lists/{}/tasks/{}", enc(&list_id), enc(&task_id));
    let body = if done { json!({ "status": "completed" }) } else { json!({ "status": "needsAction", "completed": null }) };
    api(Method::PATCH, &url, Some(body)).await.map(drop)
}
