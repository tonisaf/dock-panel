use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub const CREDENTIAL: &str = "DockPanel/youtube-push";
static HEALTHY: AtomicBool = AtomicBool::new(false);
static STATUS: Mutex<String> = Mutex::new(String::new());

pub fn status() -> String {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
fn update(app: &AppHandle, healthy: bool, text: &str) {
    HEALTHY.store(healthy, Ordering::Relaxed);
    let mut status = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    if *status != text {
        *status = text.into();
        let _ = app.emit("youtube:changed", ());
    }
}
pub fn refresh_interval() -> Duration {
    if HEALTHY.load(Ordering::Relaxed) {
        Duration::from_secs(3600)
    } else {
        REFRESH_EVERY
    }
}
fn validate_url(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value.trim()).map_err(|_| "Некорректный адрес сервера")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Укажите HTTPS-адрес сервера без пути, параметров и пароля".into());
    }
    Ok(url.as_str().trim_end_matches('/').into())
}

#[tauri::command]
pub fn youtube_set_push(
    app: AppHandle,
    enabled: bool,
    url: String,
    token: Option<String>,
) -> Result<(), String> {
    let url = if url.trim().is_empty() && !enabled {
        String::new()
    } else {
        validate_url(&url)?
    };
    let mut saved = load(&app);
    let changed = saved.push_url != url;
    let supplied = token.filter(|t| !t.is_empty());
    if enabled
        && supplied.as_ref().map_or_else(
            || changed || crate::secrets::read(CREDENTIAL).is_none(),
            |t| t.len() < 32,
        )
    {
        return Err("Укажите токен нового сервера (не менее 32 символов)".into());
    }
    if let Some(token) = supplied {
        crate::secrets::write(CREDENTIAL, &token)?;
    } else if changed {
        crate::secrets::delete(CREDENTIAL);
    }
    saved.push_enabled = enabled;
    saved.push_url = url;
    save(&app, &saved)?;
    update(
        &app,
        false,
        if enabled {
            "Подключение…"
        } else {
            "Опрос раз в 15 минут"
        },
    );
    Ok(())
}

#[derive(Deserialize)]
struct Events {
    cursor: u64,
    channels: HashSet<String>,
    reset: bool,
}
#[derive(Deserialize)]
struct Lease {
    id: String,
    active: i32,
    lease: f64,
}
#[derive(Deserialize)]
struct Leases {
    channels: Vec<Lease>,
}
#[derive(Default, Serialize, Deserialize)]
struct Cursor {
    url: String,
    value: u64,
}

fn leases_ready(ids: &HashSet<String>, leases: &[Lease], now: f64) -> bool {
    !ids.is_empty()
        && ids.iter().all(|id| {
            leases
                .iter()
                .any(|l| &l.id == id && l.active == 1 && l.lease > now + 60.0)
        })
}

async fn cycle(app: &AppHandle, saved: &Saved, token: &str, cursor: &mut Cursor) -> Result<(), ()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ())?;
    let ids: HashSet<String> = saved.channels.iter().map(|c| c.id.clone()).collect();
    client
        .post(format!("{}/channels", saved.push_url))
        .bearer_auth(token)
        .json(&serde_json::json!({ "channels": ids }))
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?;
    let leases: Leases = client
        .get(format!("{}/status", saved.push_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())?;
    let now = now_ms() as f64 / 1000.0;
    let healthy = leases_ready(&ids, &leases.channels, now);
    update(
        app,
        healthy,
        if healthy {
            "Сервер подключён; контрольный опрос раз в час"
        } else {
            "Ожидание подписок; опрос раз в 15 минут"
        },
    );
    if cursor.url != saved.push_url {
        cursor.url = saved.push_url.clone();
        cursor.value = 0;
    }
    let events: Events = client
        .get(format!("{}/events", saved.push_url))
        .bearer_auth(token)
        .query(&[("after", cursor.value)])
        .timeout(Duration::from_secs(35))
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())?;
    // Ignore responses from a server disabled or replaced while the request was running.
    let current = load(app);
    if !current.push_enabled || current.push_url != saved.push_url {
        return Ok(());
    }
    let selected = if events.reset {
        ids
    } else {
        events.channels.intersection(&ids).cloned().collect()
    };
    if !selected.is_empty() {
        let (fresh, success) = refresh_selected(app, Some(&selected)).await;
        notify_new(app, fresh);
        if !success {
            return Err(());
        }
    }
    cursor.value = events.cursor;
    write_json(app, "youtube-push-cursor.json", cursor).map_err(|_| ())
}
pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut cursor: Cursor = read_json(&app, "youtube-push-cursor.json");
        loop {
            let saved = load(&app);
            if saved.push_enabled {
                if let Some(token) = crate::secrets::read(CREDENTIAL) {
                    if cycle(&app, &saved, &token, &mut cursor).await.is_err() {
                        update(&app, false, "Нет связи с сервером; опрос раз в 15 минут");
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                } else {
                    update(&app, false, "Укажите токен сервера; опрос раз в 15 минут");
                }
            } else {
                update(&app, false, "Опрос раз в 15 минут");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_expired_and_inactive_leases_keep_polling() {
        let ids = HashSet::from(["one".to_string(), "two".to_string()]);
        let mut leases = vec![Lease {
            id: "one".into(),
            active: 1,
            lease: 1000.0,
        }];
        assert!(!leases_ready(&ids, &leases, 100.0));
        leases.push(Lease {
            id: "two".into(),
            active: 1,
            lease: 1000.0,
        });
        assert!(leases_ready(&ids, &leases, 100.0));
        leases[1].lease = 120.0;
        assert!(!leases_ready(&ids, &leases, 100.0));
        leases[1].lease = 1000.0;
        leases[1].active = 0;
        assert!(!leases_ready(&ids, &leases, 100.0));
        assert!(!leases_ready(&HashSet::new(), &[], 100.0));
    }
    #[test]
    fn only_https_origin_is_accepted() {
        assert_eq!(
            validate_url("https://push.example.com/").unwrap(),
            "https://push.example.com"
        );
        for value in [
            "http://push.example.com",
            "https://user:pass@push.example.com",
            "https://push.example.com/path",
            "https://push.example.com?token=x",
        ] {
            assert!(validate_url(value).is_err());
        }
    }
}
