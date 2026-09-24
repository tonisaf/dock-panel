//! "Дом" widget: Yeelight lamps and Google Cast speakers, all over the local
//! network, no cloud. Found devices are remembered in `home.json`, so the
//! widget shows them at once and only searches again when one goes missing
//! (a new IP from the router) or the user asks.

mod cast;
mod yeelight;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE: &str = "home.json";
const YEELIGHT_WAIT: Duration = Duration::from_millis(1500);
const CAST_WAIT: Duration = Duration::from_millis(2500);
/// A missing device triggers a new search at most this often.
const AUTO_RESCAN: Duration = Duration::from_secs(5 * 60);

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    lamps: HashMap<String, yeelight::Known>,
    speakers: HashMap<String, cast::Known>,
    /// Names given in the panel; Yeelight lamps don't report the app's names.
    names: HashMap<String, String>,
}

static LAST_SCAN: Mutex<Option<Instant>> = Mutex::new(None);

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(FILE))
}

fn load(app: &AppHandle) -> Saved {
    path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, saved: &Saved) -> Result<(), String> {
    let p = path(app)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// IPv4 addresses of this PC's LAN interfaces: no loopback, no VPN tunnels.
fn lan_ips() -> Vec<Ipv4Addr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && !i.is_p2p() && i.is_oper_up())
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) => Some(v4.ip),
            _ => None,
        })
        .collect()
}

/// Searches both kinds of devices at once and merges them into `saved`
/// (new IPs replace old ones; devices not heard from are kept).
fn scan(saved: &mut Saved) {
    let ips = lan_ips();
    let (lamps, speakers) = std::thread::scope(|s| {
        let lamps = s.spawn(|| yeelight::discover(&ips, YEELIGHT_WAIT));
        let speakers = s.spawn(|| cast::discover(CAST_WAIT));
        (lamps.join().unwrap_or_default(), speakers.join().unwrap_or_default())
    });
    saved.lamps.extend(lamps);
    saved.speakers.extend(speakers);
    *LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lamp {
    id: String,
    name: String,
    model: String,
    supports_ct: bool,
    supports_rgb: bool,
    ct_min: u16,
    ct_max: u16,
    /// `None` when the lamp didn't answer.
    state: Option<yeelight::LampState>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    id: String,
    name: String,
    model: String,
    state: Option<cast::SpeakerState>,
}

#[derive(Serialize)]
pub struct HomeState {
    lamps: Vec<Lamp>,
    speakers: Vec<Speaker>,
}

fn lamp_name(saved: &Saved, id: &str, known: &yeelight::Known) -> String {
    saved.names.get(id).cloned().unwrap_or_else(|| match known.model.as_str() {
        "" => "Лампа Yeelight".into(),
        m => format!("Yeelight {m}"),
    })
}

fn lamp(saved: &Saved, id: &str, known: &yeelight::Known, state: Option<yeelight::LampState>) -> Lamp {
    let supports = |m: &str| known.support.iter().any(|s| s == m);
    Lamp {
        id: id.to_string(),
        name: lamp_name(saved, id, known),
        model: known.model.clone(),
        supports_ct: supports("set_ct_abx"),
        supports_rgb: supports("set_rgb"),
        ct_min: yeelight::CT_MIN,
        ct_max: yeelight::CT_MAX,
        state,
    }
}

fn speaker(saved: &Saved, id: &str, known: &cast::Known, state: Option<cast::SpeakerState>) -> Speaker {
    Speaker {
        id: id.to_string(),
        name: saved.names.get(id).cloned().unwrap_or_else(|| known.name.clone()),
        model: known.model.clone(),
        state,
    }
}

/// Polls every known device in parallel.
fn snapshot(saved: &Saved) -> HomeState {
    std::thread::scope(|s| {
        let lamps: Vec<_> = saved
            .lamps
            .iter()
            .map(|(id, k)| (id, k, s.spawn(move || yeelight::state(k).ok())))
            .collect();
        let speakers: Vec<_> = saved
            .speakers
            .iter()
            .map(|(id, k)| (id, k, s.spawn(move || cast::state(k).ok())))
            .collect();
        let mut lamps: Vec<Lamp> =
            lamps.into_iter().map(|(id, k, h)| lamp(saved, id, k, h.join().ok().flatten())).collect();
        let mut speakers: Vec<Speaker> =
            speakers.into_iter().map(|(id, k, h)| speaker(saved, id, k, h.join().ok().flatten())).collect();
        lamps.sort_by_cached_key(|l| l.name.to_lowercase());
        speakers.sort_by_cached_key(|s| s.name.to_lowercase());
        HomeState { lamps, speakers }
    })
}

fn state_blocking(app: &AppHandle, rescan: bool) -> Result<HomeState, String> {
    let mut saved = load(app);
    let never = saved.lamps.is_empty() && saved.speakers.is_empty();
    if rescan || never {
        scan(&mut saved);
        save(app, &saved)?;
    }
    let state = snapshot(&saved);

    let missing = state.lamps.iter().any(|l| l.state.is_none()) || state.speakers.iter().any(|s| s.state.is_none());
    let stale = LAST_SCAN.lock().unwrap_or_else(|e| e.into_inner()).is_none_or(|t| t.elapsed() > AUTO_RESCAN);
    if missing && stale {
        scan(&mut saved);
        save(app, &saved)?;
        return Ok(snapshot(&saved));
    }
    Ok(state)
}

async fn blocking<R: Send + 'static>(f: impl FnOnce() -> Result<R, String> + Send + 'static) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// Every known device with its current state; `rescan` searches the network first.
#[tauri::command]
pub async fn home_state(app: AppHandle, rescan: bool) -> Result<HomeState, String> {
    blocking(move || state_blocking(&app, rescan)).await
}

#[tauri::command]
pub async fn home_lamp_set(app: AppHandle, id: String, change: yeelight::Change) -> Result<Lamp, String> {
    blocking(move || {
        let saved = load(&app);
        let known = saved.lamps.get(&id).ok_or("лампа не найдена")?;
        yeelight::apply(known, &change)?;
        let state = yeelight::state(known).ok();
        Ok(lamp(&saved, &id, known, state))
    })
    .await
}

#[tauri::command]
pub async fn home_speaker_control(app: AppHandle, id: String, control: cast::Control) -> Result<Speaker, String> {
    blocking(move || {
        let saved = load(&app);
        let known = saved.speakers.get(&id).ok_or("колонка не найдена")?;
        cast::control(known, &control)?;
        let state = cast::state(known).ok();
        Ok(speaker(&saved, &id, known, state))
    })
    .await
}

/// Renames a device in the panel; an empty name goes back to the default.
#[tauri::command]
pub async fn home_rename(app: AppHandle, id: String, name: String) -> Result<(), String> {
    blocking(move || {
        let mut saved = load(&app);
        match name.trim() {
            "" => saved.names.remove(&id),
            n => saved.names.insert(id, n.to_string()),
        };
        save(&app, &saved)
    })
    .await
}

/// Forgets a device that is gone for good.
#[tauri::command]
pub async fn home_forget(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || {
        let mut saved = load(&app);
        saved.lamps.remove(&id);
        saved.speakers.remove(&id);
        saved.names.remove(&id);
        save(&app, &saved)
    })
    .await
}
