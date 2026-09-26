//! Mail over IMAP with app passwords (Yandex, Gmail, Mail.ru, iCloud...).
//! Accounts live in `mail.json`, passwords in Credential Manager. One IMAP
//! connection per account is kept open and shared by the UI and a
//! background poller, which counts unread mail and raises a notification
//! for new messages.

mod client;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use self::client::{Account, Action, Letter, Summary};
use crate::secrets;

const FILE: &str = "mail.json";
const POLL_EVERY: Duration = Duration::from_secs(60);
/// More new messages than this at once become one "N new messages" toast.
const TOASTS_MAX: usize = 3;

#[derive(Serialize, Deserialize)]
struct Saved {
    accounts: Vec<Account>,
    notify: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Saved { accounts: Vec::new(), notify: true }
    }
}

fn secret_key(id: &str) -> String {
    format!("DockPanel/mail/{id}")
}

static APP: OnceLock<AppHandle> = OnceLock::new();
/// Unread UIDs per account from the last poll; `None` until the first one.
static UNSEEN: Mutex<Option<HashMap<String, (u32, HashSet<u32>)>>> = Mutex::new(None);

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(FILE))
}

fn load(app: &AppHandle) -> Saved {
    path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, saved: &Saved) -> Result<(), String> {
    let p = path(app)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn account(app: &AppHandle, id: &str) -> Result<Account, String> {
    load(app).accounts.into_iter().find(|a| a.id == id).ok_or_else(|| "Ящик не найден".into())
}

// ---- accounts ----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSettings {
    accounts: Vec<Account>,
    notify: bool,
}

#[tauri::command]
pub fn mail_settings(app: AppHandle) -> MailSettings {
    let saved = load(&app);
    MailSettings { accounts: saved.accounts, notify: saved.notify }
}

/// Checks the password by logging in, then remembers the account.
#[tauri::command]
pub async fn mail_add(app: AppHandle, email: String, password: String, host: Option<String>) -> Result<Account, String> {
    let email = email.trim().to_string();
    // App passwords are often copied with the spaces Google shows between groups.
    let password: String = password.chars().filter(|c| !c.is_whitespace()).collect();
    if !email.contains('@') || password.is_empty() {
        return Err("Введите адрес и пароль приложения".into());
    }
    let (host, port) = match host.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(h) => (h.to_string(), 993),
        None => client::known_server(&email)
            .map(|(h, p)| (h.to_string(), p))
            .ok_or("Для этого домена укажите IMAP-сервер")?,
    };
    let account = Account { id: email.to_lowercase(), email, host, port };

    let check = account.clone();
    let pw = password.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut session = client::connect(&check, &pw)?;
        let _ = session.logout();
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    secrets::write(&secret_key(&account.id), &password)?;
    let mut saved = load(&app);
    saved.accounts.retain(|a| a.id != account.id);
    saved.accounts.push(account.clone());
    save(&app, &saved)?;
    client::drop_session(&account.id);
    let _ = app.emit("mail:changed", ());
    Ok(account)
}

#[tauri::command]
pub fn mail_remove(app: AppHandle, id: String) -> Result<(), String> {
    let mut saved = load(&app);
    saved.accounts.retain(|a| a.id != id);
    save(&app, &saved)?;
    secrets::delete(&secret_key(&id));
    client::drop_session(&id);
    if let Some(map) = UNSEEN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(&id);
    }
    let _ = app.emit("mail:changed", ());
    Ok(())
}

#[tauri::command]
pub fn mail_set_notify(app: AppHandle, on: bool) -> Result<(), String> {
    let mut saved = load(&app);
    saved.notify = on;
    save(&app, &saved)
}

// ---- reading -----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailList {
    messages: Vec<Summary>,
    /// Accounts that failed, with the reason.
    errors: Vec<(String, String)>,
    /// Where each account's next (older) page starts; `None` once it has no more.
    cursors: HashMap<String, Option<u32>>,
    more: bool,
}

/// A page of letters, newest first, from every account (or one). `cursors`
/// comes from the previous page; without it the newest letters are listed.
/// `unread` lists only unread letters.
#[tauri::command]
pub async fn mail_list(
    app: AppHandle,
    account: Option<String>,
    cursors: Option<HashMap<String, Option<u32>>>,
    unread: bool,
) -> Result<MailList, String> {
    // Accounts to read, each with where to start; exhausted ones are skipped.
    let jobs: Vec<(Account, Option<u32>)> = load(&app)
        .accounts
        .into_iter()
        .filter(|a| account.as_ref().is_none_or(|id| &a.id == id))
        .filter_map(|a| match &cursors {
            None => Some((a, None)),
            Some(map) => map.get(&a.id).copied().flatten().map(|c| (a, Some(c))),
        })
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let results: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|(a, before)| (a, s.spawn(move || client::list_page(a, *before, unread))))
                .collect();
            handles.into_iter().map(|(a, h)| (a, h.join().unwrap_or_else(|_| Err("сбой".into())))).collect()
        });
        let mut list = MailList { messages: Vec::new(), errors: Vec::new(), cursors: HashMap::new(), more: false };
        for (a, r) in results {
            match r {
                Ok(page) => {
                    list.messages.extend(page.messages);
                    list.more |= page.next.is_some();
                    list.cursors.insert(a.id.clone(), page.next);
                }
                Err(e) => list.errors.push((a.email.clone(), e)),
            }
        }
        list.messages.sort_by(|a, b| b.date.cmp(&a.date));
        list
    })
    .await
    .map_err(|e| e.to_string())
}

// ---- actions -----------------------------------------------------------------------

// ---- unread count and notifications ------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unread {
    total: usize,
    by_account: HashMap<String, usize>,
}

#[tauri::command]
pub fn mail_unread() -> Unread {
    let map = UNSEEN.lock().unwrap_or_else(|e| e.into_inner());
    let by_account: HashMap<String, usize> =
        map.iter().flatten().map(|(id, (_, uids))| (id.clone(), uids.len())).collect();
    Unread { total: by_account.values().sum(), by_account }
}

fn poll(app: &AppHandle) {
    // The timer and a manual refresh can overlap; both would see the same
    // "previous" unread set and announce the same letters twice.
    static POLLING: Mutex<()> = Mutex::new(());
    let _guard = POLLING.lock().unwrap_or_else(|e| e.into_inner());
    let saved = load(app);
    let mut changed = false;
    let mut fresh: Vec<Summary> = Vec::new();

    for account in &saved.accounts {
        let Ok((validity, unseen)) = client::poll_account(account) else { continue };
        let previous = UNSEEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(&account.id).cloned();
        let new: Vec<u32> = match &previous {
            // The first poll after start only learns what is already there.
            None => Vec::new(),
            Some((v, _)) if *v != validity => Vec::new(),
            Some((_, old)) => unseen.difference(old).copied().collect(),
        };
        if previous.as_ref().is_none_or(|(_, old)| old != &unseen) {
            changed = true;
        }
        if saved.notify {
            fresh.extend(client::new_headers(account, &new));
        }
        UNSEEN
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(HashMap::new)
            .insert(account.id.clone(), (validity, unseen));
    }

    if changed {
        let _ = app.emit("mail:changed", ());
    }
    fresh.sort_by(|a, b| b.date.cmp(&a.date));
    if fresh.len() > TOASTS_MAX {
        let senders: Vec<&str> = fresh.iter().take(3).map(|m| m.from_name.as_str()).collect();
        notify(app, &format!("Новых писем: {}", fresh.len()), &format!("От {}…", senders.join(", ")), None);
    } else {
        for m in fresh {
            notify(app, &m.from_name.clone(), &m.subject.clone(), Some(m));
        }
    }
}

/// Unread letters across all accounts, as of the last poll.
pub fn unread_total() -> usize {
    UNSEEN.lock().unwrap_or_else(|e| e.into_inner()).iter().flatten().map(|(_, (_, uids))| uids.len()).sum()
}

#[cfg(windows)]
/// Clicking the toast opens the mail tab, and the letter itself when there is one.
fn notify(app: &AppHandle, title: &str, body: &str, letter: Option<Summary>) {
    let handle = app.clone();
    let open = move || {
        crate::panel::show_tab(&handle, "mail");
        if let Some(letter) = &letter {
            let _ = handle.emit("mail:open", letter);
        }
    };
    crate::alerts::notify_then(app, title, body, Some(Box::new(open)));
}

#[cfg(not(windows))]
fn notify(_app: &AppHandle, _title: &str, _body: &str, _letter: Option<Summary>) {}

/// Starts the background poller.
pub fn init(app: &AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    client::set_password_source(|id| secrets::read(&secret_key(id)));
    let app = app.clone();
    std::thread::spawn(move || loop {
        poll(&app);
        std::thread::sleep(POLL_EVERY);
    });
}

/// Polls right away, e.g. when the panel opens.
#[tauri::command]
pub async fn mail_refresh(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || poll(&app)).await.map_err(|e| e.to_string())
}


/// Opens a letter and marks it read.
#[tauri::command]
pub async fn mail_open(app: AppHandle, account: String, uid: u32) -> Result<Letter, String> {
    let account = self::account(&app, &account)?;
    tauri::async_runtime::spawn_blocking(move || client::open_letter(&account, uid)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mail_action(app: AppHandle, account: String, uid: u32, action: Action) -> Result<(), String> {
    let account = self::account(&app, &account)?;
    tauri::async_runtime::spawn_blocking(move || client::act(&account, uid, action)).await.map_err(|e| e.to_string())??;
    let _ = app.emit("mail:changed", ());
    Ok(())
}
