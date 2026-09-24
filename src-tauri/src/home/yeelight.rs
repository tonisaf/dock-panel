//! Yeelight LAN protocol: SSDP-style discovery on 239.255.255.250:1982 and
//! JSON commands over TCP 55443. Works only for lamps with "LAN Control"
//! turned on in the Yeelight app.
//! https://www.yeelight.com/download/Yeelight_Inter-Operation_Spec.pdf

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use socket2::{Domain, Protocol, Socket, Type};

const MULTICAST: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const PORT: u16 = 1982;
const IO_TIMEOUT: Duration = Duration::from_secs(3);
/// Smooth transitions, in ms: the lamp fades instead of jumping.
const FADE: u32 = 300;
/// Colour temperature range every model accepts (some go down to 1700 K).
pub const CT_MIN: u16 = 2700;
pub const CT_MAX: u16 = 6500;

/// What discovery learns about a lamp; kept on disk so lamps show up at once.
#[derive(Serialize, Deserialize, Clone)]
pub struct Known {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub model: String,
    /// Methods the lamp supports, from its discovery reply.
    pub support: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LampState {
    pub power: bool,
    /// 1–100.
    pub bright: u8,
    /// Kelvin.
    pub ct: u16,
    /// 0xRRGGBB.
    pub rgb: u32,
    /// 1 = RGB, 2 = colour temperature, 3 = HSV.
    pub color_mode: u8,
}

/// Lamps answering an M-SEARCH on any of this PC's LAN interfaces within `wait`.
/// Sent per interface: with a VPN up, the default multicast route is the tunnel.
pub fn discover(local_ips: &[Ipv4Addr], wait: Duration) -> HashMap<String, Known> {
    let request = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {MULTICAST}:{PORT}\r\nMAN: \"ssdp:discover\"\r\nST: wifi_bulb\r\n"
    );
    let sockets: Vec<UdpSocket> = local_ips
        .iter()
        .filter_map(|ip| {
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
            socket.bind(&SocketAddr::from((*ip, 0)).into()).ok()?;
            socket.set_multicast_if_v4(ip).ok()?;
            socket.set_read_timeout(Some(Duration::from_millis(100))).ok()?;
            let socket: UdpSocket = socket.into();
            socket.send_to(request.as_bytes(), SocketAddrV4::new(MULTICAST, PORT)).ok()?;
            Some(socket)
        })
        .collect();

    let mut found = HashMap::new();
    let deadline = Instant::now() + wait;
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        for socket in &sockets {
            while let Ok((n, _)) = socket.recv_from(&mut buf) {
                if let Some((id, known)) = parse_reply(&String::from_utf8_lossy(&buf[..n])) {
                    found.insert(id, known);
                }
            }
        }
    }
    found
}

/// HTTP-like headers: `Location: yeelight://192.168.1.20:55443`, `id: 0x…`, `model: color`, `support: …`.
fn parse_reply(text: &str) -> Option<(String, Known)> {
    let headers: HashMap<String, &str> = text
        .lines()
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim()))
        .collect();
    let location = headers.get("location")?.strip_prefix("yeelight://")?;
    let addr: SocketAddrV4 = location.parse().ok()?;
    Some((
        headers.get("id")?.to_string(),
        Known {
            ip: *addr.ip(),
            port: addr.port(),
            model: headers.get("model").unwrap_or(&"").to_string(),
            support: headers.get("support").unwrap_or(&"").split_whitespace().map(str::to_string).collect(),
        },
    ))
}

/// One command on a fresh connection; returns `result`. Lamps also push
/// `props` notifications on the socket, which are skipped.
pub fn call(lamp: &Known, method: &str, params: Value) -> Result<Value, String> {
    let addr = SocketAddr::from((lamp.ip, lamp.port));
    let mut stream = TcpStream::connect_timeout(&addr, IO_TIMEOUT).map_err(|e| format!("лампа не отвечает: {e}"))?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(|e| e.to_string())?;
    let request = json!({ "id": 1, "method": method, "params": params });
    stream.write_all(format!("{request}\r\n").as_bytes()).map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("лампа закрыла соединение".into());
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["id"] != 1 {
            continue;
        }
        if let Some(err) = v["error"]["message"].as_str() {
            return Err(format!("лампа: {err}"));
        }
        return Ok(v["result"].clone());
    }
}

pub fn state(lamp: &Known) -> Result<LampState, String> {
    let r = call(lamp, "get_prop", json!(["power", "bright", "ct", "rgb", "color_mode"]))?;
    let num = |i: usize| r[i].as_str().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    Ok(LampState {
        power: r[0] == "on",
        bright: num(1).clamp(1, 100) as u8,
        ct: num(2) as u16,
        rgb: num(3),
        color_mode: num(4) as u8,
    })
}

/// Changes requested from the UI; unset fields stay as they are.
#[derive(Deserialize, Default)]
pub struct Change {
    pub power: Option<bool>,
    pub bright: Option<u8>,
    pub ct: Option<u16>,
    pub rgb: Option<u32>,
}

/// Brightness and colour are accepted only while the lamp is on, so moving a
/// slider on a lamp that is off turns it on first.
pub fn apply(lamp: &Known, change: &Change) -> Result<(), String> {
    let adjusts = change.bright.is_some() || change.ct.is_some() || change.rgb.is_some();
    match change.power {
        Some(false) => return call(lamp, "set_power", json!(["off", "smooth", FADE])).map(drop),
        Some(true) => {
            call(lamp, "set_power", json!(["on", "smooth", FADE]))?;
        }
        None if adjusts => {
            call(lamp, "set_power", json!(["on", "smooth", FADE]))?;
        }
        None => {}
    }
    if let Some(b) = change.bright {
        call(lamp, "set_bright", json!([b.clamp(1, 100), "smooth", FADE]))?;
    }
    if let Some(ct) = change.ct {
        call(lamp, "set_ct_abx", json!([ct.clamp(CT_MIN, CT_MAX), "smooth", FADE]))?;
    }
    if let Some(rgb) = change.rgb {
        call(lamp, "set_rgb", json!([rgb.clamp(1, 0xFF_FF_FF), "smooth", FADE]))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_discovery_reply() {
        let reply = "HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nDate: \r\nExt: \r\n\
            Location: yeelight://192.168.1.239:55443\r\nServer: POSIX UPnP/1.0 YGLC/1\r\n\
            id: 0x000000000015243f\r\nmodel: color\r\nfw_ver: 18\r\n\
            support: get_prop set_default set_power toggle set_bright start_cf stop_cf set_scene cron_add \
            cron_get cron_del set_ct_abx set_rgb\r\npower: on\r\nbright: 100\r\ncolor_mode: 2\r\nct: 4000\r\n\
            rgb: 16711680\r\nhue: 100\r\nsat: 35\r\nname: my_bulb\r\n";
        let (id, known) = parse_reply(reply).unwrap();
        assert_eq!(id, "0x000000000015243f");
        assert_eq!(known.ip, Ipv4Addr::new(192, 168, 1, 239));
        assert_eq!(known.port, 55443);
        assert_eq!(known.model, "color");
        assert!(known.support.iter().any(|s| s == "set_rgb"));
    }
}
