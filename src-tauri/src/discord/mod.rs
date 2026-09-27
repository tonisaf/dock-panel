//! Discord through the local RPC of the running client: who is in the
//! voice channels the user watches (a toast when someone joins), the
//! channel the user is in, and the microphone and sound switches.
//!
//! RPC scopes are open only to an application's owner and its testers, so
//! the user signs in with their own app from discord.com/developers: the
//! client asks to authorize it in its own window (AUTHORIZE), the code is
//! swapped for tokens over HTTPS, and each session AUTHENTICATEs with the
//! access token. The client, secret and tokens live in Credential Manager.
//!
//! Voice state events don't say which channel they are about, so they only
//! trigger a re-read of the watched channels (GET_CHANNEL, local and cheap);
//! a periodic re-read also covers channels Discord sends no events for.

mod ipc;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
use tokio::io::{AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::sync::{oneshot, Notify};

use crate::{net, secrets};

const SECRET: &str = "DockPanel/discord";
const FILE: &str = "discord.json";
const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const SCOPES: [&str; 4] = ["rpc", "rpc.voice.read", "rpc.voice.write", "identify"];
/// Some apps need the redirect the token request names to be one of theirs.
const REDIRECT: &str = "http://localhost";
/// How often to look for a Discord that isn't running yet.
const RETRY: Duration = Duration::from_secs(5);
const RESYNC: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// The user reads and clicks Discord's authorize window.
const AUTHORIZE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const EVENT: &str = "discord:changed";
const VOICE_EVENTS: [&str; 5] = ["VOICE_STATE_CREATE", "VOICE_STATE_UPDATE", "VOICE_STATE_DELETE", "SPEAKING_START", "SPEAKING_STOP"];

// ---- saved settings ----------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Stored {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    access_token: String,
    /// Unix seconds.
    expires_at: u64,
}

fn stored() -> Option<Stored> {
    serde_json::from_str(&secrets::read(SECRET)?).ok()
}

fn store(s: &Stored) -> Result<(), String> {
    secrets::write(SECRET, &serde_json::to_string(s).map_err(|e| e.to_string())?)
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchedChannel {
    id: String,
    guild_id: String,
    name: String,
    guild_name: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    watched: Vec<WatchedChannel>,
    /// A toast when someone joins a watched channel.
    notify: bool,
    /// Global shortcut that toggles the microphone.
    mute_shortcut: Option<String>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved { watched: Vec::new(), notify: true, mute_shortcut: None }
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static SAVED: Mutex<Option<Saved>> = Mutex::new(None);

fn saved() -> Saved {
    SAVED.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(FILE))
}

fn update_saved(app: &AppHandle, f: impl FnOnce(&mut Saved)) -> Result<(), String> {
    let mut s = saved();
    f(&mut s);
    let p = settings_path(app)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    Ok(())
}

// ---- live state --------------------------------------------------------------------

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    id: String,
    name: String,
    avatar: String,
    speaking: bool,
    muted: bool,
    deafened: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    id: String,
    guild_id: String,
    name: String,
    guild_name: String,
    members: Vec<Member>,
}

#[derive(Serialize, Clone)]
pub struct Device {
    id: String,
    name: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    mute: bool,
    deaf: bool,
    /// 0–100.
    input_volume: f64,
    /// "VOICE_ACTIVITY" or "PUSH_TO_TALK".
    mode: String,
    input_device: String,
    input_devices: Vec<Device>,
}

#[derive(Default)]
struct Live {
    /// The session is up and authenticated.
    ready: bool,
    error: Option<String>,
    user: Option<Member>,
    voice: Option<Voice>,
    current: Option<String>,
    /// Channels read so far: the current one and the watched ones.
    channels: HashMap<String, Channel>,
    guild_names: HashMap<String, String>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn with_live<R>(f: impl FnOnce(&mut Live) -> R) -> R {
    let mut guard = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Live::default))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct State {
    configured: bool,
    /// Discord is running and the panel is signed in to it.
    connected: bool,
    error: Option<String>,
    user: Option<Member>,
    voice: Option<Voice>,
    current: Option<Channel>,
    watched: Vec<Channel>,
    notify: bool,
    mute_shortcut: Option<String>,
}

fn snapshot() -> State {
    let s = saved();
    with_live(|l| {
        let watched = s
            .watched
            .iter()
            .map(|w| {
                l.channels.get(&w.id).cloned().unwrap_or_else(|| Channel {
                    id: w.id.clone(),
                    guild_id: w.guild_id.clone(),
                    name: w.name.clone(),
                    guild_name: w.guild_name.clone(),
                    members: Vec::new(),
                })
            })
            .collect();
        State {
            configured: stored().is_some(),
            connected: l.ready,
            error: l.error.clone(),
            user: l.user.clone(),
            voice: l.voice.clone(),
            current: l.current.as_ref().and_then(|id| l.channels.get(id)).cloned(),
            watched,
            notify: s.notify,
            mute_shortcut: s.mute_shortcut,
        }
    })
}

fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, snapshot());
    }
}

// ---- parsing -----------------------------------------------------------------------

fn avatar(user: &Value) -> String {
    let id = user["id"].as_str().unwrap_or("0");
    match user["avatar"].as_str() {
        Some(hash) => format!("https://cdn.discordapp.com/avatars/{id}/{hash}.png?size=64"),
        // Discord's default avatars, picked the way the client picks them.
        None => format!("https://cdn.discordapp.com/embed/avatars/{}.png", (id.parse::<u64>().unwrap_or(0) >> 22) % 6),
    }
}

fn user_name(user: &Value) -> String {
    [&user["global_name"], &user["username"]]
        .into_iter()
        .find_map(|v| v.as_str().filter(|s| !s.is_empty()))
        .unwrap_or("?")
        .to_string()
}

fn parse_user(user: &Value) -> Option<Member> {
    Some(Member {
        id: user["id"].as_str()?.to_string(),
        name: user_name(user),
        avatar: avatar(user),
        speaking: false,
        muted: false,
        deafened: false,
    })
}

/// An entry of GET_CHANNEL's `voice_states`.
fn parse_member(v: &Value) -> Option<Member> {
    let mut m = parse_user(&v["user"])?;
    if let Some(nick) = v["nick"].as_str().filter(|n| !n.is_empty()) {
        m.name = nick.to_string();
    }
    let s = &v["voice_state"];
    let flag = |k: &str| s[k].as_bool().unwrap_or(false);
    m.muted = flag("mute") || flag("self_mute") || flag("suppress");
    m.deafened = flag("deaf") || flag("self_deaf");
    Some(m)
}

fn parse_voice(v: &Value) -> Voice {
    let input = &v["input"];
    Voice {
        mute: v["mute"].as_bool().unwrap_or(false),
        deaf: v["deaf"].as_bool().unwrap_or(false),
        input_volume: input["volume"].as_f64().unwrap_or(100.0),
        mode: v["mode"]["type"].as_str().unwrap_or("VOICE_ACTIVITY").to_string(),
        input_device: input["device_id"].as_str().unwrap_or("default").to_string(),
        input_devices: input["available_devices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| Some(Device { id: d["id"].as_str()?.to_string(), name: d["name"].as_str()?.to_string() }))
            .collect(),
    }
}

// ---- the connection ----------------------------------------------------------------

type Reply = Result<Value, String>;

struct Link {
    writer: tokio::sync::Mutex<WriteHalf<NamedPipeClient>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Reply>>>,
    stop: Notify,
    /// A voice event came in: re-read the channels soon.
    resync: Notify,
}

static LINK: Mutex<Option<Arc<Link>>> = Mutex::new(None);
/// Wakes the connection loop at once (after a sign-in, say).
static WAKE: Notify = Notify::const_new();
static NONCE: AtomicU64 = AtomicU64::new(1);

impl Link {
    async fn send(&self, op: u32, payload: &Value) -> Result<(), String> {
        let frame = ipc::encode(op, payload);
        let mut w = self.writer.lock().await;
        w.write_all(&frame).await.map_err(|e| format!("Связь с Discord прервалась: {e}"))
    }

    async fn call(&self, cmd: &str, args: Value, evt: Option<&str>, timeout: Duration) -> Reply {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed).to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(nonce.clone(), tx);
        let mut msg = json!({ "cmd": cmd, "args": args, "nonce": nonce });
        if let Some(evt) = evt {
            msg["evt"] = json!(evt);
        }
        self.send(ipc::FRAME, &msg).await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err("Discord закрылся".into()),
            Err(_) => {
                self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&nonce);
                Err("Discord не ответил".into())
            }
        }
    }

    async fn cmd(&self, cmd: &str, args: Value) -> Reply {
        self.call(cmd, args, None, CALL_TIMEOUT).await
    }

    fn fail_pending(&self) {
        for (_, tx) in self.pending.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            let _ = tx.send(Err("Discord закрылся".into()));
        }
    }
}

fn link() -> Result<Arc<Link>, String> {
    LINK.lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| if stored().is_some() { "Discord не запущен".into() } else { "Discord не подключён".to_string() })
}

fn rpc_error(data: &Value) -> String {
    let msg = data["message"].as_str().unwrap_or("ошибка");
    match data["code"].as_u64() {
        Some(4006) => "Этой команде не хватает прав: войдите в Discord заново".into(),
        _ => format!("Discord: {msg}"),
    }
}

/// Connects and does the handshake; returns the reader half and the READY payload.
async fn open(client_id: &str) -> Result<(Arc<Link>, ReadHalf<NamedPipeClient>), String> {
    let pipe = ipc::open().ok_or("Discord не запущен")?;
    let (mut rd, wr) = tokio::io::split(pipe);
    let link = Arc::new(Link {
        writer: tokio::sync::Mutex::new(wr),
        pending: Mutex::new(HashMap::new()),
        stop: Notify::new(),
        resync: Notify::new(),
    });
    link.send(ipc::HANDSHAKE, &json!({ "v": 1, "client_id": client_id })).await?;
    let (op, v) = tokio::time::timeout(CALL_TIMEOUT, ipc::read(&mut rd))
        .await
        .map_err(|_| "Discord не ответил")?
        .map_err(|e| format!("Связь с Discord прервалась: {e}"))?;
    if op == ipc::CLOSE || v["evt"].as_str() != Some("READY") {
        return Err(match v["code"].as_u64() {
            Some(4000) => "Discord не принял Application ID".into(),
            _ => format!("Discord: {}", v["message"].as_str().unwrap_or("отказал в подключении")),
        });
    }
    Ok((link, rd))
}

/// Reads until the pipe closes or `stop`, answering calls and handling events.
async fn read_loop(link: Arc<Link>, mut rd: ReadHalf<NamedPipeClient>) {
    loop {
        let frame = tokio::select! {
            f = ipc::read(&mut rd) => f,
            _ = link.stop.notified() => break,
        };
        let Ok((op, v)) = frame else { break };
        match op {
            ipc::PING => {
                let _ = link.send(ipc::PONG, &v).await;
            }
            ipc::CLOSE => break,
            ipc::FRAME => {
                if v["cmd"].as_str() == Some("DISPATCH") {
                    on_event(&link, v["evt"].as_str().unwrap_or(""), &v["data"]);
                } else if let Some(tx) = v["nonce"].as_str().and_then(|n| link.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(n)) {
                    let reply = if v["evt"].as_str() == Some("ERROR") { Err(rpc_error(&v["data"])) } else { Ok(v["data"].clone()) };
                    let _ = tx.send(reply);
                }
            }
            _ => {}
        }
    }
    link.fail_pending();
}

fn on_event(link: &Link, evt: &str, data: &Value) {
    match evt {
        "VOICE_SETTINGS_UPDATE" => {
            with_live(|l| l.voice = Some(parse_voice(data)));
            changed();
        }
        "SPEAKING_START" | "SPEAKING_STOP" => {
            let Some(user) = data["user_id"].as_str() else { return };
            let speaking = evt == "SPEAKING_START";
            let hit = with_live(|l| {
                let mut hit = false;
                for m in l.channels.values_mut().flat_map(|c| c.members.iter_mut()).filter(|m| m.id == user) {
                    hit |= m.speaking != speaking;
                    m.speaking = speaking;
                }
                hit
            });
            if hit {
                changed();
            }
        }
        "VOICE_CHANNEL_SELECT" => {
            let id = data["channel_id"].as_str().map(str::to_string);
            let previous = with_live(|l| std::mem::replace(&mut l.current, id.clone()));
            if previous != id {
                tauri::async_runtime::spawn(follow_current(previous, id));
            }
        }
        e if e.starts_with("VOICE_STATE_") => link.resync.notify_one(),
        _ => {}
    }
}

// ---- tokens ------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

async fn token_request(form: &[(&str, &str)]) -> Result<Value, String> {
    let res = net::client()
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("Нет связи с discord.com (в России нужен VPN): {e}"))?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if ok {
        return Ok(v);
    }
    Err(match v["error"].as_str() {
        Some("invalid_client") => "Неверный Application ID или Client Secret".into(),
        Some("invalid_grant") => "Вход в Discord истёк или отозван. Войдите снова.".into(),
        _ => format!("Discord: {}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("ошибка входа")),
    })
}

fn with_tokens(s: Stored, v: &Value) -> Result<Stored, String> {
    Ok(Stored {
        access_token: v["access_token"].as_str().ok_or("Discord не выдал токен")?.to_string(),
        refresh_token: v["refresh_token"].as_str().map_or(s.refresh_token, str::to_string),
        expires_at: now_secs() + v["expires_in"].as_u64().unwrap_or(3600),
        ..s
    })
}

/// Swaps an AUTHORIZE code for tokens.
async fn exchange(code: &str, client_id: &str, client_secret: &str, redirect: Option<&str>) -> Result<Value, String> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
    ];
    if let Some(r) = redirect {
        form.push(("redirect_uri", r));
    }
    token_request(&form).await
}

async fn refresh(s: Stored) -> Result<Stored, String> {
    let v = token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &s.refresh_token),
        ("client_id", &s.client_id),
        ("client_secret", &s.client_secret),
    ])
    .await?;
    let s = with_tokens(s, &v)?;
    store(&s)?;
    Ok(s)
}

async fn authenticate(link: &Link, mut s: Stored) -> Result<Member, String> {
    if s.expires_at < now_secs() + 60 {
        s = refresh(s).await?;
    }
    let data = match link.cmd("AUTHENTICATE", json!({ "access_token": s.access_token })).await {
        Ok(d) => d,
        // The token may have been revoked or cut short; one refresh, then give up.
        Err(_) => {
            let s = refresh(s).await?;
            link.cmd("AUTHENTICATE", json!({ "access_token": s.access_token })).await?
        }
    };
    parse_user(&data["user"]).ok_or("Discord не назвал пользователя".into())
}

// ---- the session -------------------------------------------------------------------

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let s = settings_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
        .unwrap_or_default();
    if let Some(sc) = s.mute_shortcut.as_deref().and_then(|t| t.parse::<Shortcut>().ok()) {
        if let Err(e) = app.global_shortcut().register(sc) {
            eprintln!("Discord mute shortcut is unavailable: {e}");
        }
    }
    *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    tauri::async_runtime::spawn(async {
        loop {
            if let Some(s) = stored() {
                if let Err(e) = session(s).await {
                    with_live(|l| l.error = Some(e));
                }
                with_live(|l| {
                    let error = l.error.take();
                    *l = Live { error, ..Live::default() };
                });
                changed();
            }
            let _ = tokio::time::timeout(RETRY, WAKE.notified()).await;
        }
    });
}

/// One connection to the running client, until it closes.
async fn session(s: Stored) -> Result<(), String> {
    let (link, rd) = match open(&s.client_id).await {
        Ok(x) => x,
        // Not running: nothing to report, just try again later.
        Err(_) if ipc::open().is_none() => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut reader = tauri::async_runtime::spawn(read_loop(link.clone(), rd));
    *LINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(link.clone());

    let setup = async {
        let user = authenticate(&link, s).await?;
        with_live(|l| {
            l.user = Some(user);
            l.error = None;
        });
        for evt in ["VOICE_SETTINGS_UPDATE", "VOICE_CHANNEL_SELECT"] {
            link.call("SUBSCRIBE", json!({}), Some(evt), CALL_TIMEOUT).await?;
        }
        let voice = link.cmd("GET_VOICE_SETTINGS", json!({})).await?;
        let current = link.cmd("GET_SELECTED_VOICE_CHANNEL", json!({})).await?;
        with_live(|l| {
            l.voice = Some(parse_voice(&voice));
            l.current = current["id"].as_str().map(str::to_string);
            l.ready = true;
        });
        for id in tracked() {
            subscribe(&link, &id, true).await;
        }
        resync(&link, false).await;
        Ok::<_, String>(())
    };
    let result = setup.await;
    if result.is_ok() {
        loop {
            tokio::select! {
                _ = &mut reader => break,
                _ = tokio::time::sleep(RESYNC) => resync(&link, true).await,
                _ = link.resync.notified() => {
                    // A join comes as several events; read the channels once.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    resync(&link, true).await;
                }
            }
        }
    }
    link.stop.notify_one();
    let _ = reader.await;
    let mut guard = LINK.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_some_and(|l| Arc::ptr_eq(l, &link)) {
        *guard = None;
    }
    result
}

/// Ends the current session (sign-out, sign-in with another app).
fn stop_session() {
    if let Some(link) = LINK.lock().unwrap_or_else(|e| e.into_inner()).take() {
        link.stop.notify_one();
    }
}

/// The current channel and the watched ones.
fn tracked() -> Vec<String> {
    let mut ids: Vec<String> = saved().watched.into_iter().map(|w| w.id).collect();
    if let Some(current) = with_live(|l| l.current.clone()) {
        if !ids.contains(&current) {
            ids.push(current);
        }
    }
    ids
}

async fn subscribe(link: &Link, channel: &str, on: bool) {
    let cmd = if on { "SUBSCRIBE" } else { "UNSUBSCRIBE" };
    for evt in VOICE_EVENTS {
        if let Err(e) = link.call(cmd, json!({ "channel_id": channel }), Some(evt), CALL_TIMEOUT).await {
            eprintln!("discord: {cmd} {evt} for {channel}: {e}");
        }
    }
}

async fn follow_current(previous: Option<String>, current: Option<String>) {
    let Ok(link) = link() else { return };
    let watched: HashSet<String> = saved().watched.into_iter().map(|w| w.id).collect();
    if let Some(p) = previous.filter(|p| !watched.contains(p)) {
        subscribe(&link, &p, false).await;
        with_live(|l| l.channels.remove(&p));
    }
    if let Some(c) = current.filter(|c| !watched.contains(c)) {
        subscribe(&link, &c, true).await;
    }
    resync(&link, false).await;
}

async fn guild_name(link: &Link, guild_id: &str) -> String {
    if let Some(name) = with_live(|l| l.guild_names.get(guild_id).cloned()) {
        return name;
    }
    let name = match link.cmd("GET_GUILD", json!({ "guild_id": guild_id })).await {
        Ok(g) => g["name"].as_str().unwrap_or_default().to_string(),
        Err(_) => String::new(),
    };
    if !name.is_empty() {
        with_live(|l| l.guild_names.insert(guild_id.to_string(), name.clone()));
    }
    name
}

/// Re-reads the tracked channels; with `notify`, toasts people who joined a
/// watched channel since the last read.
async fn resync(link: &Link, notify: bool) {
    let saved = saved();
    for id in tracked() {
        let data = match link.cmd("GET_CHANNEL", json!({ "channel_id": id })).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("discord: GET_CHANNEL {id}: {e}");
                continue;
            }
        };
        let guild_id = data["guild_id"].as_str().unwrap_or_default().to_string();
        let watched = saved.watched.iter().find(|w| w.id == id);
        let guild = match watched {
            Some(w) if !w.guild_name.is_empty() => w.guild_name.clone(),
            _ if guild_id.is_empty() => "Личный звонок".into(),
            _ => guild_name(link, &guild_id).await,
        };
        let mut members: Vec<Member> = data["voice_states"].as_array().into_iter().flatten().filter_map(parse_member).collect();

        let joined = with_live(|l| {
            let before = l.channels.get(&id);
            for m in &mut members {
                m.speaking = before.and_then(|c| c.members.iter().find(|b| b.id == m.id)).is_some_and(|b| b.speaking);
            }
            let me = l.user.as_ref().map(|u| u.id.clone());
            // Only a channel read before can have news; the first read is just who's there.
            let joined: Vec<String> = match before {
                Some(before) if l.current.as_deref() != Some(id.as_str()) => members
                    .iter()
                    .filter(|m| Some(&m.id) != me.as_ref() && !before.members.iter().any(|b| b.id == m.id))
                    .map(|m| m.name.clone())
                    .collect(),
                _ => Vec::new(),
            };
            l.channels.insert(
                id.clone(),
                Channel {
                    id: id.clone(),
                    guild_id,
                    name: data["name"].as_str().unwrap_or("Канал").to_string(),
                    guild_name: guild,
                    members,
                },
            );
            joined
        });
        if notify && saved.notify && watched.is_some() && !joined.is_empty() {
            toast(&id, &joined);
        }
    }
    changed();
}

fn toast(channel_id: &str, names: &[String]) {
    let Some(app) = APP.get() else { return };
    let Some(channel) = with_live(|l| l.channels.get(channel_id).cloned()) else { return };
    let who = match names {
        [one] => format!("{one} зашёл"),
        [a, b] => format!("{a} и {b} зашли"),
        [a, rest @ ..] => format!("{a} и ещё {} зашли", rest.len()),
        [] => return,
    };
    let present = channel.members.len();
    let body = format!("{} · сейчас в канале: {present}. Нажмите, чтобы зайти.", channel.guild_name);
    let id = channel_id.to_string();
    crate::alerts::notify_then(
        app,
        &format!("{who} в «{}»", channel.name),
        &body,
        Some(Box::new(move || {
            let id = id.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = join(Some(id)).await {
                    eprintln!("discord: join from toast: {e}");
                }
            });
        })),
    );
}

async fn join(channel: Option<String>) -> Result<(), String> {
    let link = link()?;
    link.cmd("SELECT_VOICE_CHANNEL", json!({ "channel_id": channel, "force": true })).await?;
    Ok(())
}

// ---- commands ----------------------------------------------------------------------

#[tauri::command]
pub fn discord_state() -> State {
    snapshot()
}

/// Asks the running client to authorize the user's app, then keeps its tokens.
#[tauri::command]
pub async fn discord_login(client_id: String, client_secret: String) -> Result<State, String> {
    let (client_id, client_secret) = (client_id.trim().to_string(), client_secret.trim().to_string());
    if client_id.is_empty() || !client_id.bytes().all(|b| b.is_ascii_digit()) || client_secret.is_empty() {
        return Err("Вставьте Application ID (только цифры) и Client Secret приложения".into());
    }
    stop_session();
    let (link, rd) = open(&client_id).await?;
    let reader = tauri::async_runtime::spawn(read_loop(link.clone(), rd));
    let authorized = link
        .call("AUTHORIZE", json!({ "client_id": client_id, "scopes": SCOPES }), None, AUTHORIZE_TIMEOUT)
        .await;
    link.stop.notify_one();
    let _ = reader.await;
    let code = match authorized {
        Ok(d) => d["code"].as_str().ok_or("Discord не вернул код входа")?.to_string(),
        Err(e) if e.contains("cancel") || e.contains("denied") => return Err("Вход отменён".into()),
        Err(e) => return Err(e),
    };

    let v = match exchange(&code, &client_id, &client_secret, None).await {
        Ok(v) => v,
        Err(e) if !e.contains("Client Secret") => {
            exchange(&code, &client_id, &client_secret, Some(REDIRECT)).await.map_err(|_| e)?
        }
        Err(e) => return Err(e),
    };
    let s = with_tokens(
        Stored { client_id, client_secret, refresh_token: String::new(), access_token: String::new(), expires_at: 0 },
        &v,
    )?;
    if s.refresh_token.is_empty() {
        return Err("Discord не выдал refresh token".into());
    }
    store(&s)?;
    with_live(|l| l.error = None);
    WAKE.notify_one();
    // Give the session a moment to come up, so the answer already shows it.
    for _ in 0..30 {
        if with_live(|l| l.ready || l.error.is_some()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(snapshot())
}

#[tauri::command]
pub fn discord_logout() -> State {
    secrets::delete(SECRET);
    stop_session();
    with_live(|l| *l = Live::default());
    changed();
    snapshot()
}

#[derive(Serialize)]
pub struct Guild {
    id: String,
    name: String,
    icon: Option<String>,
}

#[tauri::command]
pub async fn discord_guilds() -> Result<Vec<Guild>, String> {
    let data = link()?.cmd("GET_GUILDS", json!({})).await?;
    Ok(data["guilds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            Some(Guild {
                id: g["id"].as_str()?.to_string(),
                name: g["name"].as_str()?.to_string(),
                icon: g["icon_url"].as_str().map(str::to_string),
            })
        })
        .collect())
}

/// The guild's voice and stage channels.
#[tauri::command]
pub async fn discord_channels(guild_id: String) -> Result<Vec<WatchedChannel>, String> {
    let link = link()?;
    let data = link.cmd("GET_CHANNELS", json!({ "guild_id": guild_id })).await?;
    let guild_name = guild_name(&link, &guild_id).await;
    Ok(data["channels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| matches!(c["type"].as_u64(), Some(2 | 13)))
        .filter_map(|c| {
            Some(WatchedChannel {
                id: c["id"].as_str()?.to_string(),
                guild_id: guild_id.clone(),
                name: c["name"].as_str()?.to_string(),
                guild_name: guild_name.clone(),
            })
        })
        .collect())
}

#[tauri::command]
pub async fn discord_set_watched(app: AppHandle, channels: Vec<WatchedChannel>) -> Result<State, String> {
    let before: HashSet<String> = tracked().into_iter().collect();
    update_saved(&app, |s| s.watched = channels)?;
    if let Ok(link) = link() {
        let after: HashSet<String> = tracked().into_iter().collect();
        for id in before.difference(&after) {
            subscribe(&link, id, false).await;
            with_live(|l| l.channels.remove(id));
        }
        for id in after.difference(&before) {
            subscribe(&link, id, true).await;
        }
        resync(&link, false).await;
    }
    changed();
    Ok(snapshot())
}

#[tauri::command]
pub fn discord_set_notify(app: AppHandle, on: bool) -> Result<State, String> {
    update_saved(&app, |s| s.notify = on)?;
    changed();
    Ok(snapshot())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceChange {
    mute: Option<bool>,
    deaf: Option<bool>,
    input_volume: Option<f64>,
    mode: Option<String>,
    input_device: Option<String>,
}

#[tauri::command]
pub async fn discord_voice(change: VoiceChange) -> Result<State, String> {
    let mut args = json!({});
    if let Some(m) = change.mute {
        args["mute"] = json!(m);
    }
    if let Some(d) = change.deaf {
        args["deaf"] = json!(d);
    }
    if let Some(v) = change.input_volume {
        args["input"]["volume"] = json!(v.clamp(0.0, 100.0));
    }
    if let Some(d) = change.input_device {
        args["input"]["device_id"] = json!(d);
    }
    if let Some(m) = change.mode {
        args["mode"]["type"] = json!(m);
    }
    let voice = link()?.cmd("SET_VOICE_SETTINGS", args).await?;
    with_live(|l| l.voice = Some(parse_voice(&voice)));
    changed();
    Ok(snapshot())
}

/// Joins a voice channel, or leaves the current one with `None`.
#[tauri::command]
pub async fn discord_join(channel_id: Option<String>) -> Result<(), String> {
    join(channel_id).await
}

/// The microphone for the taskbar button, `(muted, speaking)`, while the user
/// is in a voice channel. Deafened counts as muted, as in Discord.
pub fn mic_state() -> Option<(bool, bool)> {
    with_live(|l| {
        let voice = l.voice.as_ref().filter(|_| l.ready)?;
        let current = l.current.as_ref()?;
        let me = l.user.as_ref().map(|u| u.id.as_str());
        let speaking = l
            .channels
            .get(current)
            .is_some_and(|c| c.members.iter().any(|m| Some(m.id.as_str()) == me && m.speaking));
        Some((voice.mute || voice.deaf, speaking))
    })
}

// ---- the mute shortcut -------------------------------------------------------------

/// Whether the global shortcut that fired is the microphone toggle.
pub fn is_mute_shortcut(shortcut: &Shortcut) -> bool {
    saved().mute_shortcut.and_then(|t| t.parse::<Shortcut>().ok()).is_some_and(|sc| sc == *shortcut)
}

pub fn toggle_mute() {
    tauri::async_runtime::spawn(async {
        let Some(mute) = with_live(|l| l.voice.as_ref().map(|v| v.mute)) else { return };
        let change = VoiceChange { mute: Some(!mute), deaf: None, input_volume: None, mode: None, input_device: None };
        if let Err(e) = discord_voice(change).await {
            eprintln!("discord: mute shortcut: {e}");
        }
    });
}

/// Swaps the microphone shortcut; `None` removes it.
#[tauri::command]
pub fn discord_set_mute_shortcut(app: AppHandle, shortcut: Option<String>) -> Result<State, String> {
    let new = match shortcut.as_deref() {
        Some(t) => Some(t.parse::<Shortcut>().map_err(|e| format!("Не получилось разобрать сочетание: {e}"))?),
        None => None,
    };
    let gs = app.global_shortcut();
    if new.is_some_and(|n| gs.is_registered(n)) && saved().mute_shortcut != shortcut {
        return Err("Это сочетание уже занято панелью".into());
    }
    if let Some(old) = saved().mute_shortcut.and_then(|t| t.parse::<Shortcut>().ok()) {
        let _ = gs.unregister(old);
    }
    if let Some(n) = new {
        gs.register(n).map_err(|e| format!("Сочетание занято другой программой ({e})"))?;
    }
    update_saved(&app, |s| s.mute_shortcut = shortcut)?;
    Ok(snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_voice_state() {
        let v = json!({
            "nick": "Капитан",
            "mute": false,
            "volume": 100,
            "voice_state": { "mute": false, "deaf": false, "self_mute": true, "self_deaf": false, "suppress": false },
            "user": { "id": "80351110224678912", "username": "nelly", "global_name": "Nelly", "avatar": null }
        });
        let m = parse_member(&v).unwrap();
        assert_eq!(m.name, "Капитан");
        assert!(m.muted && !m.deafened);
        assert_eq!(m.avatar, "https://cdn.discordapp.com/embed/avatars/5.png");
        let named = parse_user(&json!({ "id": "1", "username": "nelly", "global_name": null, "avatar": "abc" })).unwrap();
        assert_eq!(named.name, "nelly");
        assert_eq!(named.avatar, "https://cdn.discordapp.com/avatars/1/abc.png?size=64");
    }

    #[test]
    fn parses_voice_settings() {
        let v = parse_voice(&json!({
            "input": { "device_id": "default", "volume": 72.5, "available_devices": [{ "id": "default", "name": "Default" }] },
            "mode": { "type": "PUSH_TO_TALK" },
            "mute": true,
            "deaf": false
        }));
        assert!(v.mute && !v.deaf);
        assert_eq!((v.input_volume, v.mode.as_str(), v.input_devices.len()), (72.5, "PUSH_TO_TALK", 1));
    }
}
