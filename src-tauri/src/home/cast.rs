//! Google Cast speakers and displays on the LAN: found over mDNS
//! (`_googlecast._tcp`), controlled over the Cast v2 protocol: TLS to port
//! 8009 with the device's self-signed certificate, length-prefixed protobuf
//! `CastMessage`s carrying JSON. Each call opens a short-lived connection.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use native_tls::{TlsConnector, TlsStream};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const IO_TIMEOUT: Duration = Duration::from_secs(3);
const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
const SENDER: &str = "sender-0";
const RECEIVER: &str = "receiver-0";
/// `supportedMediaCommands` bits.
const CMD_QUEUE_NEXT: u64 = 64;
const CMD_QUEUE_PREV: u64 = 128;

#[derive(Serialize, Deserialize, Clone)]
pub struct Known {
    pub ip: Ipv4Addr,
    pub port: u16,
    /// Name set in the Google Home app.
    pub name: String,
    pub model: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerState {
    /// 0.0–1.0.
    pub volume: f64,
    pub muted: bool,
    /// Running app, e.g. "Spotify"; none when idle.
    pub app: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub image: Option<String>,
    /// "PLAYING", "PAUSED", "BUFFERING" or "IDLE" when an app reports media.
    pub player_state: Option<String>,
    pub can_next: bool,
    pub can_prev: bool,
}

// ---- discovery ---------------------------------------------------------------------

/// Cast devices announcing themselves within `wait`, keyed by their Cast id.
pub fn discover(wait: Duration) -> HashMap<String, Known> {
    let mut found = HashMap::new();
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else {
        return found;
    };
    let Ok(events) = daemon.browse("_googlecast._tcp.local.") else {
        return found;
    };
    let deadline = Instant::now() + wait;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            let Some(ip) = info.get_addresses_v4().into_iter().next() else {
                continue;
            };
            let txt = |k: &str| info.get_property_val_str(k).unwrap_or_default().to_string();
            let id = Some(txt("id"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| info.get_fullname().to_string());
            found.insert(
                id,
                Known {
                    ip,
                    port: info.get_port(),
                    name: txt("fn"),
                    model: txt("md"),
                },
            );
        }
    }
    let _ = daemon.shutdown();
    found
}

// ---- wire format -------------------------------------------------------------------

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn string_field(out: &mut Vec<u8>, tag: u64, s: &str) {
    varint(out, tag << 3 | 2);
    varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// `CastMessage { protocol_version: CASTV2_1_0, source_id, destination_id, namespace, payload_type: STRING, payload_utf8 }`
fn encode(dest: &str, namespace: &str, payload: &Value) -> Vec<u8> {
    let mut msg = Vec::new();
    varint(&mut msg, 1 << 3);
    varint(&mut msg, 0);
    string_field(&mut msg, 2, SENDER);
    string_field(&mut msg, 3, dest);
    string_field(&mut msg, 4, namespace);
    varint(&mut msg, 5 << 3);
    varint(&mut msg, 0);
    string_field(&mut msg, 6, &payload.to_string());
    let mut framed = (msg.len() as u32).to_be_bytes().to_vec();
    framed.extend(msg);
    framed
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*pos)?;
        *pos += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// `(namespace, payload)` of a `CastMessage`; other fields are skipped.
fn decode(buf: &[u8]) -> Option<(String, Value)> {
    let (mut pos, mut namespace, mut payload) = (0, String::new(), Value::Null);
    while pos < buf.len() {
        let key = read_varint(buf, &mut pos)?;
        match key & 7 {
            0 => {
                read_varint(buf, &mut pos)?;
            }
            2 => {
                let len = read_varint(buf, &mut pos)? as usize;
                let bytes = buf.get(pos..pos + len)?;
                pos += len;
                match key >> 3 {
                    4 => namespace = String::from_utf8_lossy(bytes).into_owned(),
                    6 => payload = serde_json::from_slice(bytes).unwrap_or(Value::Null),
                    _ => {}
                }
            }
            _ => return None,
        }
    }
    Some((namespace, payload))
}

// ---- session -----------------------------------------------------------------------

struct Session {
    tls: TlsStream<TcpStream>,
    next_id: u64,
}

impl Session {
    fn open(device: &Known) -> Result<Self, String> {
        let tcp =
            TcpStream::connect_timeout(&SocketAddr::from((device.ip, device.port)), IO_TIMEOUT)
                .map_err(|e| format!("колонка не отвечает: {e}"))?;
        tcp.set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|e| e.to_string())?;
        // Cast devices present a self-signed certificate.
        let connector = TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|e| e.to_string())?;
        let tls = connector
            .connect(&device.ip.to_string(), tcp)
            .map_err(|e| e.to_string())?;
        let mut session = Session { tls, next_id: 1 };
        session.send(RECEIVER, NS_CONNECTION, &json!({ "type": "CONNECT" }))?;
        Ok(session)
    }

    fn send(&mut self, dest: &str, namespace: &str, payload: &Value) -> Result<(), String> {
        self.tls
            .write_all(&encode(dest, namespace, payload))
            .map_err(|e| e.to_string())
    }

    fn read(&mut self) -> Result<(String, Value), String> {
        let mut len = [0u8; 4];
        self.tls.read_exact(&mut len).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; u32::from_be_bytes(len) as usize];
        self.tls.read_exact(&mut buf).map_err(|e| e.to_string())?;
        decode(&buf).ok_or_else(|| "непонятный ответ колонки".into())
    }

    /// Sends a request and waits for the reply carrying the same `requestId`.
    fn request(
        &mut self,
        dest: &str,
        namespace: &str,
        mut payload: Value,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        payload["requestId"] = json!(id);
        self.send(dest, namespace, &payload)?;
        loop {
            let (ns, reply) = self.read()?;
            if ns == NS_HEARTBEAT && reply["type"] == "PING" {
                self.send(RECEIVER, NS_HEARTBEAT, &json!({ "type": "PONG" }))?;
            } else if reply["requestId"] == id {
                return Ok(reply);
            }
        }
    }
}

/// The app currently casting, if it speaks the standard media namespace.
struct MediaApp {
    name: String,
    transport: String,
    session: Option<Value>,
}

fn status(s: &mut Session) -> Result<(SpeakerState, Option<MediaApp>), String> {
    let reply = s.request(RECEIVER, NS_RECEIVER, json!({ "type": "GET_STATUS" }))?;
    let status = &reply["status"];
    let mut state = SpeakerState {
        volume: status["volume"]["level"].as_f64().unwrap_or(0.0),
        muted: status["volume"]["muted"] == true,
        ..Default::default()
    };
    let app = status["applications"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["isIdleScreen"] != true);
    let Some(app) = app else {
        return Ok((state, None));
    };
    state.app = app["displayName"].as_str().map(str::to_string);

    let speaks_media = app["namespaces"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|n| n["name"] == NS_MEDIA);
    let Some(transport) = app["transportId"].as_str().filter(|_| speaks_media) else {
        state.title = app["statusText"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        return Ok((state, None));
    };
    s.send(transport, NS_CONNECTION, &json!({ "type": "CONNECT" }))?;
    let media = s.request(transport, NS_MEDIA, json!({ "type": "GET_STATUS" }))?;
    let session = media["status"].as_array().and_then(|a| a.first()).cloned();
    if let Some(m) = &session {
        let meta = &m["media"]["metadata"];
        state.title = meta["title"].as_str().map(str::to_string);
        state.artist = meta["artist"]
            .as_str()
            .or(meta["albumArtist"].as_str())
            .map(str::to_string);
        state.image = meta["images"][0]["url"].as_str().map(str::to_string);
        state.player_state = m["playerState"].as_str().map(str::to_string);
        let commands = m["supportedMediaCommands"].as_u64().unwrap_or(0);
        state.can_next = commands & CMD_QUEUE_NEXT != 0;
        state.can_prev = commands & CMD_QUEUE_PREV != 0;
    }
    let name = state.app.clone().unwrap_or_default();
    Ok((
        state,
        Some(MediaApp {
            name,
            transport: transport.to_string(),
            session,
        }),
    ))
}

pub fn state(device: &Known) -> Result<SpeakerState, String> {
    status(&mut Session::open(device)?).map(|(state, _)| state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum Control {
    Play,
    Pause,
    Next,
    Prev,
    Volume { level: f64 },
    Mute { muted: bool },
}

pub fn control(device: &Known, control: &Control) -> Result<(), String> {
    let mut s = Session::open(device)?;
    match control {
        Control::Volume { level } => {
            s.request(
                RECEIVER,
                NS_RECEIVER,
                json!({ "type": "SET_VOLUME", "volume": { "level": level.clamp(0.0, 1.0) } }),
            )?;
        }
        Control::Mute { muted } => {
            s.request(
                RECEIVER,
                NS_RECEIVER,
                json!({ "type": "SET_VOLUME", "volume": { "muted": muted } }),
            )?;
        }
        Control::Play | Control::Pause | Control::Next | Control::Prev => {
            let (_, app) = status(&mut s)?;
            let app = app.ok_or("на колонке ничего не играет")?;
            let session_id = app
                .session
                .as_ref()
                .and_then(|m| m["mediaSessionId"].as_u64());
            let session_id = session_id
                .ok_or_else(|| format!("{} не даёт управлять воспроизведением", app.name))?;
            let kind = match control {
                Control::Play => "PLAY",
                Control::Pause => "PAUSE",
                Control::Next => "QUEUE_NEXT",
                _ => "QUEUE_PREV",
            };
            s.request(
                &app.transport,
                NS_MEDIA,
                json!({ "type": kind, "mediaSessionId": session_id }),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trip() {
        let framed = encode(
            "receiver-0",
            NS_RECEIVER,
            &json!({ "type": "GET_STATUS", "requestId": 7 }),
        );
        let len = u32::from_be_bytes(framed[..4].try_into().unwrap()) as usize;
        assert_eq!(len, framed.len() - 4);
        let (ns, payload) = decode(&framed[4..]).unwrap();
        assert_eq!(ns, NS_RECEIVER);
        assert_eq!(payload["requestId"], 7);
    }

    #[test]
    fn long_payload_uses_multibyte_lengths() {
        let big = "x".repeat(300);
        let framed = encode("receiver-0", NS_MEDIA, &json!({ "title": big }));
        let (_, payload) = decode(&framed[4..]).unwrap();
        assert_eq!(payload["title"].as_str().unwrap().len(), 300);
    }
}
