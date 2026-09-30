//! Lamps from the Yandex smart home ("Дом с Алисой") through its official
//! API: any light the account has there, whatever brand's skill brought it in
//! (e.g. Kojima, which has no local protocol of its own to speak). Cloud, not
//! LAN, so states come from Yandex and changes take a round trip through it.
//!
//! Sign-in is OAuth with the user's own app from oauth.yandex.ru (scopes
//! `iot:view` and `iot:control`) and a loopback redirect. Yandex matches the
//! redirect URI exactly, so the port is fixed. The refresh token and the
//! client live in Credential Manager.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use super::yeelight::{LampState, Light};
use crate::{net, oauth, secrets};

const AUTH_URL: &str = "https://oauth.yandex.ru/authorize";
const TOKEN_URL: &str = "https://oauth.yandex.ru/token";
const API: &str = "https://api.iot.yandex.net/v1.0";
const SECRET: &str = "DockPanel/yandex";
pub const REDIRECT_PORT: u16 = 43822;
/// Used when a lamp doesn't report its colour temperature range.
const CT_RANGE: (u16, u16) = (2700, 6500);

#[derive(Serialize, Deserialize)]
struct Stored {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

static ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);
/// Lamps from the last listing, so a change knows the lamp's colour model and
/// current state without asking Yandex first.
static DEVICES: Mutex<Option<HashMap<String, Device>>> = Mutex::new(None);

fn stored() -> Option<Stored> {
    serde_json::from_str(&secrets::read(SECRET)?).ok()
}

pub fn connected() -> bool {
    stored().is_some()
}

// ---- tokens ------------------------------------------------------------------------

async fn token_request(form: &[(&str, &str)]) -> Result<Value, String> {
    let res = net::client()
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("Нет связи с Яндексом: {e}"))?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if ok {
        return Ok(v);
    }
    Err(match v["error"].as_str() {
        Some("invalid_grant") => "Вход в Яндекс истёк или отозван. Войдите снова.".into(),
        Some("invalid_client") => "Неверный ClientID или Client secret.".into(),
        _ => format!(
            "Яндекс: {}",
            v["error_description"]
                .as_str()
                .or(v["error"].as_str())
                .unwrap_or("ошибка входа")
        ),
    })
}

fn remember_access(v: &Value) {
    if let Some(token) = v["access_token"].as_str() {
        let ttl = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
        *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((token.to_string(), Instant::now() + Duration::from_secs(ttl)));
    }
}

async fn access_token() -> Result<String, String> {
    if let Some((token, until)) = ACCESS.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if Instant::now() < until {
            return Ok(token);
        }
    }
    let s = stored().ok_or("Яндекс не подключён")?;
    let v = token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &s.refresh_token),
        ("client_id", &s.client_id),
        ("client_secret", &s.client_secret),
    ])
    .await?;
    // Yandex may hand out a new refresh token; the old one then stops working.
    if let Some(refresh_token) = v["refresh_token"]
        .as_str()
        .filter(|t| *t != s.refresh_token)
    {
        let s = Stored {
            refresh_token: refresh_token.to_string(),
            ..s
        };
        secrets::write(
            SECRET,
            &serde_json::to_string(&s).map_err(|e| e.to_string())?,
        )?;
    }
    remember_access(&v);
    v["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or("Яндекс не выдал токен".into())
}

async fn api(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let token = access_token().await?;
    let mut req = net::client()
        .request(method, format!("{API}{path}"))
        .bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req
        .send()
        .await
        .map_err(|e| format!("Нет связи с Яндексом: {e}"))?;
    let status = res.status();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if status.is_success() && v["status"].as_str() != Some("error") {
        return Ok(v);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return Err(
            "Яндекс не пустил: войдите снова в настройках (нужны права «Умный дом»)".into(),
        );
    }
    Err(format!(
        "Яндекс {}: {}",
        status.as_u16(),
        v["message"].as_str().unwrap_or("ошибка")
    ))
}

// ---- sign-in -----------------------------------------------------------------------

#[tauri::command]
pub async fn yandex_login(
    app: AppHandle,
    client_id: String,
    client_secret: String,
) -> Result<usize, String> {
    let (client_id, client_secret) = (
        client_id.trim().to_string(),
        client_secret.trim().to_string(),
    );
    if client_id.len() != 32 || client_secret.is_empty() {
        return Err(
            "Вставьте ClientID (32 символа) и Client secret приложения с oauth.yandex.ru".into(),
        );
    }
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .map_err(|e| format!("Порт {REDIRECT_PORT} занят, вход невозможен: {e}"))?;
    let redirect = format!("http://127.0.0.1:{REDIRECT_PORT}/callback");
    let (verifier, challenge) = oauth::pkce()?;
    let state = oauth::random_urlsafe(16)?;
    let auth = reqwest::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            // Lets the user pick the account instead of silently using the last one.
            ("force_confirm", "yes"),
        ],
    )
    .map_err(|e| e.to_string())?;
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(auth.as_str(), None::<&str>)
        .map_err(|e| e.to_string())?;

    let query = tauri::async_runtime::spawn_blocking(move || {
        oauth::wait_for_redirect(listener, "/callback")
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(err) = oauth::query_param(&query, "error") {
        return Err(if err == "access_denied" {
            "Вход отменён".into()
        } else {
            format!("Яндекс: {err}")
        });
    }
    if oauth::query_param(&query, "state").as_deref() != Some(state.as_str()) {
        return Err("Ответ Яндекса не прошёл проверку, попробуйте ещё раз".into());
    }
    let code = oauth::query_param(&query, "code").ok_or("Яндекс не вернул код входа")?;
    let v = token_request(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("client_id", &client_id),
        ("client_secret", &client_secret),
        ("code_verifier", &verifier),
    ])
    .await?;
    let refresh_token = v["refresh_token"]
        .as_str()
        .ok_or("Яндекс не выдал refresh token")?
        .to_string();
    let stored = Stored {
        client_id,
        client_secret,
        refresh_token,
    };
    secrets::write(
        SECRET,
        &serde_json::to_string(&stored).map_err(|e| e.to_string())?,
    )?;
    remember_access(&v);
    Ok(devices().await?.len())
}

#[derive(Serialize)]
pub struct Status {
    connected: bool,
    /// Lights in the Yandex home.
    lamps: usize,
    error: Option<String>,
}

#[tauri::command]
pub async fn yandex_status() -> Status {
    if !connected() {
        return Status {
            connected: false,
            lamps: 0,
            error: None,
        };
    }
    match devices().await {
        Ok(d) => Status {
            connected: true,
            lamps: d.len(),
            error: None,
        },
        Err(e) => Status {
            connected: true,
            lamps: 0,
            error: Some(e),
        },
    }
}

#[tauri::command]
pub fn yandex_logout() {
    secrets::delete(SECRET);
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *DEVICES.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// ---- devices -----------------------------------------------------------------------

#[derive(Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub room: Option<String>,
    pub supports_ct: bool,
    pub supports_rgb: bool,
    pub ct_min: u16,
    pub ct_max: u16,
    /// "hsv" or "rgb": how the lamp takes colours.
    color_model: Option<String>,
    /// `None` when Yandex doesn't know the lamp's state (it is offline).
    pub state: Option<LampState>,
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> u32 {
    let (s, v) = (s / 100.0, v / 100.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let byte = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// Hue 0–360, saturation and value 0–100, as Yandex wants them.
fn rgb_to_hsv(rgb: u32) -> (u32, u32, u32) {
    let [r, g, b] = [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF].map(|c| c as f64 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (
        h.round() as u32 % 360,
        (s * 100.0).round() as u32,
        (max * 100.0).round() as u32,
    )
}

fn capability<'a>(dev: &'a Value, kind: &str, instance: Option<&str>) -> Option<&'a Value> {
    dev["capabilities"].as_array()?.iter().find(|c| {
        c["type"].as_str() == Some(kind)
            && instance.is_none_or(|i| {
                c["parameters"]["instance"].as_str() == Some(i)
                    || c["state"]["instance"].as_str() == Some(i)
            })
    })
}

fn parse(dev: &Value, rooms: &HashMap<String, String>) -> Option<Device> {
    if !dev["type"].as_str()?.starts_with("devices.types.light") {
        return None;
    }
    let color = capability(dev, "devices.capabilities.color_setting", None);
    let params = color.map(|c| &c["parameters"]);
    let color_model = params
        .and_then(|p| p["color_model"].as_str())
        .map(str::to_string);
    let ct = params
        .map(|p| &p["temperature_k"])
        .filter(|t| t.is_object());
    let (ct_min, ct_max) = ct.map_or(CT_RANGE, |t| {
        (
            t["min"].as_u64().unwrap_or(CT_RANGE.0 as u64) as u16,
            t["max"].as_u64().unwrap_or(CT_RANGE.1 as u64) as u16,
        )
    });

    let power = capability(dev, "devices.capabilities.on_off", None)
        .and_then(|c| c["state"]["value"].as_bool());
    let state = power.map(|power| {
        let bright = capability(dev, "devices.capabilities.range", Some("brightness"))
            .and_then(|c| c["state"]["value"].as_f64())
            .map_or(100, |b| b.round().clamp(1.0, 100.0) as u8);
        let mut light = Light {
            power,
            bright,
            ct: (ct_min + ct_max) / 2,
            rgb: 0xFF_FF_FF,
            color_mode: 2,
        };
        let s = color.map(|c| &c["state"]);
        match s.and_then(|s| s["instance"].as_str()) {
            Some("temperature_k") => {
                light.ct = s
                    .and_then(|s| s["value"].as_u64())
                    .map_or(light.ct, |k| k as u16)
            }
            Some("rgb") => {
                light.rgb = s.and_then(|s| s["value"].as_u64()).unwrap_or(0xFF_FF_FF) as u32;
                light.color_mode = 1;
            }
            Some("hsv") => {
                let v = &s.expect("instance came from it")["value"];
                let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
                // The panel shows brightness separately; the colour itself is at full value.
                light.rgb = hsv_to_rgb(f("h"), f("s"), 100.0);
                light.color_mode = 1;
            }
            _ => {}
        }
        LampState {
            main: light,
            bg: None,
        }
    });

    Some(Device {
        id: dev["id"].as_str()?.to_string(),
        name: dev["name"].as_str().unwrap_or("Лампа").to_string(),
        room: dev["room"].as_str().and_then(|r| rooms.get(r)).cloned(),
        supports_ct: ct.is_some(),
        supports_rgb: color_model.is_some(),
        ct_min,
        ct_max,
        color_model,
        state,
    })
}

/// Every light in the Yandex home; empty when Yandex isn't connected.
pub async fn devices() -> Result<Vec<Device>, String> {
    if !connected() {
        return Ok(Vec::new());
    }
    let info = api(reqwest::Method::GET, "/user/info", None).await?;
    let rooms: HashMap<String, String> = info["rooms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some((
                r["id"].as_str()?.to_string(),
                r["name"].as_str()?.to_string(),
            ))
        })
        .collect();
    let devices: Vec<Device> = info["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| parse(d, &rooms))
        .collect();
    *DEVICES.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(devices.iter().map(|d| (d.id.clone(), d.clone())).collect());
    Ok(devices)
}

async fn device(id: &str) -> Result<Device, String> {
    if let Some(d) = DEVICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(id))
    {
        return Ok(d.clone());
    }
    devices()
        .await?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or("Лампа не найдена в доме Яндекса".into())
}

fn action(kind: &str, instance: &str, value: Value) -> Value {
    json!({ "type": kind, "state": { "instance": instance, "value": value } })
}

/// Sends the change and returns the lamp as it should be now: Yandex answers
/// before the lamp's own cloud reports back, so a fresh read would still show
/// the old state for a while.
pub async fn apply(id: &str, change: &super::yeelight::Change) -> Result<Device, String> {
    let mut dev = device(id).await?;
    let adjusts = change.bright.is_some() || change.ct.is_some() || change.rgb.is_some();
    let power = match change.power {
        Some(p) => Some(p),
        None if adjusts => Some(true),
        None => None,
    };
    let mut actions = Vec::new();
    if let Some(on) = power {
        actions.push(action("devices.capabilities.on_off", "on", json!(on)));
    }
    if power != Some(false) {
        if let Some(b) = change.bright {
            actions.push(action(
                "devices.capabilities.range",
                "brightness",
                json!(b.clamp(1, 100)),
            ));
        }
        if let Some(ct) = change.ct.filter(|_| dev.supports_ct) {
            actions.push(action(
                "devices.capabilities.color_setting",
                "temperature_k",
                json!(ct.clamp(dev.ct_min, dev.ct_max)),
            ));
        }
        if let Some(rgb) = change.rgb.filter(|_| dev.supports_rgb) {
            actions.push(match dev.color_model.as_deref() {
                Some("hsv") => {
                    let (h, s, _) = rgb_to_hsv(rgb);
                    // Value at the lamp's brightness, so picking a colour doesn't change it.
                    let v = change
                        .bright
                        .or(dev.state.as_ref().map(|s| s.main.bright))
                        .unwrap_or(100);
                    action(
                        "devices.capabilities.color_setting",
                        "hsv",
                        json!({ "h": h, "s": s, "v": v }),
                    )
                }
                _ => action(
                    "devices.capabilities.color_setting",
                    "rgb",
                    json!(rgb & 0xFF_FF_FF),
                ),
            });
        }
    }
    if actions.is_empty() {
        return Ok(dev);
    }

    let res = api(
        reqwest::Method::POST,
        "/devices/actions",
        Some(json!({ "devices": [{ "id": id, "actions": actions }] })),
    )
    .await?;
    let failed = res["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|d| d["capabilities"].as_array().into_iter().flatten())
        .map(|c| &c["state"]["action_result"])
        .find(|r| r["status"].as_str() == Some("ERROR"));
    if let Some(r) = failed {
        return Err(match r["error_code"].as_str() {
            Some("DEVICE_UNREACHABLE") => "Лампа не в сети".into(),
            Some("DEVICE_BUSY") => "Лампа занята, попробуйте ещё раз".into(),
            _ => format!(
                "Лампа не выполнила команду: {}",
                r["error_message"]
                    .as_str()
                    .or(r["error_code"].as_str())
                    .unwrap_or("ошибка")
            ),
        });
    }

    if let Some(state) = dev.state.as_mut() {
        let l = &mut state.main;
        if let Some(on) = power {
            l.power = on;
        }
        if let Some(b) = change.bright {
            l.bright = b.clamp(1, 100);
        }
        if let Some(ct) = change.ct.filter(|_| dev.supports_ct) {
            (l.ct, l.color_mode) = (ct.clamp(dev.ct_min, dev.ct_max), 2);
        }
        if let Some(rgb) = change.rgb.filter(|_| dev.supports_rgb) {
            (l.rgb, l.color_mode) = (rgb & 0xFF_FF_FF, 1);
        }
    }
    if let Some(map) = DEVICES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.insert(dev.id.clone(), dev.clone());
    }
    Ok(dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_round_trip() {
        assert_eq!(hsv_to_rgb(0.0, 100.0, 100.0), 0xFF0000);
        assert_eq!(hsv_to_rgb(120.0, 100.0, 100.0), 0x00FF00);
        assert_eq!(rgb_to_hsv(0x0000FF), (240, 100, 100));
        assert_eq!(rgb_to_hsv(0xFFFFFF), (0, 0, 100));
        let (h, s, v) = rgb_to_hsv(0xFF8800);
        assert_eq!(hsv_to_rgb(h as f64, s as f64, v as f64), 0xFF8800);
    }

    #[test]
    fn parses_a_light() {
        let dev = json!({
            "id": "abc", "name": "Торшер", "type": "devices.types.light", "room": "r1",
            "capabilities": [
                { "type": "devices.capabilities.on_off", "state": { "instance": "on", "value": true } },
                { "type": "devices.capabilities.range", "parameters": { "instance": "brightness" },
                  "state": { "instance": "brightness", "value": 40 } },
                { "type": "devices.capabilities.color_setting",
                  "parameters": { "color_model": "hsv", "temperature_k": { "min": 2700, "max": 6500 } },
                  "state": { "instance": "hsv", "value": { "h": 240, "s": 100, "v": 40 } } }
            ]
        });
        let rooms = HashMap::from([("r1".to_string(), "Спальня".to_string())]);
        let d = parse(&dev, &rooms).unwrap();
        assert_eq!(
            (d.name.as_str(), d.room.as_deref()),
            ("Торшер", Some("Спальня"))
        );
        assert!(d.supports_ct && d.supports_rgb);
        let s = d.state.unwrap().main;
        assert!(s.power);
        assert_eq!((s.bright, s.rgb, s.color_mode), (40, 0x0000FF, 1));
        assert!(parse(
            &json!({ "id": "x", "type": "devices.types.socket" }),
            &rooms
        )
        .is_none());
    }
}
