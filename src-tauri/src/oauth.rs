//! Loopback OAuth helpers shared by the Spotify and Google sign-ins: the
//! browser is sent to the provider, which redirects back to a listener on
//! 127.0.0.1 with the code (RFC 8252), protected with PKCE and `state`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use base64::prelude::{Engine, BASE64_URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub fn random_urlsafe(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(BASE64_URL_SAFE_NO_PAD.encode(buf))
}

/// A PKCE verifier and its S256 challenge.
pub fn pkce() -> Result<(String, String), String> {
    let verifier = random_urlsafe(48)?;
    let challenge = BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Ok((verifier, challenge))
}

/// Waits for the browser's redirect to `path` on the loopback listener and
/// returns its query string. Other requests (favicon) get a 404.
pub fn wait_for_redirect(listener: TcpListener, path: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let prefix = format!("{path}?");
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let target = request.split_whitespace().nth(1).unwrap_or("");
                let Some(query) = target.strip_prefix(prefix.as_str()) else {
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

pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
    })
}
