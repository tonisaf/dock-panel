//! IMAP side of the mail tab, free of Tauri so it can be tested against a
//! real server: connection per account (kept open, reconnected when the
//! server drops it), listing, reading and the actions.

use std::collections::{HashMap, HashSet};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use imap_proto::NameAttribute;
use mail_parser::{Address, MessageParser, MimeHeaders};
use native_tls::{TlsConnector, TlsStream};
use serde::{Deserialize, Serialize};

const INBOX: &str = "INBOX";
const LIST_LIMIT: u32 = 40;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Bodies are cut here; enough for any real letter, not for big attachments.
const BODY_LIMIT: u32 = 3_000_000;
const TEXT_LIMIT: usize = 100_000;

pub type Session = imap::Session<TlsStream<TcpStream>>;

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    /// The address, lowercased.
    pub id: String,
    pub email: String,
    pub host: String,
    pub port: u16,
}

/// Where passwords come from (Credential Manager in the app).
static PASSWORDS: OnceLock<fn(&str) -> Option<String>> = OnceLock::new();
static SESSIONS: Mutex<Option<HashMap<String, Arc<Mutex<Option<Session>>>>>> = Mutex::new(None);

pub fn set_password_source(source: fn(&str) -> Option<String>) {
    let _ = PASSWORDS.set(source);
}

/// IMAP server for well-known domains.
pub fn known_server(email: &str) -> Option<(&'static str, u16)> {
    let domain = email.rsplit('@').next()?.to_lowercase();
    Some(match domain.as_str() {
        "gmail.com" | "googlemail.com" => ("imap.gmail.com", 993),
        "yandex.ru" | "yandex.com" | "ya.ru" | "yandex.by" | "yandex.kz" | "yandex.ua"
        | "narod.ru" => ("imap.yandex.ru", 993),
        "mail.ru" | "bk.ru" | "inbox.ru" | "list.ru" | "internet.ru" => ("imap.mail.ru", 993),
        "icloud.com" | "me.com" | "mac.com" => ("imap.mail.me.com", 993),
        _ => return None,
    })
}

fn friendly(e: imap::Error) -> String {
    match e {
        imap::Error::No(imap::error::No {
            information: msg, ..
        })
        | imap::Error::Bad(imap::error::Bad {
            information: msg, ..
        }) => {
            let lower = msg.to_lowercase();
            if lower.contains("auth")
                || lower.contains("credentials")
                || lower.contains("password")
                || lower.contains("login")
            {
                "Почта не приняла пароль. Нужен пароль приложения, а в Яндексе ещё и включённый доступ по IMAP.".into()
            } else {
                format!("Сервер почты: {msg}")
            }
        }
        imap::Error::Io(e) => format!("Нет связи с сервером почты: {e}"),
        imap::Error::TlsHandshake(e) => format!("Не удалось установить защищённое соединение: {e}"),
        e => e.to_string(),
    }
}

pub fn connect(account: &Account, password: &str) -> Result<Session, String> {
    connect_watcher(account, password).map(|(session, _)| session)
}

pub fn connect_watcher(account: &Account, password: &str) -> Result<(Session, TcpStream), String> {
    let addr = (account.host.as_str(), account.port)
        .to_socket_addrs()
        .map_err(|e| format!("Сервер {} не найден: {e}", account.host))?
        .next()
        .ok_or_else(|| format!("Сервер {} не найден", account.host))?;
    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("Нет связи с сервером почты: {e}"))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let interrupt = tcp.try_clone().map_err(|e| e.to_string())?;
    let tls = TlsConnector::new()
        .map_err(|e| e.to_string())?
        .connect(&account.host, tcp)
        .map_err(|e| format!("Не удалось установить защищённое соединение: {e}"))?;
    let mut client = imap::Client::new(tls);
    client.read_greeting().map_err(friendly)?;
    client
        .login(&account.email, password)
        .map(|session| (session, interrupt))
        .map_err(|(e, _)| friendly(e))
}

/// Runs `f` on the account's open connection, connecting (or reconnecting
/// once, if the old connection died) as needed.
pub fn with_session<R>(
    account: &Account,
    mut f: impl FnMut(&mut Session) -> imap::Result<R>,
) -> Result<R, String> {
    let slot = SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .entry(account.id.clone())
        .or_default()
        .clone();
    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());

    for attempt in 0..2 {
        if slot.is_none() {
            let password = PASSWORDS
                .get()
                .and_then(|get| get(&account.id))
                .ok_or("Пароль не найден: добавьте ящик заново")?;
            *slot = Some(connect(account, &password)?);
        }
        match f(slot.as_mut().expect("connected above")) {
            Ok(r) => return Ok(r),
            // A connection the server has closed shows up as an I/O or parse error.
            Err(imap::Error::Io(_) | imap::Error::ConnectionLost | imap::Error::Parse(_))
                if attempt == 0 =>
            {
                *slot = None
            }
            Err(e) => return Err(friendly(e)),
        }
    }
    Err("Нет связи с сервером почты".into())
}

pub fn drop_session(id: &str) {
    if let Some(map) = SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(id);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub account: String,
    pub uid: u32,
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    /// Unix ms.
    pub date: i64,
    pub unread: bool,
    pub flagged: bool,
}

pub fn first_addr(a: Option<&Address>) -> (String, String) {
    let Some(addr) = a.and_then(|a| a.first()) else {
        return (String::new(), String::new());
    };
    let email = addr.address().unwrap_or_default().to_string();
    let name = addr
        .name()
        .map(str::to_string)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| email.clone());
    (name, email)
}

pub fn summarize(account: &str, fetch: &imap::types::Fetch) -> Option<Summary> {
    let header = MessageParser::default().parse_headers(fetch.header()?)?;
    let (from_name, from_email) = first_addr(header.from());
    let date = header
        .date()
        .map(|d| d.to_timestamp() * 1000)
        .or_else(|| fetch.internal_date().map(|d| d.timestamp_millis()))
        .unwrap_or(0);
    let flags = fetch.flags();
    Some(Summary {
        account: account.to_string(),
        uid: fetch.uid?,
        from_name,
        from_email,
        subject: header.subject().unwrap_or("(без темы)").to_string(),
        date,
        unread: !flags.contains(&imap::types::Flag::Seen),
        flagged: flags.contains(&imap::types::Flag::Flagged),
    })
}

/// The newest messages of the inbox.
/// One page of the inbox, newest first, and the cursor for the next (older)
/// page, if there is one. The cursor is opaque to the UI: a sequence number
/// for the whole inbox, a UID for unread-only pages.
pub struct Page {
    pub messages: Vec<Summary>,
    pub next: Option<u32>,
}

const FETCH_HEADERS: &str = "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER])";

pub fn list_page(
    account: &Account,
    before: Option<u32>,
    unread_only: bool,
) -> Result<Page, String> {
    with_session(account, |s| {
        let exists = s.select(INBOX)?.exists;
        if unread_only {
            let mut uids: Vec<u32> = s
                .uid_search("UNSEEN")?
                .into_iter()
                .filter(|u| before.is_none_or(|b| *u < b))
                .collect();
            uids.sort_unstable_by(|a, b| b.cmp(a));
            let page: Vec<u32> = uids.iter().take(LIST_LIMIT as usize).copied().collect();
            if page.is_empty() {
                return Ok(Page {
                    messages: Vec::new(),
                    next: None,
                });
            }
            let set = page
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let fetches = s.uid_fetch(set, FETCH_HEADERS)?;
            let next = (uids.len() > page.len()).then(|| *page.last().expect("page is not empty"));
            return Ok(Page {
                messages: fetches
                    .iter()
                    .filter_map(|f| summarize(&account.id, f))
                    .collect(),
                next,
            });
        }
        // Sequence numbers: 1 is the oldest letter, `exists` the newest.
        let top = before.map_or(exists, |b| b.saturating_sub(1).min(exists));
        if top == 0 {
            return Ok(Page {
                messages: Vec::new(),
                next: None,
            });
        }
        let from = top.saturating_sub(LIST_LIMIT - 1).max(1);
        let fetches = s.fetch(format!("{from}:{top}"), FETCH_HEADERS)?;
        let messages = fetches
            .iter()
            .filter_map(|f| summarize(&account.id, f))
            .collect();
        Ok(Page {
            messages,
            next: (from > 1).then_some(from),
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Letter {
    #[serde(flatten)]
    pub summary: Summary,
    pub to: String,
    /// Plain text; HTML-only letters are converted.
    pub text: String,
    pub attachments: Vec<String>,
    /// Where to open the letter in the browser.
    pub web_url: String,
}

pub fn web_url(account: &Account, message_id: Option<&str>) -> String {
    match (account.host.as_str(), message_id) {
        ("imap.gmail.com", Some(id)) => {
            let q: String = percent_encoding::utf8_percent_encode(
                &format!("rfc822msgid:{id}"),
                percent_encoding::NON_ALPHANUMERIC,
            )
            .to_string();
            format!(
                "https://mail.google.com/mail/u/{}/#search/{q}",
                account.email
            )
        }
        ("imap.gmail.com", None) => format!("https://mail.google.com/mail/u/{}/", account.email),
        ("imap.yandex.ru", _) => "https://mail.yandex.ru/".into(),
        ("imap.mail.ru", _) => "https://e.mail.ru/inbox/".into(),
        ("imap.mail.me.com", _) => "https://www.icloud.com/mail/".into(),
        (host, _) => format!("https://{}", host.trim_start_matches("imap.")),
    }
}

/// A folder by its special-use flag (RFC 6154), e.g. `\Trash`.
pub fn special_folder(
    s: &mut Session,
    flags: &[NameAttribute<'static>],
) -> imap::Result<Option<String>> {
    let names = s.list(Some(""), Some("*"))?;
    Ok(names
        .iter()
        .find(|n| n.attributes().iter().any(|a| flags.contains(a)))
        .map(|n| n.name().to_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Read,
    Unread,
    Archive,
    Delete,
}

/// Tags that end a block of text; mail-parser only breaks lines at `<br>` and `</p>`.
const BLOCK_TAGS: [&str; 14] = [
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "tr",
    "table",
    "blockquote",
    "ul",
    "ol",
    "section",
];
/// Longer links are trackers; the text around them is enough.
const LINK_MAX: usize = 100;

fn href(tag: &str) -> Option<&str> {
    let at = tag.to_ascii_lowercase().find("href=")? + 5;
    let rest = &tag[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &rest[1..rest[1..].find(quote)? + 1];
    (value.starts_with("http") && value.len() <= LINK_MAX).then_some(value)
}

/// HTML to readable text: line breaks after blocks, short links kept as "text (url)".
pub fn html_to_text(html: &str) -> String {
    // ASCII lowercasing keeps byte offsets, so positions found in `lower` index `html`.
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 256);
    let (mut i, mut link) = (0, None::<&str>);
    while let Some(off) = lower[i..].find('<') {
        let start = i + off;
        let end = lower[start..]
            .find('>')
            .map_or(html.len(), |e| start + e + 1);
        out.push_str(&html[i..start]);
        let tag = &lower[start..end];
        let closing = tag.starts_with("</");
        let name: String = tag
            .trim_start_matches(['<', '/'])
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        out.push_str(&html[start..end]);
        if name == "a" {
            if closing {
                if let Some(url) = link.take() {
                    out.push_str(&format!(" ({url})"));
                }
            } else {
                link = href(&html[start..end]);
            }
        } else if closing && BLOCK_TAGS.contains(&name.as_str()) || name == "hr" {
            out.push_str("<br>");
        }
        i = end;
    }
    out.push_str(&html[i.min(html.len())..]);
    tidy(&mail_parser::decoders::html::html_to_text(&out))
}

/// Unix line ends, no trailing spaces, at most one empty line in a row.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.replace("\r\n", "\n").lines() {
        let line = line.trim_end();
        blank = if line.is_empty() { blank + 1 } else { 0 };
        if blank <= 1 {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// A letter's text and details; marks it read.
pub fn open_letter(account: &Account, uid: u32) -> Result<Letter, String> {
    let letter = with_session(account, |s| {
        s.select(INBOX)?;
        let fetches = s.uid_fetch(
            uid.to_string(),
            format!("(UID FLAGS INTERNALDATE BODY.PEEK[]<0.{BODY_LIMIT}>)"),
        )?;
        let Some(fetch) = fetches.iter().next() else {
            return Ok(None);
        };
        let Some(raw) = fetch.body() else {
            return Ok(None);
        };
        let Some(msg) = MessageParser::default().parse(raw) else {
            return Ok(None);
        };
        let (from_name, from_email) = first_addr(msg.from());
        let to = msg
            .to()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.address())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        // HTML-only letters go through our converter, which keeps paragraphs and links.
        let first_text = msg.text_body.first().and_then(|&id| msg.part(id));
        let mut text = match first_text {
            Some(part) if matches!(part.body, mail_parser::PartType::Html(_)) => {
                html_to_text(part.text_contents().unwrap_or_default())
            }
            _ => tidy(&msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default()),
        };
        if text.len() > TEXT_LIMIT {
            let cut = (0..=TEXT_LIMIT)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0);
            text.truncate(cut);
            text.push_str("\n\n…");
        }
        let flags = fetch.flags();
        let summary = Summary {
            account: account.id.clone(),
            uid,
            from_name,
            from_email,
            subject: msg.subject().unwrap_or("(без темы)").to_string(),
            date: msg.date().map(|d| d.to_timestamp() * 1000).unwrap_or(0),
            unread: false,
            flagged: flags.contains(&imap::types::Flag::Flagged),
        };
        let letter = Letter {
            summary,
            to,
            text,
            attachments: msg
                .attachments()
                .filter_map(|p| p.attachment_name().map(str::to_string))
                .collect(),
            web_url: web_url(account, msg.message_id()),
        };
        if !flags.contains(&imap::types::Flag::Seen) {
            s.uid_store(uid.to_string(), "+FLAGS.SILENT (\\Seen)")?;
        }
        Ok(Some(letter))
    })?;
    letter.ok_or_else(|| "Письмо не найдено: возможно, его уже удалили".into())
}

pub fn act(account: &Account, uid: u32, action: Action) -> Result<(), String> {
    with_session(account, |s| {
        s.select(INBOX)?;
        let uid = uid.to_string();
        match action {
            Action::Read => drop(s.uid_store(&uid, "+FLAGS.SILENT (\\Seen)")?),
            Action::Unread => drop(s.uid_store(&uid, "-FLAGS.SILENT (\\Seen)")?),
            Action::Archive => {
                // Gmail: "All Mail" (leaving the inbox is archiving); others: their archive folder.
                let folder = match special_folder(s, &[NameAttribute::All, NameAttribute::Archive])?
                {
                    Some(f) => f,
                    None => {
                        let _ = s.create("Archive");
                        "Archive".to_string()
                    }
                };
                s.uid_mv(&uid, &folder)?;
            }
            Action::Delete => match special_folder(s, &[NameAttribute::Trash])? {
                Some(trash) => s.uid_mv(&uid, &trash)?,
                None => {
                    s.uid_store(&uid, "+FLAGS.SILENT (\\Deleted)")?;
                    s.uid_expunge(&uid)?;
                }
            },
        }
        Ok(())
    })
}

/// One poll of one account: unread UIDs, and headers of those not seen before.
pub fn poll_account(account: &Account) -> Result<(u32, HashSet<u32>), String> {
    with_session(account, |s| {
        let mailbox = s.examine(INBOX)?;
        let unseen = s.uid_search("UNSEEN")?;
        Ok((mailbox.uid_validity.unwrap_or(0), unseen))
    })
}

pub fn new_headers(account: &Account, uids: &[u32]) -> Vec<Summary> {
    if uids.is_empty() {
        return Vec::new();
    }
    let set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    with_session(account, |s| {
        let fetches = s.uid_fetch(&set, "(UID FLAGS BODY.PEEK[HEADER])")?;
        Ok(fetches
            .iter()
            .filter_map(|f| summarize(&account.id, f))
            .collect())
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_keeps_blocks_and_short_links() {
        let html = "<h1>Big sale</h1><p>Up to <b>50%</b> off. <a href=\"https://shop.example\">Shop</a></p>\
            <ul><li>One</li><li>Two</li></ul><script>alert(1)</script><a href='https://t.example/?x=\
            aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'>Track</a>";
        assert_eq!(
            html_to_text(html),
            "Big sale\nUp to 50% off. Shop (https://shop.example)\nOne\nTwo\n\nTrack"
        );
    }

    #[test]
    fn tidy_collapses_blank_lines() {
        assert_eq!(tidy("a  \r\n\r\n\r\n\r\nb\r\n"), "a\n\nb");
    }

    #[test]
    fn servers_for_known_domains() {
        assert_eq!(known_server("me@yandex.ru"), Some(("imap.yandex.ru", 993)));
        assert_eq!(known_server("Me@Gmail.com"), Some(("imap.gmail.com", 993)));
        assert_eq!(known_server("me@example.org"), None);
    }
}
