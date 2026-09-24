//! Spotify playlists and Liked Songs via the Web API.
//!
//! Sign-in is OAuth Authorization Code + PKCE with the user's own Client ID
//! (Spotify no longer grants quota to individual apps) and a loopback
//! redirect on a fixed port: register `http://127.0.0.1:43821/callback` in the
//! Spotify dashboard (its validator rejects port-less loopback URIs). The
//! refresh token lives in Credential Manager.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::prelude::{Engine, BASE64_URL_SAFE_NO_PAD};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{net, secrets};

const AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API: &str = "https://api.spotify.com/v1";
const SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read \
                      user-library-modify user-read-playback-state user-modify-playback-state";
/// Added after the first release; older sign-ins lack it until the user signs in again.
const LIKE_SCOPE: &str = "user-library-modify";
const SECRET: &str = "DockPanel/spotify";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Must match the Redirect URI registered in the Spotify dashboard.
const REDIRECT_PORT: u16 = 43821;
/// Opens Liked Songs in the desktop app.
const LIKED_URI: &str = "spotify:collection:tracks";
/// How long to wait for a just-launched Spotify app to show up as a device.
const DEVICE_WAIT: Duration = Duration::from_secs(15);
const DEVICE_POLL: Duration = Duration::from_millis(750);

#[derive(Serialize, Deserialize)]
struct Stored {
    client_id: String,
    refresh_token: String,
}

static ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);
/// Scopes the current token was granted, as the token endpoint reported them.
static GRANTED: Mutex<Option<String>> = Mutex::new(None);

fn random_urlsafe(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(BASE64_URL_SAFE_NO_PAD.encode(buf))
}

fn stored() -> Option<Stored> {
    serde_json::from_str(&secrets::read(SECRET)?).ok()
}

fn store(s: &Stored) -> Result<(), String> {
    secrets::write(SECRET, &serde_json::to_string(s).map_err(|e| e.to_string())?)
}

async fn token_request(form: &[(&str, &str)]) -> Result<Value, String> {
    let res = net::client()
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("Нет связи со Spotify: {e}"))?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if ok {
        Ok(v)
    } else {
        Err(match v["error"].as_str() {
            Some("invalid_grant") => "Вход в Spotify истёк. Войдите заново.".into(),
            Some("invalid_client") => "Неверный Client ID.".into(),
            _ => format!("Spotify: {}", v["error_description"].as_str().unwrap_or("ошибка входа")),
        })
    }
}

fn remember_access(v: &Value) {
    if let Some(scope) = v["scope"].as_str() {
        *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(scope.to_string());
    }
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
    let s = stored().ok_or("Spotify не подключён")?;
    let v = token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &s.refresh_token),
        ("client_id", &s.client_id),
    ])
    .await?;
    // PKCE refresh tokens rotate.
    if let Some(rt) = v["refresh_token"].as_str() {
        store(&Stored { client_id: s.client_id, refresh_token: rt.to_string() })?;
    }
    remember_access(&v);
    v["access_token"].as_str().map(str::to_string).ok_or("Spotify не выдал токен".into())
}

async fn api(method: Method, path: &str, body: Option<Value>) -> Result<(StatusCode, Value), String> {
    let token = access_token().await?;
    let mut req = net::client().request(method, format!("{API}{path}")).bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await.map_err(|e| format!("Нет связи со Spotify: {e}"))?;
    let status = res.status();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    Ok((status, v))
}

async fn api_get(path: &str) -> Result<Value, String> {
    let (status, v) = api(Method::GET, path, None).await?;
    if status.is_success() {
        return Ok(v);
    }
    let msg = v["error"]["message"].as_str().unwrap_or("");
    Err(if status == StatusCode::FORBIDDEN && msg.to_lowercase().contains("registered") {
        "Аккаунт не добавлен в приложение: Spotify Dashboard → ваше приложение → User Management.".into()
    } else if status == StatusCode::FORBIDDEN {
        "Spotify отказал в доступе. Для приложений в режиме разработки владельцу нужен Premium.".into()
    } else {
        format!("Spotify {}: {msg}", status.as_u16())
    })
}

// ---- login ------------------------------------------------------------------------

/// Waits for the browser redirect on the loopback listener; returns the query string.
fn wait_for_redirect(listener: TcpListener) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let target = request.split_whitespace().nth(1).unwrap_or("");
                // Browsers also ask for /favicon.ico; only the callback counts.
                let Some(query) = target.strip_prefix("/callback?") else {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let body = "<!doctype html><meta charset=utf-8><title>Dock Panel</title>\
                    <body style=\"font:16px system-ui;background:#18181c;color:#eee;display:grid;place-items:center;height:100vh;margin:0\">\
                    <div>Готово! Вкладку можно закрыть и вернуться в Dock Panel.</div>";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                return Ok(query.to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("Вход не завершён за 5 минут".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
    })
}

#[tauri::command]
pub async fn spotify_login(app: tauri::AppHandle, client_id: String) -> Result<String, String> {
    let client_id = client_id.trim().to_string();
    if client_id.len() < 16 {
        return Err("Вставьте Client ID из Spotify Developer Dashboard".into());
    }

    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .map_err(|_| format!("Порт {REDIRECT_PORT} занят другой программой. Закройте её или повторите позже."))?;
    let redirect = format!("http://127.0.0.1:{REDIRECT_PORT}/callback");
    let verifier = random_urlsafe(48)?;
    let challenge = BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(16)?;

    let auth = reqwest::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
            ("scope", SCOPES),
            ("state", state.as_str()),
        ],
    )
    .map_err(|e| e.to_string())?;
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(auth.as_str(), None::<&str>)
        .map_err(|e| e.to_string())?;

    let query = tauri::async_runtime::spawn_blocking(move || wait_for_redirect(listener))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(err) = query_param(&query, "error") {
        return Err(if err == "access_denied" { "Вход отменён".into() } else { format!("Spotify: {err}") });
    }
    if query_param(&query, "state").as_deref() != Some(state.as_str()) {
        return Err("Ответ Spotify не прошёл проверку, попробуйте ещё раз".into());
    }
    let code = query_param(&query, "code").ok_or("Spotify не вернул код входа")?;

    let v = token_request(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("client_id", &client_id),
        ("code_verifier", &verifier),
    ])
    .await?;
    let refresh_token = v["refresh_token"].as_str().ok_or("Spotify не выдал refresh token")?.to_string();
    store(&Stored { client_id, refresh_token })?;
    remember_access(&v);

    let me = api_get("/me").await?;
    Ok(me["display_name"].as_str().or(me["id"].as_str()).unwrap_or("Spotify").to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyStatus {
    connected: bool,
    user: Option<String>,
    error: Option<String>,
    /// False for sign-ins made before liking was added: they need to sign in again.
    can_like: bool,
}

fn can_like() -> bool {
    // A token response without `scope` tells nothing; let the like itself find out.
    GRANTED.lock().unwrap_or_else(|e| e.into_inner()).as_deref().is_none_or(|s| s.split(' ').any(|x| x == LIKE_SCOPE))
}

#[tauri::command]
pub async fn spotify_status() -> SpotifyStatus {
    if stored().is_none() {
        return SpotifyStatus { connected: false, user: None, error: None, can_like: false };
    }
    match api_get("/me").await {
        Ok(me) => SpotifyStatus {
            connected: true,
            user: me["display_name"].as_str().or(me["id"].as_str()).map(str::to_string),
            error: None,
            can_like: can_like(),
        },
        Err(e) => SpotifyStatus { connected: true, user: None, error: Some(e), can_like: can_like() },
    }
}

#[tauri::command]
pub fn spotify_logout() {
    secrets::delete(SECRET);
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// ---- library ----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    id: String,
    name: String,
    uri: String,
    image: Option<String>,
    tracks: Option<u64>,
    owner: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    liked_total: Option<u64>,
    liked_uri: String,
    playlists: Vec<Playlist>,
}

#[tauri::command]
pub async fn spotify_library() -> Result<Library, String> {
    let lists = api_get("/me/playlists?limit=50").await?;
    let liked = api_get("/me/tracks?limit=1").await;
    let playlists = lists["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| !p.is_null())
        .filter_map(|p| {
            Some(Playlist {
                id: p["id"].as_str()?.to_string(),
                name: p["name"].as_str().unwrap_or_default().to_string(),
                uri: p["uri"].as_str()?.to_string(),
                // Images are ordered largest first; the smallest keeps tiles light.
                image: p["images"].as_array().and_then(|imgs| imgs.last()).and_then(|i| i["url"].as_str()).map(str::to_string),
                tracks: p["items"]["total"].as_u64().or(p["tracks"]["total"].as_u64()),
                owner: p["owner"]["display_name"].as_str().map(str::to_string),
            })
        })
        .collect();
    Ok(Library {
        liked_total: liked.ok().and_then(|v| v["total"].as_u64()),
        liked_uri: LIKED_URI.into(),
        playlists,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayOutcome {
    /// Started through the API on a running Spotify.
    Played,
    /// Spotify was closed: launched it, then started through the API.
    Launched,
    /// Opened in the Spotify app without playing (no Premium, or it never showed up).
    Opened,
}

enum Start {
    Played,
    /// 404: no active device (or the given one is gone).
    NoDevice,
    Failed,
}

async fn start(body: &Value, device: Option<&str>) -> Start {
    let path = match device {
        Some(id) => format!("/me/player/play?device_id={id}"),
        None => "/me/player/play".into(),
    };
    match api(Method::PUT, &path, Some(body.clone())).await {
        Ok((status, _)) if status.is_success() => Start::Played,
        Ok((StatusCode::NOT_FOUND, _)) => Start::NoDevice,
        _ => Start::Failed,
    }
}

/// This PC's Spotify to wake when nothing is active. The desktop app names its
/// device after the computer; any other computer is the next best. A phone that
/// happens to be online is never picked: the click came from this PC.
async fn pick_device() -> Option<String> {
    let v = api_get("/me/player/devices").await.ok()?;
    let this_pc = std::env::var("COMPUTERNAME").unwrap_or_default().to_lowercase();
    v["devices"]
        .as_array()?
        .iter()
        .filter(|d| d["is_restricted"] != true && d["type"] == "Computer")
        .min_by_key(|d| d["name"].as_str().unwrap_or_default().to_lowercase() != this_pc)?["id"]
        .as_str()
        .map(str::to_string)
}

/// Liked Songs can't be a `context_uri` as `spotify:collection:tracks`, only per user.
async fn playable_context(uri: &str) -> Option<String> {
    if uri != LIKED_URI {
        return Some(uri.to_string());
    }
    let me = api_get("/me").await.ok()?;
    Some(format!("spotify:user:{}:collection", me["id"].as_str()?))
}

fn open_in_app(app: &tauri::AppHandle, uri: &str) -> Result<(), String> {
    tauri_plugin_opener::OpenerExt::opener(app)
        .open_url(uri, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Starts a playlist, album, artist or Liked Songs (from `track` on, if given)
/// with Premium: on the active device; else on this PC's idle Spotify; else
/// launches Spotify and starts once the app registers as a device. Without
/// Premium it only opens `uri` in the app.
#[tauri::command]
pub async fn spotify_play(app: tauri::AppHandle, uri: String, track: Option<String>) -> Result<PlayOutcome, String> {
    let Some(context) = playable_context(&uri).await else {
        open_in_app(&app, &uri)?;
        return Ok(PlayOutcome::Opened);
    };
    let context = match track {
        Some(track) => json!({ "context_uri": context, "offset": { "uri": track } }),
        None => json!({ "context_uri": context }),
    };

    match start(&context, None).await {
        Start::Played => return Ok(PlayOutcome::Played),
        Start::Failed => {
            open_in_app(&app, &uri)?;
            return Ok(PlayOutcome::Opened);
        }
        Start::NoDevice => {}
    }

    // Spotify is running but idle: wake it with an explicit device.
    if let Some(device) = pick_device().await {
        if let Start::Played = start(&context, Some(&device)).await {
            return Ok(PlayOutcome::Played);
        }
        open_in_app(&app, &uri)?;
        return Ok(PlayOutcome::Opened);
    }

    // Spotify is closed: opening the URI launches it; start once it registers.
    open_in_app(&app, &uri)?;
    let deadline = Instant::now() + DEVICE_WAIT;
    while Instant::now() < deadline {
        net::sleep(DEVICE_POLL).await;
        if let Some(device) = pick_device().await {
            if let Start::Played = start(&context, Some(&device)).await {
                return Ok(PlayOutcome::Launched);
            }
        }
    }
    Ok(PlayOutcome::Opened)
}

// ---- now playing: like ------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentTrack {
    id: String,
    uri: String,
    name: String,
    liked: bool,
}

/// PUT/DELETE/GET on the unified `/me/library` (uris), falling back to the
/// older `/me/tracks` (ids) where the account's API doesn't have it yet.
async fn library(method: Method, action: &str, track_uri: &str, track_id: &str) -> Result<(StatusCode, Value), String> {
    let (status, v) = api(method.clone(), &format!("/me/library{action}?uris={track_uri}"), None).await?;
    if status != StatusCode::NOT_FOUND && status != StatusCode::GONE {
        return Ok((status, v));
    }
    api(method, &format!("/me/tracks{action}?ids={track_id}"), None).await
}

/// The track Spotify is playing on any device, and whether it is in Liked Songs.
/// `None` for nothing, an ad or a podcast episode.
#[tauri::command]
pub async fn spotify_current() -> Result<Option<CurrentTrack>, String> {
    let (status, v) = api(Method::GET, "/me/player/currently-playing", None).await?;
    if status == StatusCode::NO_CONTENT || !status.is_success() || v["currently_playing_type"] != "track" {
        return Ok(None);
    }
    let item = &v["item"];
    let (Some(id), Some(uri)) = (item["id"].as_str(), item["uri"].as_str()) else { return Ok(None) };
    let (status, contains) = library(Method::GET, "/contains", uri, id).await?;
    Ok(Some(CurrentTrack {
        id: id.to_string(),
        uri: uri.to_string(),
        name: item["name"].as_str().unwrap_or_default().to_string(),
        liked: status.is_success() && contains[0] == true,
    }))
}

/// Adds the track to Liked Songs (`liked`) or removes it.
#[tauri::command]
pub async fn spotify_set_liked(id: String, uri: String, liked: bool) -> Result<(), String> {
    let method = if liked { Method::PUT } else { Method::DELETE };
    let (status, v) = library(method, "", &uri, &id).await?;
    if status.is_success() {
        return Ok(());
    }
    Err(if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
        "Нет разрешения на лайки: выйдите из Spotify в настройках и войдите снова.".into()
    } else {
        format!("Spotify {}: {}", status.as_u16(), v["error"]["message"].as_str().unwrap_or(""))
    })
}

// ---- devices -----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    id: String,
    name: String,
    /// "Computer", "Smartphone", "Speaker", "TV", ...
    kind: String,
    active: bool,
}

#[tauri::command]
pub async fn spotify_devices() -> Result<Vec<Device>, String> {
    let v = api_get("/me/player/devices").await?;
    Ok(v["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["is_restricted"] != true)
        .filter_map(|d| {
            Some(Device {
                id: d["id"].as_str()?.to_string(),
                name: d["name"].as_str().unwrap_or_default().to_string(),
                kind: d["type"].as_str().unwrap_or_default().to_string(),
                active: d["is_active"] == true,
            })
        })
        .collect())
}

/// Moves playback to `id` and keeps it playing there.
#[tauri::command]
pub async fn spotify_transfer(id: String) -> Result<(), String> {
    let (status, v) = api(Method::PUT, "/me/player", Some(json!({ "device_ids": [id], "play": true }))).await?;
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("Spotify {}: {}", status.as_u16(), v["error"]["message"].as_str().unwrap_or("не удалось переключить")))
    }
}

// ---- search & queue ----------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    /// "track", "artist", "album" or "playlist".
    kind: &'static str,
    uri: String,
    name: String,
    subtitle: String,
    image: Option<String>,
    /// For tracks: the album, so playback continues past the track.
    context: Option<String>,
}

fn smallest_image(images: &Value) -> Option<String> {
    images.as_array()?.last()?["url"].as_str().map(str::to_string)
}

fn artist_names(v: &Value) -> String {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Tracks first, then a few artists, albums and playlists.
#[tauri::command]
pub async fn spotify_search(query: String) -> Result<Vec<SearchItem>, String> {
    let q = percent_encoding::utf8_percent_encode(query.trim(), percent_encoding::NON_ALPHANUMERIC);
    let v = api_get(&format!("/search?q={q}&type=track,artist,album,playlist&limit=5")).await?;
    let items = |kind: &str| v[format!("{kind}s")]["items"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();

    for t in items("track").iter().filter(|t| !t.is_null()) {
        out.push(SearchItem {
            kind: "track",
            uri: t["uri"].as_str().unwrap_or_default().into(),
            name: t["name"].as_str().unwrap_or_default().into(),
            subtitle: artist_names(&t["artists"]),
            image: smallest_image(&t["album"]["images"]),
            context: t["album"]["uri"].as_str().map(str::to_string),
        });
    }
    for a in items("artist").iter().filter(|a| !a.is_null()).take(2) {
        out.push(SearchItem {
            kind: "artist",
            uri: a["uri"].as_str().unwrap_or_default().into(),
            name: a["name"].as_str().unwrap_or_default().into(),
            subtitle: "Исполнитель".into(),
            image: smallest_image(&a["images"]),
            context: None,
        });
    }
    for a in items("album").iter().filter(|a| !a.is_null()).take(3) {
        out.push(SearchItem {
            kind: "album",
            uri: a["uri"].as_str().unwrap_or_default().into(),
            name: a["name"].as_str().unwrap_or_default().into(),
            subtitle: format!("Альбом · {}", artist_names(&a["artists"])),
            image: smallest_image(&a["images"]),
            context: None,
        });
    }
    for p in items("playlist").iter().filter(|p| !p.is_null()).take(3) {
        out.push(SearchItem {
            kind: "playlist",
            uri: p["uri"].as_str().unwrap_or_default().into(),
            name: p["name"].as_str().unwrap_or_default().into(),
            subtitle: format!("Плейлист · {}", p["owner"]["display_name"].as_str().unwrap_or("Spotify")),
            image: smallest_image(&p["images"]),
            context: None,
        });
    }
    out.retain(|i| !i.uri.is_empty());
    Ok(out)
}

/// Adds a track to the end of the queue on the active device.
#[tauri::command]
pub async fn spotify_queue(uri: String) -> Result<(), String> {
    let uri = percent_encoding::utf8_percent_encode(&uri, percent_encoding::NON_ALPHANUMERIC);
    let (status, v) = api(Method::POST, &format!("/me/player/queue?uri={uri}"), None).await?;
    match status {
        s if s.is_success() => Ok(()),
        StatusCode::NOT_FOUND => Err("Сначала включите что-нибудь в Spotify".into()),
        s => Err(format!("Spotify {}: {}", s.as_u16(), v["error"]["message"].as_str().unwrap_or(""))),
    }
}
