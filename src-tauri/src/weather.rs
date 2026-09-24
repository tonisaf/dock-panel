//! Weather and city search with a fallback for networks where Open-Meteo is
//! blocked (it is in Russia without a VPN).
//!
//! Open-Meteo is tried first with a short timeout. If it fails, the
//! fallbacks are used and Open-Meteo is skipped for a while, so a blocked
//! network doesn't wait out the timeout on every refresh:
//! - forecast: MET Norway (api.met.no), free, no key;
//! - city search: OpenStreetMap Nominatim, free, no key, at most 1 request/s.
//!
//! Both fallbacks require a User-Agent that identifies the app. Rust only
//! fetches; the UI converts the fallback responses to the Open-Meteo shape.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::USER_AGENT;
use serde::Serialize;
use serde_json::Value;

use crate::net;

const OPEN_METEO_TIMEOUT: Duration = Duration::from_secs(5);
/// After Open-Meteo fails, go straight to the fallbacks for this long.
const OPEN_METEO_BACKOFF_MS: u64 = 30 * 60 * 1000;
const NOMINATIM_INTERVAL: Duration = Duration::from_millis(1100);
const APP_AGENT: &str = concat!("DockPanel/", env!("CARGO_PKG_VERSION"), " (+https://github.com/tonisaf/dock-panel)");

static OPEN_METEO_SKIP_UNTIL: AtomicU64 = AtomicU64::new(0);
static NOMINATIM_LAST: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Serialize)]
pub struct Sourced {
    /// "open-meteo", "met.no" or "nominatim": tells the UI how to read `body`.
    source: &'static str,
    body: Value,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

async fn get_json(url: &str, query: &[(&str, String)], timeout: Option<Duration>) -> Result<Value, String> {
    let mut req = net::client().get(url).query(query).header(USER_AGENT, APP_AGENT);
    if let Some(t) = timeout {
        req = req.timeout(t);
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    res.json().await.map_err(|e| e.to_string())
}

/// Open-Meteo unless it failed recently; `None` means "use the fallback".
async fn try_open_meteo(url: &str, query: &[(&str, String)]) -> Option<Value> {
    if now_ms() < OPEN_METEO_SKIP_UNTIL.load(Ordering::Relaxed) {
        return None;
    }
    match get_json(url, query, Some(OPEN_METEO_TIMEOUT)).await {
        Ok(body) => Some(body),
        Err(e) => {
            eprintln!("open-meteo unavailable, using fallback: {e}");
            OPEN_METEO_SKIP_UNTIL.store(now_ms() + OPEN_METEO_BACKOFF_MS, Ordering::Relaxed);
            None
        }
    }
}

#[tauri::command]
pub async fn weather_forecast(latitude: f64, longitude: f64) -> Result<Sourced, String> {
    let open_meteo = [
        ("latitude", latitude.to_string()),
        ("longitude", longitude.to_string()),
        ("current", "temperature_2m,apparent_temperature,weather_code,is_day,wind_speed_10m,relative_humidity_2m".into()),
        ("hourly", "temperature_2m,weather_code,is_day".into()),
        ("daily", "temperature_2m_max,temperature_2m_min".into()),
        ("timezone", "auto".into()),
        ("forecast_days", "2".into()),
        ("wind_speed_unit", "ms".into()),
    ];
    if let Some(body) = try_open_meteo("https://api.open-meteo.com/v1/forecast", &open_meteo).await {
        return Ok(Sourced { source: "open-meteo", body });
    }

    // MET Norway rejects coordinates with more than 4 decimals.
    let met = [("lat", format!("{latitude:.4}")), ("lon", format!("{longitude:.4}"))];
    let body = get_json("https://api.met.no/weatherapi/locationforecast/2.0/compact", &met, None).await?;
    Ok(Sourced { source: "met.no", body })
}

#[tauri::command]
pub async fn weather_geocode(query: String) -> Result<Sourced, String> {
    let open_meteo = [
        ("name", query.clone()),
        ("count", "6".into()),
        ("language", "ru".into()),
        ("format", "json".into()),
    ];
    if let Some(body) = try_open_meteo("https://geocoding-api.open-meteo.com/v1/search", &open_meteo).await {
        return Ok(Sourced { source: "open-meteo", body });
    }

    // Nominatim's usage policy: no more than one request per second.
    let wait = {
        let mut last = NOMINATIM_LAST.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let next = last.map_or(now, |t| (t + NOMINATIM_INTERVAL).max(now));
        *last = Some(next);
        next - now
    };
    pause(wait).await;

    let nominatim = [
        ("q", query),
        ("format", "jsonv2".into()),
        ("addressdetails", "1".into()),
        ("limit", "6".into()),
        ("accept-language", "ru".into()),
        ("featureType", "settlement".into()),
    ];
    let body = get_json("https://nominatim.openstreetmap.org/search", &nominatim, None).await?;
    Ok(Sourced { source: "nominatim", body })
}

async fn pause(d: Duration) {
    if !d.is_zero() {
        let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
    }
}
