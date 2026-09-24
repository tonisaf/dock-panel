//! Calendars from secret iCal (.ics) links: Google, Outlook, Yandex, iCloud.
//! Rust only stores the links and fetches the feeds; parsing and recurrence
//! expansion happen in the UI with ical.js.
//!
//! The links grant read access to the calendar, so they stay in our app data
//! file and are never sent back to the webview.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::net;

const FILE: &str = "calendars.json";
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const PALETTE: [&str; 6] = ["#8ab4ff", "#6fcf97", "#f5a55b", "#f28fb5", "#b794f4", "#e9c46a"];

#[derive(Serialize, Deserialize, Clone)]
struct Stored {
    id: String,
    name: String,
    color: String,
    url: String,
}

#[derive(Serialize)]
pub struct CalendarInfo {
    id: String,
    name: String,
    color: String,
}

impl From<&Stored> for CalendarInfo {
    fn from(s: &Stored) -> Self {
        CalendarInfo { id: s.id.clone(), name: s.name.clone(), color: s.color.clone() }
    }
}

static CACHE: Mutex<Option<HashMap<String, (Instant, String)>>> = Mutex::new(None);

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(FILE))
}

fn load(app: &AppHandle) -> Vec<Stored> {
    path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, calendars: &[Stored]) -> Result<(), String> {
    let p = path(app)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(calendars).map_err(|e| e.to_string())?;
    std::fs::write(p, text).map_err(|e| e.to_string())
}

async fn fetch(url: &str) -> Result<String, String> {
    let res = net::client().get(url).send().await.map_err(|e| format!("Нет связи с календарём: {e}"))?;
    let status = res.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 | 404 => "Ссылка не работает: проверьте, что скопировали секретный адрес целиком.".into(),
            code => format!("Календарь ответил ошибкой {code}"),
        });
    }
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !text.contains("BEGIN:VCALENDAR") {
        return Err("По ссылке не календарь. Нужен адрес в формате iCal (.ics).".into());
    }
    Ok(text)
}

/// `X-WR-CALNAME` from the feed, the calendar's display name in Google/Outlook.
fn feed_name(ics: &str) -> Option<String> {
    ics.lines()
        .find_map(|l| l.strip_prefix("X-WR-CALNAME:"))
        .map(|n| n.trim().replace("\\,", ",").replace("\\;", ";"))
        .filter(|n| !n.is_empty())
}

#[tauri::command]
pub fn calendar_list(app: AppHandle) -> Vec<CalendarInfo> {
    load(&app).iter().map(CalendarInfo::from).collect()
}

#[tauri::command]
pub async fn calendar_add(app: AppHandle, url: String) -> Result<CalendarInfo, String> {
    let url = url.trim();
    let url = match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    };
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Вставьте ссылку, начинающуюся с https:// или webcal://".into());
    }

    let ics = fetch(&url).await?;
    let mut calendars = load(&app);
    if calendars.iter().any(|c| c.url == url) {
        return Err("Этот календарь уже добавлен".into());
    }
    let host = url.split('/').nth(2).unwrap_or("Календарь").to_string();
    let id = format!(
        "{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
    );
    let stored = Stored {
        id: id.clone(),
        name: feed_name(&ics).unwrap_or(host),
        color: PALETTE[calendars.len() % PALETTE.len()].into(),
        url,
    };
    let info = CalendarInfo::from(&stored);
    calendars.push(stored);
    save(&app, &calendars)?;
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id, (Instant::now(), ics));
    Ok(info)
}

#[tauri::command]
pub fn calendar_remove(app: AppHandle, id: String) -> Result<(), String> {
    let mut calendars = load(&app);
    calendars.retain(|c| c.id != id);
    save(&app, &calendars)?;
    if let Some(cache) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        cache.remove(&id);
    }
    Ok(())
}

/// Raw iCal text of one calendar, cached for a few minutes.
#[tauri::command]
pub async fn calendar_ics(app: AppHandle, id: String) -> Result<String, String> {
    let cached = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(&id)
        .filter(|(t, _)| t.elapsed() < CACHE_TTL)
        .map(|(_, ics)| ics.clone());
    if let Some(ics) = cached {
        return Ok(ics);
    }
    let url = load(&app).into_iter().find(|c| c.id == id).ok_or("Календарь не найден")?.url;
    let ics = fetch(&url).await?;
    CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(id, (Instant::now(), ics.clone()));
    Ok(ics)
}
