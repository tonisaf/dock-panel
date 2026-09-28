//! Notes from a Notion database, kept on disk so they open (and are found by
//! search) without Notion. The UI only ever reads the cache; a background
//! sync refreshes it: the database is listed newest-edited first and only
//! pages whose `last_edited_time` moved are fetched again, images included
//! (Notion's file links expire within the hour).
//!
//! Changes made in the panel (a to-do ticked, a quick note, a note's title,
//! tags, pin or text) apply to the cache at once and wait in an outbox until
//! Notion takes them. A text edit goes out as the difference only (see
//! `edit`), step by step, and resumes where it stopped if the connection drops.
//!
//! Layout under `<app data>/notes/`: `index.json` (notes and their text),
//! `pages/<id>.json` (blocks), `images/`, `outbox.json`, `settings.json`.

mod blocks;
mod edit;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::http::{header, Request, Response};
use tauri::{AppHandle, Emitter, Manager};

pub use blocks::Block;

use crate::{net, notion};

const EVENT: &str = "notes:changed";
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);
/// A sync asked for sooner than this after the last one is skipped (the UI asks on every open).
const FRESH_MS: u64 = 60_000;
/// Notion's database query lags behind a page just created there: for this
/// long a sync keeps such a note although the listing doesn't have it yet.
const JUST_CREATED_MS: u64 = 10 * 60_000;
const MAX_NOTES: usize = 500;
const MAX_DEPTH: usize = 4;
/// Notion allows about three requests a second.
const PACE: Duration = Duration::from_millis(340);
const PREVIEW_CHARS: usize = 160;

// ---- stored shapes -----------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    id: String,
    title: String,
    database_id: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    source: Option<Source>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tag {
    name: String,
    color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    id: String,
    url: String,
    title: String,
    icon: Option<String>,
    tags: Vec<Tag>,
    pinned: bool,
    /// ISO timestamps from Notion.
    edited: String,
    created: String,
    preview: String,
    /// Made in the panel and not in Notion yet.
    #[serde(default)]
    local: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    #[serde(flatten)]
    note: Note,
    /// All of the note's text, for search.
    #[serde(default)]
    text: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Index {
    source_id: String,
    synced_at: Option<u64>,
    entries: Vec<Entry>,
    /// From the database's schema, for the editors.
    #[serde(default)]
    tag_options: Vec<Tag>,
    #[serde(default)]
    can_tag: bool,
    #[serde(default)]
    can_pin: bool,
}

#[derive(Serialize, Deserialize)]
struct Page {
    edited: String,
    blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Op {
    #[serde(rename_all = "camelCase")]
    Toggle { page_id: String, block_id: String, checked: bool },
    #[serde(rename_all = "camelCase")]
    Create { local_id: String, title: String, body: String },
    #[serde(rename_all = "camelCase")]
    Props { page_id: String, title: Option<String>, tags: Option<Vec<String>>, pinned: Option<bool> },
    /// The note's text went from `before` to `after`; `done` steps of the plan
    /// are already in Notion, `ids` has the blocks created so far.
    #[serde(rename_all = "camelCase")]
    Edit {
        page_id: String,
        before: Vec<edit::Line>,
        after: Vec<edit::Line>,
        #[serde(default)]
        done: usize,
        #[serde(default)]
        ids: Vec<Option<String>>,
    },
}

/// Local ids: notes and blocks the panel made that Notion hasn't yet.
fn is_local(id: &str) -> bool {
    id.starts_with("local-")
}

struct Store {
    dir: PathBuf,
    settings: Settings,
    index: Index,
    outbox: Vec<Op>,
    syncing: bool,
    error: Option<String>,
    /// Notes the panel made that are now in Notion: local id -> (Notion id,
    /// when). A note still open under its local id finds its page through it,
    /// and a sync keeps it while Notion's listing lags (JUST_CREATED_MS).
    created: HashMap<String, (String, u64)>,
    /// Notion turned down a new note (for good, not for lack of connection);
    /// the note stays local and the sync reports this.
    create_error: Option<String>,
}

impl Store {
    /// The Notion id of a note made here, or the id as it is.
    fn resolve(&self, id: &str) -> String {
        self.created.get(id).map_or_else(|| id.to_string(), |(real, _)| real.clone())
    }

    fn just_created(&self) -> HashSet<String> {
        let now = now_ms();
        self.created.values().filter(|(_, at)| now - at < JUST_CREATED_MS).map(|(id, _)| id.clone()).collect()
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static STORE: Mutex<Option<Store>> = Mutex::new(None);
/// One sync at a time.
static SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    STORE.lock().unwrap_or_else(|e| e.into_inner()).as_mut().map(f)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_json(path: &Path, value: &impl Serialize) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(value) {
        // Written aside and renamed, so a crash mid-write keeps the old file.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

impl Store {
    fn page_path(&self, id: &str) -> PathBuf {
        self.dir.join("pages").join(format!("{}.json", safe_name(id)))
    }
    fn images(&self) -> PathBuf {
        self.dir.join("images")
    }
    fn save_index(&self) {
        write_json(&self.dir.join("index.json"), &self.index);
    }
    fn save_outbox(&self) {
        write_json(&self.dir.join("outbox.json"), &self.outbox);
    }
}

/// Notion ids and our file names are hex and dashes; anything else is dropped.
fn safe_name(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.').collect()
}

fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ());
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Unix ms as Notion's ISO form ("2026-09-28T10:04:05.000Z").
fn iso(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", t / 3600, t / 60 % 60, t % 60, ms % 1000)
}

// ---- start-up ----------------------------------------------------------------------

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("notes")) else { return };
    let settings: Settings = read_json(&dir.join("settings.json")).unwrap_or_default();
    let mut index: Index = read_json(&dir.join("index.json")).unwrap_or_default();
    if settings.source.as_ref().map(|s| s.id.as_str()) != Some(index.source_id.as_str()) {
        index = Index::default();
    }
    let outbox: Vec<Op> = read_json(&dir.join("outbox.json")).unwrap_or_default();
    *STORE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Store {
        dir,
        settings,
        index,
        outbox,
        syncing: false,
        error: None,
        created: HashMap::new(),
        create_error: None,
    });

    tauri::async_runtime::spawn(async {
        loop {
            let _ = sync(false).await;
            tokio::time::sleep(SYNC_EVERY).await;
        }
    });
}

// ---- Notion ------------------------------------------------------------------------

async fn api(token: &str, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    tokio::time::sleep(PACE).await;
    notion::call(token, method, path, body).await
}

struct Schema {
    title: String,
    tags: Option<(String, &'static str)>,
    tag_options: Vec<Tag>,
    pinned: Option<String>,
}

fn detect(ds: &Value) -> Result<Schema, String> {
    let props = ds["properties"].as_object().ok_or("У базы нет свойств")?;
    let named = |t: &str, hints: &[&str]| {
        props
            .iter()
            .filter(|(_, p)| p["type"] == t)
            .find(|(n, _)| hints.is_empty() || hints.iter().any(|h| n.to_lowercase().contains(h)))
            .map(|(n, _)| n.clone())
    };
    let title = named("title", &[]).ok_or("В базе нет поля названия")?;
    let tags = named("multi_select", &[])
        .map(|n| (n, "multi_select"))
        .or_else(|| named("select", &[]).map(|n| (n, "select")));
    let pinned = named("checkbox", &["pin", "закреп", "избран", "favorite", "star", "важн"]);
    let tag_options = tags
        .as_ref()
        .and_then(|(n, t)| props[n][*t]["options"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|o| Some(Tag { name: o["name"].as_str()?.to_string(), color: o["color"].as_str().unwrap_or("default").to_string() }))
        .collect();
    Ok(Schema { title, tags, tag_options, pinned })
}

fn to_note(page: &Value, schema: &Schema) -> Option<Note> {
    let props = &page["properties"];
    let tags = match &schema.tags {
        Some((name, "multi_select")) => props[name]["multi_select"].as_array().cloned().unwrap_or_default(),
        Some((name, _)) => props[name]["select"].as_object().map(|_| vec![props[name]["select"].clone()]).unwrap_or_default(),
        None => Vec::new(),
    };
    Some(Note {
        id: page["id"].as_str()?.to_string(),
        url: page["url"].as_str().unwrap_or_default().to_string(),
        title: notion::plain_text(&props[&schema.title]["title"]),
        icon: page["icon"]["emoji"].as_str().map(str::to_string),
        tags: tags
            .iter()
            .filter_map(|t| Some(Tag { name: t["name"].as_str()?.to_string(), color: t["color"].as_str().unwrap_or("default").to_string() }))
            .collect(),
        pinned: schema.pinned.as_ref().is_some_and(|p| props[p]["checkbox"].as_bool() == Some(true)),
        edited: page["last_edited_time"].as_str().unwrap_or_default().to_string(),
        created: page["created_time"].as_str().unwrap_or_default().to_string(),
        preview: String::new(),
        local: false,
    })
}

/// A block's children, all pages of them, and theirs down to `MAX_DEPTH`.
async fn children(token: &str, id: &str, depth: usize) -> Result<Vec<Block>, String> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut path = format!("/blocks/{id}/children?page_size=100");
        if let Some(c) = &cursor {
            path.push_str(&format!("&start_cursor={c}"));
        }
        let res = api(token, Method::GET, &path, None).await?;
        out.extend(res["results"].as_array().into_iter().flatten().filter_map(blocks::parse));
        match res["next_cursor"].as_str() {
            Some(c) if res["has_more"].as_bool() == Some(true) => cursor = Some(c.to_string()),
            _ => break,
        }
    }
    if depth < MAX_DEPTH {
        for b in out.iter_mut().filter(|b| b.has_children) {
            b.children = Box::pin(children(token, &b.id, depth + 1)).await?;
        }
    }
    Ok(out)
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// The cache file an image URL maps to: its path without the (expiring) query.
fn image_name(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let ext = path
        .rsplit('.')
        .next()
        .map(str::to_lowercase)
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"))
        .unwrap_or_else(|| "img".into());
    format!("{:016x}.{ext}", fnv1a(path))
}

/// Downloads the page's images that aren't on disk yet; a failed one keeps its remote link.
async fn cache_images(blocks: &mut [Block], dir: &Path) {
    let mut wanted = Vec::new();
    blocks::each_mut(blocks, &mut |b| {
        if b.kind == "image" {
            if let Some(url) = &b.url {
                wanted.push((b.id.clone(), url.clone(), image_name(url)));
            }
        }
    });
    let mut have = HashSet::new();
    for (_, url, name) in wanted {
        let path = dir.join(&name);
        if path.exists() {
            have.insert(name);
            continue;
        }
        let Ok(res) = net::client().get(&url).send().await else { continue };
        if !res.status().is_success() {
            continue;
        }
        if let Ok(bytes) = res.bytes().await {
            let _ = std::fs::create_dir_all(dir);
            if std::fs::write(&path, &bytes).is_ok() {
                have.insert(name);
            }
        }
    }
    blocks::each_mut(blocks, &mut |b| {
        if let Some(name) = b.url.as_deref().map(image_name).filter(|n| have.contains(n)) {
            b.file = Some(name);
        }
    });
}

fn preview_of(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(PREVIEW_CHARS) {
        Some((i, _)) => format!("{}…", &flat[..i]),
        None => flat,
    }
}

/// Puts to-dos ticked in the panel (still in the outbox) back onto blocks just read from Notion.
fn apply_pending(page_id: &str, blocks: &mut [Block], outbox: &[Op]) {
    for op in outbox {
        if let Op::Toggle { page_id: p, block_id, checked } = op {
            if p == page_id {
                blocks::each_mut(blocks, &mut |b| {
                    if &b.id == block_id {
                        b.checked = Some(*checked);
                    }
                });
            }
        }
    }
}

// ---- sync --------------------------------------------------------------------------

/// Sends the outbox; stops at the first failure that looks like no connection.
async fn flush(token: &str, source_id: &str) -> Result<(), String> {
    loop {
        let Some(op) = with_store(|s| s.outbox.first().cloned()).flatten() else { return Ok(()) };
        // A note created just now, with its blocks as Notion has them.
        let mut created: Option<(Note, Option<Vec<Block>>)> = None;
        let result = match &op {
            Op::Toggle { block_id, checked, .. } => {
                api(token, Method::PATCH, &format!("/blocks/{block_id}"), Some(json!({ "to_do": { "checked": checked } }))).await
            }
            Op::Create { title, body, .. } => {
                let ds = api(token, Method::GET, &format!("/data_sources/{source_id}"), None).await?;
                let schema = detect(&ds)?;
                let body = json!({
                    "parent": { "type": "data_source_id", "data_source_id": source_id },
                    "properties": { schema.title.clone(): { "title": [{ "text": { "content": title } }] } },
                    "children": blocks::from_text(body),
                });
                let page = api(token, Method::POST, "/pages", Some(body)).await;
                if let Some(note) = page.as_ref().ok().and_then(|p| to_note(p, &schema)) {
                    // Real block ids, so edits right after go to the right blocks.
                    let mut blocks = children(token, &note.id, 0).await.ok();
                    if let (Some(b), Some(images)) = (blocks.as_mut(), with_store(|s| s.images())) {
                        cache_images(b, &images).await;
                    }
                    created = Some((note, blocks));
                }
                page
            }
            Op::Props { page_id, title, tags, pinned } => {
                let ds = api(token, Method::GET, &format!("/data_sources/{source_id}"), None).await?;
                let schema = detect(&ds)?;
                let mut props = serde_json::Map::new();
                if let Some(t) = title {
                    props.insert(schema.title.clone(), json!({ "title": [{ "text": { "content": t } }] }));
                }
                if let (Some(tags), Some((name, kind))) = (tags, &schema.tags) {
                    let value = if *kind == "multi_select" {
                        json!({ "multi_select": tags.iter().map(|t| json!({ "name": t })).collect::<Vec<_>>() })
                    } else {
                        json!({ "select": tags.first().map(|t| json!({ "name": t })) })
                    };
                    props.insert(name.clone(), value);
                }
                if let (Some(p), Some(name)) = (pinned, &schema.pinned) {
                    props.insert(name.clone(), json!({ "checkbox": p }));
                }
                api(token, Method::PATCH, &format!("/pages/{page_id}"), Some(json!({ "properties": props }))).await
            }
            Op::Edit { .. } => apply_edit(token, &op).await,
        };
        let failed = match result {
            Ok(_) => None,
            // Offline: keep it for later.
            Err(e) if e.starts_with("Нет связи") || e.contains("подождать") => return Err(e),
            // Anything else (a deleted block, say) won't get better by retrying.
            Err(e) => {
                eprintln!("notes: dropping {op:?}: {e}");
                Some(e)
            }
        };
        with_store(|s| {
            // An edit's saved progress moves on while it is sent; it is still the same edit.
            let same = |f: &Op| match (f, &op) {
                (Op::Edit { page_id: a, before: x, .. }, Op::Edit { page_id: b, before: y, .. }) => a == b && x == y,
                (f, op) => f == op,
            };
            if s.outbox.first().is_some_and(same) {
                s.outbox.remove(0);
            }
            if let Op::Create { local_id, .. } = &op {
                match (created.take(), &failed) {
                    // The local note becomes the Notion page in place: the
                    // listing may not show the page for a while yet.
                    (Some((mut note, blocks)), _) => {
                        let at = s.index.entries.iter().position(|e| &e.note.id == local_id);
                        let text = at.map(|i| s.index.entries[i].text.clone()).unwrap_or_default();
                        note.preview = preview_of(&text);
                        let entry = Entry { note: note.clone(), text };
                        match at {
                            Some(i) => s.index.entries[i] = entry,
                            None => s.index.entries.insert(0, entry),
                        }
                        let _ = std::fs::remove_file(s.page_path(local_id));
                        // Without blocks, the page is fetched when opened.
                        if let Some(blocks) = blocks {
                            write_json(&s.page_path(&note.id), &Page { edited: note.edited.clone(), blocks });
                        }
                        s.created.insert(local_id.clone(), (note.id, now_ms()));
                    }
                    // Turned down: keep the note here, and say so.
                    (None, Some(e)) => s.create_error = Some(format!("Notion не принял новую заметку: {e}")),
                    // Created, but the answer wasn't a page we can read: the next sync brings it.
                    (None, None) => {
                        s.index.entries.retain(|e| &e.note.id != local_id);
                        let _ = std::fs::remove_file(s.page_path(local_id));
                    }
                }
            }
            s.save_outbox();
            s.save_index();
        });
    }
}

/// Sends a text edit step by step, saving progress after each, so a dropped
/// connection resumes where it stopped instead of inserting lines twice.
async fn apply_edit(token: &str, op: &Op) -> Result<Value, String> {
    let Op::Edit { page_id, before, after, done, ids } = op else { return Ok(Value::Null) };
    let steps = edit::plan(before, after);
    let mut ids: Vec<Option<String>> = if ids.len() == after.len() { ids.clone() } else { after.iter().map(|l| l.id.clone()).collect() };
    let id_of = |ids: &[Option<String>], i: usize| ids[i].clone().filter(|id| !is_local(id));
    for (n, step) in steps.iter().enumerate().skip(*done) {
        let result = match step {
            edit::Step::Update { id, body } => api(token, Method::PATCH, &format!("/blocks/{id}"), Some(body.clone())).await,
            edit::Step::Delete { id } => api(token, Method::DELETE, &format!("/blocks/{id}"), None).await,
            edit::Step::Insert { parent, after_line, lines } => {
                let parent_id = match parent {
                    Some(p) => id_of(&ids, *p).ok_or("Родительский блок ещё не создан")?,
                    None => page_id.clone(),
                };
                let after_id = after_line.and_then(|a| id_of(&ids, a));
                let position = match &after_id {
                    Some(id) => json!({ "type": "after_block", "after_block": { "id": id } }),
                    None => json!({ "type": "start" }),
                };
                let children: Vec<Value> = lines.iter().map(|&i| edit::block_body(&after[i])).collect();
                let path = format!("/blocks/{parent_id}/children");
                let mut res = api(token, Method::PATCH, &path, Some(json!({ "children": children, "position": position }))).await;
                // New lines must not get lost to a request shape Notion doesn't take: the older
                // `after` parameter next, and at worst the end of the parent.
                if let Err(e) = &res {
                    if !(e.starts_with("Нет связи") || e.contains("подождать")) {
                        eprintln!("notes: insert with position: {e}");
                        let body = match &after_id {
                            Some(id) => json!({ "children": children, "after": id }),
                            None => json!({ "children": children }),
                        };
                        res = api(token, Method::PATCH, &path, Some(body.clone())).await;
                        if res.is_err() && after_id.is_some() {
                            res = api(token, Method::PATCH, &path, Some(json!({ "children": children }))).await;
                        }
                    }
                }
                if let Ok(v) = &res {
                    // Some answers list all of the parent's children: the new ones are those not seen before.
                    let known: HashSet<&str> =
                        before.iter().filter_map(|l| l.id.as_deref()).chain(ids.iter().filter_map(|i| i.as_deref())).collect();
                    let created: Vec<String> = v["results"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|b| b["id"].as_str())
                        .filter(|id| !known.contains(id))
                        .map(str::to_string)
                        .collect();
                    for (&i, id) in lines.iter().zip(created) {
                        ids[i] = Some(id);
                    }
                }
                res
            }
        };
        match result {
            Ok(_) => {}
            Err(e) if e.starts_with("Нет связи") || e.contains("подождать") => return Err(e),
            // A block deleted in Notion meanwhile: nothing left to change there.
            Err(e) => eprintln!("notes: edit step {step:?}: {e}"),
        }
        // Progress survives a restart or a dropped connection.
        with_store(|s| {
            if let Some(Op::Edit { page_id: p, done, ids: saved, .. }) = s.outbox.first_mut() {
                if p == page_id {
                    *done = n + 1;
                    *saved = ids.clone();
                }
            }
            s.save_outbox();
        });
    }
    Ok(Value::Null)
}

/// Refreshes the cache from Notion. Without `force`, a recent sync is enough.
pub async fn sync(force: bool) -> Result<(), String> {
    let Some(source) = with_store(|s| s.settings.source.clone()).flatten() else { return Ok(()) };
    let Ok(token) = notion::token() else { return Ok(()) };
    let _guard = SYNC.lock().await;
    let fresh = with_store(|s| s.index.synced_at.is_some_and(|t| now_ms() - t < FRESH_MS)).unwrap_or(false);
    if fresh && !force && with_store(|s| s.outbox.is_empty()).unwrap_or(true) {
        return Ok(());
    }
    with_store(|s| s.syncing = true);
    changed();
    let result = sync_now(&token, &source).await;
    with_store(|s| {
        s.syncing = false;
        s.error = result.as_ref().err().cloned().or_else(|| s.create_error.take());
        if result.is_ok() {
            s.index.synced_at = Some(now_ms());
            s.save_index();
        }
    });
    changed();
    result
}

async fn sync_now(token: &str, source: &Source) -> Result<(), String> {
    flush(token, &source.id).await?;
    let ds = api(token, Method::GET, &format!("/data_sources/{}", source.id), None).await?;
    let schema = detect(&ds)?;
    with_store(|s| {
        s.index.tag_options = schema.tag_options.clone();
        s.index.can_tag = schema.tags.is_some();
        s.index.can_pin = schema.pinned.is_some();
    });

    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut body = json!({ "sorts": [{ "timestamp": "last_edited_time", "direction": "descending" }], "page_size": 100 });
        if let Some(c) = &cursor {
            body["start_cursor"] = json!(c);
        }
        let res = api(token, Method::POST, &format!("/data_sources/{}/query", source.id), Some(body)).await?;
        pages.extend(res["results"].as_array().into_iter().flatten().filter_map(|p| to_note(p, &schema)));
        match res["next_cursor"].as_str() {
            Some(c) if res["has_more"].as_bool() == Some(true) && pages.len() < MAX_NOTES => cursor = Some(c.to_string()),
            _ => break,
        }
    }
    pages.truncate(MAX_NOTES);

    let (dir, images, old): (PathBuf, PathBuf, HashMap<String, Entry>) = with_store(|s| {
        (s.dir.clone(), s.images(), s.index.entries.iter().map(|e| (e.note.id.clone(), e.clone())).collect())
    })
    .ok_or("Хранилище заметок не готово")?;

    let mut entries = Vec::with_capacity(pages.len());
    for (i, mut note) in pages.into_iter().enumerate() {
        let path = dir.join("pages").join(format!("{}.json", safe_name(&note.id)));
        let cached: Option<Page> = read_json(&path);
        let text = match (cached, old.get(&note.id)) {
            (Some(page), Some(entry)) if page.edited == note.edited => {
                note.preview = entry.note.preview.clone();
                entry.text.clone()
            }
            _ => {
                let mut blocks = children(token, &note.id, 0).await?;
                cache_images(&mut blocks, &images).await;
                let outbox = with_store(|s| s.outbox.clone()).unwrap_or_default();
                apply_pending(&note.id, &mut blocks, &outbox);
                let text = blocks::plain_text(&blocks);
                note.preview = preview_of(&text);
                write_json(&path, &Page { edited: note.edited.clone(), blocks });
                text
            }
        };
        entries.push(Entry { note, text });
        // Long first syncs show up as they go.
        if i % 20 == 19 {
            publish(source, &entries, &old, false);
        }
    }
    publish(source, &entries, &old, true);
    let kept = with_store(|s| s.index.entries.clone()).unwrap_or(entries);
    prune(&dir, &kept);
    Ok(())
}

/// Swaps in the synced entries, keeping notes made in the panel that Notion doesn't have yet.
fn publish(source: &Source, synced: &[Entry], old: &HashMap<String, Entry>, done: bool) {
    let seen: HashSet<&str> = synced.iter().map(|e| e.note.id.as_str()).collect();
    with_store(|s| {
        let mut entries: Vec<Entry> = s.index.entries.iter().filter(|e| e.note.local).cloned().collect();
        entries.extend(synced.iter().cloned());
        if !done {
            // Mid-sync: notes not reached yet stay as they were.
            entries.extend(old.values().filter(|e| !e.note.local && !seen.contains(e.note.id.as_str())).cloned());
        }
        // Made here moments ago: Notion's listing may not have them yet.
        let just_created = s.just_created();
        let listed: HashSet<String> = entries.iter().map(|e| e.note.id.clone()).collect();
        let lagging: Vec<Entry> = s
            .index
            .entries
            .iter()
            .filter(|e| just_created.contains(&e.note.id) && !listed.contains(&e.note.id))
            .cloned()
            .collect();
        entries.extend(lagging);
        s.index.source_id = source.id.clone();
        s.index.entries = entries;
        s.save_index();
    });
    changed();
}

/// Removes pages and images no note uses any more.
fn prune(dir: &Path, entries: &[Entry]) {
    let ids: HashSet<String> = entries.iter().map(|e| format!("{}.json", safe_name(&e.note.id))).collect();
    let mut used_images = HashSet::new();
    if let Ok(files) = std::fs::read_dir(dir.join("pages")) {
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if !ids.contains(&name) {
                let _ = std::fs::remove_file(f.path());
            } else if let Some(mut page) = read_json::<Page>(&f.path()) {
                blocks::each_mut(&mut page.blocks, &mut |b| {
                    if let Some(file) = &b.file {
                        used_images.insert(file.clone());
                    }
                });
            }
        }
    }
    if let Ok(files) = std::fs::read_dir(dir.join("images")) {
        for f in files.flatten() {
            if !used_images.contains(&*f.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
}

fn sync_soon() {
    tauri::async_runtime::spawn(async {
        let _ = sync(true).await;
    });
}

// ---- commands ----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// Notion is connected and a notes database is chosen.
    configured: bool,
    source: Option<Source>,
    /// Pinned first, then the most recently edited.
    notes: Vec<Note>,
    synced_at: Option<u64>,
    syncing: bool,
    error: Option<String>,
    /// Changes waiting for Notion.
    pending: usize,
    tag_options: Vec<Tag>,
    can_tag: bool,
    can_pin: bool,
}

#[tauri::command]
pub fn notes_state() -> State {
    let connected = notion::token().is_ok();
    with_store(|s| {
        let mut notes: Vec<Note> = s.index.entries.iter().map(|e| e.note.clone()).collect();
        notes.sort_by(|a, b| b.pinned.cmp(&a.pinned).then_with(|| b.edited.cmp(&a.edited)));
        State {
            configured: connected && s.settings.source.is_some(),
            source: s.settings.source.clone(),
            notes,
            synced_at: s.index.synced_at,
            syncing: s.syncing,
            error: s.error.clone(),
            pending: s.outbox.len(),
            tag_options: s.index.tag_options.clone(),
            can_tag: s.index.can_tag,
            can_pin: s.index.can_pin,
        }
    })
    .unwrap_or(State {
        configured: false,
        source: None,
        notes: Vec::new(),
        synced_at: None,
        syncing: false,
        error: None,
        pending: 0,
        tag_options: Vec::new(),
        can_tag: false,
        can_pin: false,
    })
}

/// Picks the notes database (or none); a new one starts from an empty cache.
#[tauri::command]
pub fn notes_set_source(source: Option<Source>) -> State {
    with_store(|s| {
        if s.settings.source != source {
            s.index = Index::default();
            s.outbox.clear();
            let _ = std::fs::remove_dir_all(s.dir.join("pages"));
            let _ = std::fs::remove_dir_all(s.images());
            s.save_index();
            s.save_outbox();
        }
        s.settings.source = source;
        s.error = None;
        write_json(&s.dir.join("settings.json"), &s.settings);
    });
    changed();
    sync_soon();
    notes_state()
}

#[tauri::command]
pub async fn notes_sync(force: bool) -> State {
    let _ = sync(force).await;
    notes_state()
}

/// A note's blocks from the cache; fetched on the spot if it isn't cached yet.
#[tauri::command]
pub async fn notes_page(id: String) -> Result<Vec<Block>, String> {
    let id = with_store(|s| s.resolve(&id)).unwrap_or(id);
    let path = with_store(|s| s.page_path(&id)).ok_or("Хранилище заметок не готово")?;
    if let Some(page) = read_json::<Page>(&path) {
        return Ok(page.blocks);
    }
    let token = notion::token()?;
    let edited = with_store(|s| s.index.entries.iter().find(|e| e.note.id == id).map(|e| e.note.edited.clone()))
        .flatten()
        .ok_or("Заметка не найдена")?;
    let mut blocks = children(&token, &id, 0).await?;
    if let Some(images) = with_store(|s| s.images()) {
        cache_images(&mut blocks, &images).await;
    }
    write_json(&path, &Page { edited, blocks: blocks.clone() });
    Ok(blocks)
}

/// Ticks or unticks a to-do: the cache at once, Notion when it can.
#[tauri::command]
pub fn notes_toggle(page_id: String, block_id: String, checked: bool) -> Result<Vec<Block>, String> {
    let blocks = with_store(|s| {
        let page_id = s.resolve(&page_id);
        let path = s.page_path(&page_id);
        let mut page: Page = read_json(&path).ok_or("Заметки нет в кэше")?;
        blocks::each_mut(&mut page.blocks, &mut |b| {
            if b.id == block_id {
                b.checked = Some(checked);
            }
        });
        write_json(&path, &page);
        // Ticking back and forth before a sync sends only the last state.
        s.outbox.retain(|op| !matches!(op, Op::Toggle { block_id: b, .. } if *b == block_id));
        if !is_local(&page_id) && !is_local(&block_id) {
            s.outbox.push(Op::Toggle { page_id: page_id.clone(), block_id: block_id.clone(), checked });
        }
        s.save_outbox();
        Ok::<_, String>(page.blocks)
    })
    .ok_or("Хранилище заметок не готово")??;
    changed();
    sync_soon();
    Ok(blocks)
}

/// A quick note: shown at once, created in Notion when it can be.
#[tauri::command]
pub fn notes_create(title: String, body: String) -> Result<Note, String> {
    let title = title.trim().to_string();
    if title.is_empty() && body.trim().is_empty() {
        return Err("Пустая заметка".into());
    }
    let title = if title.is_empty() { body.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string() } else { title };
    let now = now_ms();
    let local_id = format!("local-{now}");
    let blocks = blocks::local_blocks(&body, &local_id);
    let text = blocks::plain_text(&blocks);
    let note = Note {
        id: local_id.clone(),
        url: String::new(),
        title: title.clone(),
        icon: None,
        tags: Vec::new(),
        pinned: false,
        edited: iso(now),
        created: iso(now),
        preview: preview_of(&text),
        local: true,
    };
    with_store(|s| {
        if s.settings.source.is_none() {
            return Err("Сначала выберите базу заметок в настройках Notion".to_string());
        }
        write_json(&s.page_path(&local_id), &Page { edited: note.edited.clone(), blocks });
        s.index.entries.insert(0, Entry { note: note.clone(), text });
        s.outbox.push(Op::Create { local_id, title, body });
        s.save_index();
        s.save_outbox();
        Ok(())
    })
    .ok_or("Хранилище заметок не готово")??;
    changed();
    sync_soon();
    Ok(note)
}

/// The note's content as editable text (see `edit`).
#[tauri::command]
pub fn notes_text(id: String) -> Result<String, String> {
    let page: Page = with_store(|s| read_json(&s.page_path(&s.resolve(&id)))).flatten().ok_or("Заметки нет в кэше")?;
    Ok(edit::to_text(&edit::to_lines(&page.blocks)))
}

fn touch(s: &mut Store, id: &str, f: impl FnOnce(&mut Entry)) {
    if let Some(e) = s.index.entries.iter_mut().find(|e| e.note.id == id) {
        f(e);
        e.note.edited = iso(now_ms());
    }
    s.save_index();
}

/// Saves edited text: the cache at once, Notion (the difference only) when it can.
#[tauri::command]
pub fn notes_edit(id: String, text: String) -> Result<Vec<Block>, String> {
    let blocks = with_store(|s| {
        let id = s.resolve(&id);
        let path = s.page_path(&id);
        let mut page: Page = read_json(&path).ok_or("Заметки нет в кэше")?;

        if is_local(&id) {
            // Not in Notion yet: the pending creation takes the new text.
            for op in &mut s.outbox {
                if let Op::Create { local_id, body, .. } = op {
                    if *local_id == id {
                        *body = text.clone();
                    }
                }
            }
            page.blocks = blocks::local_blocks(&text, &id);
        } else {
            let pending = s.outbox.iter().position(|op| matches!(op, Op::Edit { page_id, .. } if *page_id == id));
            let before = match pending {
                // Still unsent: fold this edit into it, diffing from what Notion has.
                Some(i) => match &s.outbox[i] {
                    Op::Edit { done: 0, before, .. } => before.clone(),
                    _ => return Err("Предыдущая правка этой заметки ещё отправляется — попробуйте через минуту".to_string()),
                },
                None => edit::to_lines(&page.blocks),
            };
            let mut after = edit::from_text(&text);
            edit::match_ids(&before, &mut after);
            let current = edit::to_lines(&page.blocks);
            let mut shown = after.clone();
            // Lines already shown under local ids keep them in the cache.
            edit::match_ids(&current, &mut shown);
            page.blocks = edit::rebuild(&page.blocks, &shown, &format!("local-edit-{}", now_ms()));
            let op = Op::Edit { page_id: id.clone(), before, after, done: 0, ids: Vec::new() };
            match pending {
                Some(i) => s.outbox[i] = op,
                None => s.outbox.push(op),
            }
            s.save_outbox();
        }
        write_json(&path, &page);
        let text = blocks::plain_text(&page.blocks);
        touch(s, &id, |e| {
            e.note.preview = preview_of(&text);
            e.text = text.clone();
        });
        Ok::<_, String>(page.blocks)
    })
    .ok_or("Хранилище заметок не готово")??;
    changed();
    sync_soon();
    Ok(blocks)
}

/// Title, tags and pin; any left out stay as they are.
#[tauri::command]
pub fn notes_set_props(id: String, title: Option<String>, tags: Option<Vec<String>>, pinned: Option<bool>) -> Result<Note, String> {
    let note = with_store(|s| {
        let id = s.resolve(&id);
        if is_local(&id) {
            for op in &mut s.outbox {
                if let (Op::Create { local_id, title: t, .. }, Some(new)) = (&mut *op, &title) {
                    if *local_id == id {
                        *t = new.clone();
                    }
                }
            }
        } else {
            let merged = s.outbox.iter_mut().find_map(|op| match op {
                Op::Props { page_id, title: t, tags: g, pinned: p } if *page_id == id => {
                    if title.is_some() {
                        *t = title.clone();
                    }
                    if tags.is_some() {
                        *g = tags.clone();
                    }
                    if pinned.is_some() {
                        *p = pinned;
                    }
                    Some(())
                }
                _ => None,
            });
            if merged.is_none() {
                s.outbox.push(Op::Props { page_id: id.clone(), title: title.clone(), tags: tags.clone(), pinned });
            }
        }
        s.save_outbox();
        let options = s.index.tag_options.clone();
        touch(s, &id, |e| {
            if let Some(t) = &title {
                e.note.title = t.clone();
            }
            if let Some(tags) = &tags {
                e.note.tags = tags
                    .iter()
                    .map(|t| options.iter().find(|o| &o.name == t).cloned().unwrap_or(Tag { name: t.clone(), color: "default".into() }))
                    .collect();
            }
            if let Some(p) = pinned {
                e.note.pinned = p;
            }
        });
        s.index.entries.iter().find(|e| e.note.id == id).map(|e| e.note.clone()).ok_or("Заметка не найдена".to_string())
    })
    .ok_or("Хранилище заметок не готово")??;
    changed();
    sync_soon();
    Ok(note)
}

#[derive(Serialize)]
pub struct Hit {
    id: String,
    title: String,
    icon: Option<String>,
    snippet: String,
}

/// Notes whose title or text has every word of `query`; title matches first.
#[tauri::command]
pub fn notes_search(query: String, limit: usize) -> Vec<Hit> {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Vec::new();
    }
    with_store(|s| search(&s.index.entries, &words, limit)).unwrap_or_default()
}

fn search(entries: &[Entry], words: &[String], limit: usize) -> Vec<Hit> {
    let mut hits: Vec<(bool, &Entry)> = entries
        .iter()
        .filter_map(|e| {
            let title = e.note.title.to_lowercase();
            let text = e.text.to_lowercase();
            words
                .iter()
                .all(|w| title.contains(w.as_str()) || text.contains(w.as_str()))
                .then(|| (words.iter().all(|w| title.contains(w.as_str())), e))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.note.edited.cmp(&a.1.note.edited)));
    hits.into_iter()
        .take(limit.max(1))
        .map(|(_, e)| Hit { id: e.note.id.clone(), title: e.note.title.clone(), icon: e.note.icon.clone(), snippet: snippet(&e.text, &words[0]) })
        .collect()
}

/// The line of `text` where `word` first appears, trimmed around it.
fn snippet(text: &str, word: &str) -> String {
    let line = text.lines().find(|l| l.to_lowercase().contains(word)).or_else(|| text.lines().next()).unwrap_or("");
    let chars: Vec<char> = line.chars().collect();
    let lower: Vec<char> = line.to_lowercase().chars().collect();
    let at = lower.windows(word.chars().count().max(1)).position(|w| w.iter().copied().eq(word.chars())).unwrap_or(0);
    let start = at.saturating_sub(30);
    let end = (at + 90).min(chars.len());
    let mut out: String = chars[start..end].iter().collect();
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// `noteimg://localhost/<file>`: a cached image.
pub fn image_response(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let name = safe_name(request.uri().path().trim_start_matches('/'));
    let path = with_store(|s| s.images().join(&name));
    let bytes = path.filter(|_| !name.is_empty()).and_then(|p| std::fs::read(p).ok());
    let mime = match name.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    let builder = Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    match bytes {
        Some(b) => builder.status(200).header(header::CONTENT_TYPE, mime).header(header::CACHE_CONTROL, "max-age=604800").body(b),
        None => builder.status(404).body(Vec::new()),
    }
    .expect("static response parts are valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, title: &str, text: &str, edited: &str) -> Entry {
        Entry {
            note: Note {
                id: id.into(),
                url: String::new(),
                title: title.into(),
                icon: None,
                tags: Vec::new(),
                pinned: false,
                edited: edited.into(),
                created: String::new(),
                preview: String::new(),
                local: false,
            },
            text: text.into(),
        }
    }

    #[test]
    fn searches_titles_first() {
        let entries = [
            entry("a", "Покупки", "молоко\nхлеб", "2026-09-01"),
            entry("b", "Идеи", "виджет для покупки билетов\nещё", "2026-09-20"),
            entry("c", "Другое", "ничего", "2026-09-25"),
        ];
        let words = vec!["покупк".to_string()];
        let hits = search(&entries, &words, 10);
        assert_eq!(hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(hits[1].snippet, "виджет для покупки билетов");
        assert!(search(&entries, &["покупк".into(), "хлеб".into()], 10).len() == 1);
    }

    #[test]
    fn formats_iso() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_790_000_000_123), "2026-09-21T14:13:20.123Z");
    }

    #[test]
    fn detects_notes_schema() {
        let ds = json!({ "properties": {
            "Name": { "type": "title" },
            "Теги": { "type": "multi_select" },
            "Done": { "type": "checkbox" },
            "Закреплено": { "type": "checkbox" }
        }});
        let s = detect(&ds).unwrap();
        assert_eq!(s.title, "Name");
        assert_eq!(s.tags.map(|t| t.0).as_deref(), Some("Теги"));
        assert_eq!(s.pinned.as_deref(), Some("Закреплено"));
    }

    #[test]
    fn image_names_ignore_the_expiring_query() {
        let a = image_name("https://s3.aws/x/abc/photo.PNG?X-Amz-Expires=3600&sig=1");
        let b = image_name("https://s3.aws/x/abc/photo.PNG?X-Amz-Expires=3600&sig=2");
        assert_eq!(a, b);
        assert!(a.ends_with(".png"));
    }
}
