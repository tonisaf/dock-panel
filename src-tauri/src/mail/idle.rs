//! Independent IMAP IDLE connections; UI sessions never wait behind IDLE.
use std::collections::HashMap;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use super::{client, load, poll_selected, secret_key};

#[derive(Default)]
struct Watcher {
    stopped: AtomicBool,
    healthy: AtomicBool,
    socket: Mutex<Option<TcpStream>>,
}

impl Watcher {
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.healthy.store(false, Ordering::SeqCst);
        if let Some(socket) = self.socket.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

static WATCHERS: Mutex<Option<HashMap<String, Arc<Watcher>>>> = Mutex::new(None);

pub fn healthy(id: &str) -> bool {
    WATCHERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(id))
        .is_some_and(|w| w.healthy.load(Ordering::SeqCst))
}

pub fn restart(id: &str) {
    if let Some(watcher) = WATCHERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .and_then(|m| m.remove(id))
    {
        watcher.stop();
    }
}

pub fn reconcile(app: &AppHandle) {
    let accounts = load(app).accounts;
    let mut guard = WATCHERS.lock().unwrap_or_else(|e| e.into_inner());
    let watchers = guard.get_or_insert_with(HashMap::new);
    watchers.retain(|id, w| {
        if accounts.iter().any(|a| a.id == *id) {
            true
        } else {
            w.stop();
            false
        }
    });
    for account in accounts {
        if watchers.contains_key(&account.id) {
            continue;
        }
        let watcher = Arc::new(Watcher::default());
        watchers.insert(account.id.clone(), watcher.clone());
        let app = app.clone();
        std::thread::spawn(move || watch(&app, account, &watcher));
    }
}

fn watch(app: &AppHandle, account: client::Account, watcher: &Watcher) {
    let mut backoff = 5;
    while !watcher.stopped.load(Ordering::SeqCst) {
        let result = (|| -> Result<(), String> {
            let password =
                crate::secrets::read(&secret_key(&account.id)).ok_or("Пароль не найден")?;
            let (mut session, socket) = client::connect_watcher(&account, &password)?;
            {
                let mut slot = watcher.socket.lock().unwrap_or_else(|e| e.into_inner());
                if watcher.stopped.load(Ordering::SeqCst) {
                    return Ok(());
                }
                *slot = Some(socket);
            }
            let capabilities = session.capabilities().map_err(|e| e.to_string())?;
            if !capabilities.has_str("IDLE") {
                let _ = session.logout();
                // Keep the registry entry so the minute poll remains the fallback.
                return Ok(());
            }
            session.examine("INBOX").map_err(|e| e.to_string())?;
            poll_selected(app, Some(&account.id));
            while !watcher.stopped.load(Ordering::SeqCst) {
                watcher.healthy.store(true, Ordering::SeqCst);
                let result = {
                    let socket = watcher
                        .socket
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                        .and_then(|s| s.try_clone().ok())
                        .ok_or("Соединение закрыто")?;
                    wait_for_change(&mut session, &socket, Duration::from_secs(25 * 60))
                };
                watcher.healthy.store(false, Ordering::SeqCst);
                if watcher.stopped.load(Ordering::SeqCst) {
                    return Ok(());
                }
                result.map_err(|e| e.to_string())?;
                backoff = 5;
                poll_selected(app, Some(&account.id));
                let _ = app.emit("mail:changed", ());
            }
            Ok(())
        })();
        watcher.healthy.store(false, Ordering::SeqCst);
        watcher
            .socket
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if result.is_ok() {
            return;
        }
        // Retry after network loss / resume, without a busy loop. Minute polling
        // remains active while this watcher cannot establish IDLE.
        for _ in 0..backoff {
            if watcher.stopped.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        backoff = (backoff * 2).min(60);
    }
}

fn wait_for_change<T: std::io::Read + std::io::Write + imap::extensions::idle::SetReadTimeout>(
    session: &mut imap::Session<T>,
    socket: &TcpStream,
    timeout: Duration,
) -> imap::Result<imap::extensions::idle::WaitOutcome> {
    let mut idle = session.idle();
    idle.timeout(timeout).keepalive(false);
    let result = idle.wait_while(|_| false);
    // Bound the DONE handshake: the library clears read timeout after waiting.
    let _ = socket.set_read_timeout(Some(Duration::from_secs(30)));
    drop(idle);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn exercise_idle(event: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let socket = stream.try_clone().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            stream.write_all(b"* OK ready\r\n").unwrap();
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let tag = line.split_whitespace().next().unwrap();
            write!(stream, "{tag} OK logged in\r\n").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert!(line.contains("IDLE"));
            let tag = line.split_whitespace().next().unwrap().to_string();
            stream.write_all(b"+ idling\r\n").unwrap();
            if event {
                stream.write_all(b"* 1 EXISTS\r\n").unwrap();
            }
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "DONE");
            write!(stream, "{tag} OK idle done\r\n").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert!(line.contains("NOOP"));
            let tag = line.split_whitespace().next().unwrap();
            write!(stream, "{tag} OK noop done\r\n").unwrap();
        });
        let mut client = imap::Client::new(stream);
        client.read_greeting().unwrap();
        let mut session = client.login("test", "test").map_err(|(e, _)| e).unwrap();
        let result = wait_for_change(&mut session, &socket, Duration::from_millis(100)).unwrap();
        assert_eq!(
            matches!(result, imap::extensions::idle::WaitOutcome::TimedOut),
            !event
        );
        session.noop().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn mailbox_event_ends_idle_and_leaves_session_usable() {
        exercise_idle(true);
    }

    #[test]
    fn idle_timeout_ends_cleanly_for_renewal() {
        exercise_idle(false);
    }

    #[test]
    fn removing_account_interrupts_blocked_network_read() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let watcher = Watcher::default();
        *watcher.socket.lock().unwrap() = Some(stream.try_clone().unwrap());
        watcher.healthy.store(true, Ordering::SeqCst);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let read = std::thread::spawn(move || {
            let mut stream = stream;
            stream.read(&mut [0u8; 1])
        });
        watcher.stop();
        assert!(matches!(read.join().unwrap(), Ok(0)));
        assert!(!watcher.healthy.load(Ordering::SeqCst));
        assert!(watcher.stopped.load(Ordering::SeqCst));
    }
}
