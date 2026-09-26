//! Spotify playlists and Liked Songs via the Web API.
//!
//! Sign-in is OAuth Authorization Code + PKCE with the user's own Client ID
//! (Spotify no longer grants quota to individual apps) and a loopback
//! redirect on a fixed port: register `http://127.0.0.1:43821/callback` in the
//! Spotify dashboard (its validator rejects port-less loopback URIs). The
//! refresh token lives in Credential Manager.

use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{net, oauth, secrets};

const AUTH_URL: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API: &str = "https://api.spotify.com/v1";
const SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read \
                      user-library-modify user-read-playback-state user-modify-playback-state \
                      playlist-modify-public playlist-modify-private";
/// Added after the first release; older sign-ins lack them until the user signs in again.
const LIKE_SCOPE: &str = "user-library-modify";
const PLAYLIST_SCOPE: &str = "playlist-modify-private";
const SECRET: &str = "DockPanel/spotify";
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
/// The signed-in user's id, once known.
static ME: Mutex<Option<String>> = Mutex::new(None);

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
    let needs_length = method != Method::GET && method != Method::DELETE;
    let mut req = net::client().request(method, format!("{API}{path}")).bearer_auth(token);
    if let Some(b) = body {
        req = req.json(&b);
    } else if needs_length {
        // Spotify answers 411 to a PUT/POST without Content-Length.
        req = req.header(reqwest::header::CONTENT_LENGTH, "0");
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

#[tauri::command]
pub async fn spotify_login(app: tauri::AppHandle, client_id: String) -> Result<String, String> {
    let client_id = client_id.trim().to_string();
    if client_id.len() < 16 {
        return Err("Вставьте Client ID из Spotify Developer Dashboard".into());
    }

    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .map_err(|_| format!("Порт {REDIRECT_PORT} занят другой программой. Закройте её или повторите позже."))?;
    let redirect = format!("http://127.0.0.1:{REDIRECT_PORT}/callback");
    let (verifier, challenge) = oauth::pkce()?;
    let state = oauth::random_urlsafe(16)?;

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

    let query = tauri::async_runtime::spawn_blocking(move || oauth::wait_for_redirect(listener, "/callback"))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(err) = oauth::query_param(&query, "error") {
        return Err(if err == "access_denied" { "Вход отменён".into() } else { format!("Spotify: {err}") });
    }
    if oauth::query_param(&query, "state").as_deref() != Some(state.as_str()) {
        return Err("Ответ Spotify не прошёл проверку, попробуйте ещё раз".into());
    }
    let code = oauth::query_param(&query, "code").ok_or("Spotify не вернул код входа")?;

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

    *ME.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
    /// Likewise for adding tracks to playlists.
    can_edit_playlists: bool,
}

fn granted(scope: &str) -> bool {
    // A token response without `scope` tells nothing; let the call itself find out.
    GRANTED.lock().unwrap_or_else(|e| e.into_inner()).as_deref().is_none_or(|s| s.split(' ').any(|x| x == scope))
}

#[tauri::command]
pub async fn spotify_status() -> SpotifyStatus {
    if stored().is_none() {
        return SpotifyStatus { connected: false, user: None, error: None, can_like: false, can_edit_playlists: false };
    }
    match api_get("/me").await {
        Ok(me) => SpotifyStatus {
            connected: true,
            user: me["display_name"].as_str().or(me["id"].as_str()).map(str::to_string),
            error: None,
            can_like: granted(LIKE_SCOPE),
            can_edit_playlists: granted(PLAYLIST_SCOPE),
        },
        Err(e) => SpotifyStatus {
            connected: true,
            user: None,
            error: Some(e),
            can_like: granted(LIKE_SCOPE),
            can_edit_playlists: granted(PLAYLIST_SCOPE),
        },
    }
}

#[tauri::command]
pub fn spotify_logout() {
    secrets::delete(SECRET);
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *GRANTED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *ME.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
    /// The user's own or a collaborative playlist: tracks can be added to it.
    editable: bool,
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
    let me = my_id().await;
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
                editable: p["collaborative"] == true || (me.is_some() && p["owner"]["id"].as_str() == me.as_deref()),
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
    Some(format!("spotify:user:{}:collection", my_id().await?))
}

async fn my_id() -> Option<String> {
    if let Some(id) = ME.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(id);
    }
    let id = api_get("/me").await.ok()?["id"].as_str()?.to_string();
    *ME.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
    Some(id)
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

// ---- player: seek, volume, shuffle, repeat -----------------------------------------

/// Turns a player call's answer into a user-facing error.
fn player_result(status: StatusCode, v: &Value) -> Result<(), String> {
    let msg = v["error"]["message"].as_str().unwrap_or("");
    match status {
        s if s.is_success() => Ok(()),
        StatusCode::NOT_FOUND => Err("Сейчас в Spotify ничего не играет".into()),
        StatusCode::FORBIDDEN if v["error"]["reason"] == "VOLUME_CONTROL_DISALLOW" => {
            Err("Это устройство не даёт менять громкость".into())
        }
        StatusCode::FORBIDDEN if msg.to_lowercase().contains("premium") => Err("Нужен Spotify Premium".into()),
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
            Err("Нет разрешения: выйдите из Spotify в настройках и войдите снова.".into())
        }
        s => Err(format!("Spotify {}: {msg}", s.as_u16())),
    }
}

async fn player_call(method: Method, path: &str, body: Option<Value>) -> Result<(), String> {
    let (status, v) = api(method, path, body).await?;
    player_result(status, &v)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    playing: bool,
    /// `None` when the device doesn't let its volume be changed.
    volume: Option<u8>,
    shuffle: bool,
    /// "off", "context" (the playlist or album) or "track".
    repeat: String,
    progress_ms: Option<u64>,
    duration_ms: Option<u64>,
}

/// Spotify's playback state on whatever device is active; `None` when idle.
#[tauri::command]
pub async fn spotify_player() -> Result<Option<PlayerState>, String> {
    let (status, v) = api(Method::GET, "/me/player", None).await?;
    if status == StatusCode::NO_CONTENT || !status.is_success() || v.is_null() {
        return Ok(None);
    }
    let device = &v["device"];
    Ok(Some(PlayerState {
        playing: v["is_playing"] == true,
        volume: (device["supports_volume"] != false)
            .then(|| device["volume_percent"].as_u64())
            .flatten()
            .map(|p| p.min(100) as u8),
        shuffle: v["shuffle_state"] == true,
        repeat: v["repeat_state"].as_str().unwrap_or("off").to_string(),
        progress_ms: v["progress_ms"].as_u64(),
        duration_ms: v["item"]["duration_ms"].as_u64(),
    }))
}

#[tauri::command]
pub async fn spotify_seek(position_ms: u64) -> Result<(), String> {
    player_call(Method::PUT, &format!("/me/player/seek?position_ms={position_ms}"), None).await
}

#[tauri::command]
pub async fn spotify_volume(percent: u8) -> Result<(), String> {
    player_call(Method::PUT, &format!("/me/player/volume?volume_percent={}", percent.min(100)), None).await
}

#[tauri::command]
pub async fn spotify_shuffle(on: bool) -> Result<(), String> {
    player_call(Method::PUT, &format!("/me/player/shuffle?state={on}"), None).await
}

#[tauri::command]
pub async fn spotify_repeat(mode: String) -> Result<(), String> {
    if !matches!(mode.as_str(), "off" | "context" | "track") {
        return Err("Неизвестный режим повтора".into());
    }
    player_call(Method::PUT, &format!("/me/player/repeat?state={mode}"), None).await
}

// ---- up next -----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    uri: String,
    name: String,
    subtitle: String,
    image: Option<String>,
    duration_ms: Option<u64>,
}

/// How many upcoming tracks the panel lists (and can skip to).
const UP_NEXT_MAX: usize = 15;

/// What plays after the current track: the user's queue, then the context.
#[tauri::command]
pub async fn spotify_up_next() -> Result<Vec<QueueItem>, String> {
    let (status, v) = api(Method::GET, "/me/player/queue", None).await?;
    if status == StatusCode::NO_CONTENT || v.is_null() {
        return Ok(Vec::new());
    }
    player_result(status, &v)?;
    Ok(v["queue"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| !i.is_null())
        .take(UP_NEXT_MAX)
        .filter_map(|i| {
            let episode = i["type"] == "episode";
            Some(QueueItem {
                uri: i["uri"].as_str()?.to_string(),
                name: i["name"].as_str().unwrap_or_default().to_string(),
                subtitle: if episode {
                    i["show"]["name"].as_str().unwrap_or("Подкаст").to_string()
                } else {
                    artist_names(&i["artists"])
                },
                image: smallest_image(if episode { &i["images"] } else { &i["album"]["images"] }),
                duration_ms: i["duration_ms"].as_u64(),
            })
        })
        .collect())
}

/// Jumps `count` tracks ahead (to the `count`-th item of "up next").
/// Spotify has no "play this queue entry", so it skips one by one.
#[tauri::command]
pub async fn spotify_skip(count: u32) -> Result<(), String> {
    for _ in 0..count.clamp(1, UP_NEXT_MAX as u32) {
        player_call(Method::POST, "/me/player/next", None).await?;
    }
    Ok(())
}

// ---- add to playlist ---------------------------------------------------------------

/// Adds a track to one of the user's playlists: `/playlists/{id}/items`, or
/// the older `/tracks` where the account's API doesn't have it yet.
#[tauri::command]
pub async fn spotify_add_to_playlist(playlist_id: String, uri: String) -> Result<(), String> {
    let id = percent_encoding::utf8_percent_encode(&playlist_id, percent_encoding::NON_ALPHANUMERIC).to_string();
    let body = json!({ "uris": [uri] });
    let (mut status, mut v) = api(Method::POST, &format!("/playlists/{id}/items"), Some(body.clone())).await?;
    if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
        (status, v) = api(Method::POST, &format!("/playlists/{id}/tracks"), Some(body)).await?;
    }
    match status {
        s if s.is_success() => Ok(()),
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
            Err("Нет разрешения менять плейлисты: выйдите из Spotify в настройках и войдите снова.".into())
        }
        s => Err(format!("Spotify {}: {}", s.as_u16(), v["error"]["message"].as_str().unwrap_or(""))),
    }
}
