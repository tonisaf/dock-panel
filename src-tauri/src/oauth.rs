//! Loopback OAuth helpers shared by the Spotify and Google sign-ins: the
//! browser is sent to the provider, which redirects back to a listener on
//! 127.0.0.1 with the code (RFC 8252), protected with PKCE and `state`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use base64::prelude::{Engine, BASE64_URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Enough for an authorization code, `state` and a long error description.
const MAX_HEAD: usize = 16 * 1024;

/// How long one connection gets to send its request head. The per-read timeout alone
/// lets a client that drips a byte at a time hold the listener for minutes.
const HEAD_DEADLINE: Duration = Duration::from_secs(10);

/// Reads until the end of the request head (`\r\n\r\n`), the cap, `deadline`, EOF or an error.
/// One `read` is not enough: the request can arrive in several TCP segments. The deadline is
/// checked between reads, so it can be overrun by one read timeout.
fn read_head(stream: &mut impl Read, deadline: Instant) -> String {
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while head.len() < MAX_HEAD
        && Instant::now() < deadline
        && !head.windows(4).any(|w| w == b"\r\n\r\n")
    {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

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
                let request = read_head(&mut stream, Instant::now() + HEAD_DEADLINE);
                let target = request.split_whitespace().nth(1).unwrap_or("");
                let Some(query) = target.strip_prefix(prefix.as_str()) else {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
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
        (k == key).then(|| {
            percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .into_owned()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    /// Hands out the request in fixed pieces, like a slow TCP stream.
    struct Pieces<'a>(Vec<&'a [u8]>);

    impl Read for Pieces<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let piece = self.0.remove(0);
            buf[..piece.len()].copy_from_slice(piece);
            Ok(piece.len())
        }
    }

    #[test]
    fn joins_a_request_split_across_reads() {
        let mut stream = Pieces(vec![
            b"GET /callback?code=ab",
            b"c&state=xyz HTTP/1.1\r\nHo",
            b"st: 127.0.0.1\r\n\r\n",
        ]);
        let head = read_head(&mut stream, far());
        let target = head.split_whitespace().nth(1).unwrap();
        assert_eq!(target, "/callback?code=abc&state=xyz");
    }

    #[test]
    fn reads_a_head_longer_than_one_chunk() {
        let long = "a".repeat(5000);
        let request = format!("GET /callback?code={long} HTTP/1.1\r\n\r\n");
        let head = read_head(&mut request.as_bytes(), far());
        assert!(head.contains(&long));
    }

    #[test]
    fn stops_at_the_cap_without_a_terminator() {
        let endless = vec![b'a'; 100 * 1024];
        assert!(read_head(&mut endless.as_slice(), far()).len() <= MAX_HEAD + 2048);
    }

    #[test]
    fn query_param_decodes() {
        assert_eq!(
            query_param("code=a%20b&state=s", "code").as_deref(),
            Some("a b")
        );
        assert_eq!(query_param("code=a", "state"), None);
    }

    #[test]
    fn gives_up_on_a_client_that_drips_bytes() {
        struct Drip;
        impl Read for Drip {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                std::thread::sleep(Duration::from_millis(20));
                buf[0] = b'a';
                Ok(1)
            }
        }
        let started = Instant::now();
        let head = read_head(&mut Drip, started + Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!head.is_empty() && head.len() < 100);
    }

    #[test]
    fn does_not_read_after_the_deadline() {
        let mut stream = Pieces(vec![b"GET / HTTP/1.1\r\n\r\n"]);
        assert!(read_head(&mut stream, Instant::now()).is_empty());
    }
}
