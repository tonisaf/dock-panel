//! New videos from the user's YouTube channels, through each channel's
//! public RSS feed: no API key, no Google sign-in. Channels are added by
//! link, @handle or ID, or imported from a Google Takeout subscriptions.csv.
//!
//! With the panel's Google sign-in (the calendar's), the channel list can
//! follow the account's subscriptions, and videos get their duration and
//! live/premiere state from the Data API (a few quota units per refresh).
//!
//! YouTube is throttled in some countries, so feeds are cached on disk and
//! a failed refresh keeps showing the last good copy.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::stream::{self, StreamExt};
use reqwest::header::{ACCEPT_LANGUAGE, COOKIE};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{gcal, net};

mod parse;
use parse::{
    channel_id_in_page, is_channel_id, parse_duration, parse_feed, parse_takeout, parse_time, Video,
};

const FILE: &str = "youtube.json";
const CACHE_FILE: &str = "youtube-cache.json";
const FEED_URL: &str = "https://www.youtube.com/feeds/videos.xml?channel_id=";
const REFRESH_EVERY: Duration = Duration::from_secs(15 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// Feeds fetched at once; gentle on a throttled connection.
const PARALLEL: usize = 8;
const FEED_MAX: usize = 150;
const WATCHED_MAX: usize = 5000;
const TOASTS_MAX: usize = 3;
/// Skips YouTube's cookie-consent interstitial in the EU.
const CONSENT_COOKIE: &str = "SOCS=CAI; CONSENT=YES+1";
const YT_API: &str = "https://www.googleapis.com/youtube/v3";
/// Subscriptions change rarely; re-read them at most this often.
const SUBS_EVERY_MS: i64 = 60 * 60_000;
/// 50 per page: up to 1000 subscriptions.
const SUBS_PAGES_MAX: usize = 20;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    id: String,
    title: String,
    /// Came from the Google account's subscriptions (and leaves with them).
    #[serde(default)]
    google: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    channels: Vec<Channel>,
    notify: bool,
    hide_shorts: bool,
    /// Oldest first; trimmed to `WATCHED_MAX`.
    watched: VecDeque<String>,
    /// Keep `channels` in step with the Google account's subscriptions.
    sync_google: bool,
    /// Subscriptions the user removed from the panel; sync leaves them out.
    ignored: HashSet<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Feed {
    fetched: i64,
    title: String,
    videos: Vec<Video>,
}

/// What the Data API adds to a feed's video.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Details {
    /// Seconds; `None` for live streams and upcoming premieres.
    duration: Option<u32>,
    /// "live" or "upcoming" (a scheduled stream or premiere).
    live: Option<String>,
    /// Scheduled start of an upcoming one, Unix ms.
    starts: Option<i64>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Cache {
    feeds: HashMap<String, Feed>,
    /// When the last refresh finished, and how many channels failed in it.
    refreshed: i64,
    failed: usize,
    /// By video ID.
    details: HashMap<String, Details>,
    /// Last subscriptions sync (tried), and why it failed if it did.
    subs_synced: i64,
    google_error: Option<String>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
/// Serialises refreshes: the background one and a manual one never overlap.
static REFRESHING: refresh_lock::Lock = refresh_lock::Lock::new();

/// A tiny async mutex on top of std, so one refresh waits for another.
mod refresh_lock {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    pub struct Lock(AtomicBool);
    pub struct Guard<'a>(&'a AtomicBool);

    impl Lock {
        pub const fn new() -> Self {
            Lock(AtomicBool::new(false))
        }
        pub async fn acquire(&self) -> Guard<'_> {
            while self.0.swap(true, Ordering::Acquire) {
                crate::net::sleep(Duration::from_millis(100)).await;
            }
            Guard(&self.0)
        }
    }

    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn data_path(app: &AppHandle, file: &str) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(file))
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(app: &AppHandle, file: &str) -> T {
    data_path(app, file)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(app: &AppHandle, file: &str, value: &T) -> Result<(), String> {
    let p = data_path(app, file)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, serde_json::to_string(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn load(app: &AppHandle) -> Saved {
    read_json(app, FILE)
}

fn save(app: &AppHandle, saved: &Saved) -> Result<(), String> {
    write_json(app, FILE, saved)
}

fn with_cache<R>(app: &AppHandle, f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(|| read_json(app, CACHE_FILE)))
}

// ---- network -----------------------------------------------------------------------

async fn get_text(url: &str) -> Result<String, String> {
    let res = net::client()
        .get(url)
        .header(COOKIE, CONSENT_COOKIE)
        .header(ACCEPT_LANGUAGE, "ru,en;q=0.8")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "YouTube не отвечает (может помочь VPN)".to_string()
            } else {
                format!("Нет связи с YouTube: {e}")
            }
        })?;
    let status = res.status();
    if !status.is_success() {
        return Err(if status.as_u16() == 404 {
            "канал не найден".into()
        } else {
            format!("YouTube: HTTP {status}")
        });
    }
    res.text().await.map_err(|e| e.to_string())
}

async fn fetch_feed(id: &str) -> Result<Feed, String> {
    let (title, mut videos) = parse_feed(&get_text(&format!("{FEED_URL}{id}")).await?)?;
    // Some feeds report the channel ID without its "UC" prefix; trust the one we asked for.
    for v in &mut videos {
        v.channel_id = id.to_string();
    }
    Ok(Feed {
        fetched: now_ms(),
        title,
        videos,
    })
}

async fn fetch_tagged(id: String) -> (String, Result<Feed, String>) {
    let result = fetch_feed(&id).await;
    (id, result)
}

/// Link, @handle, /c/ or /user/ name, video link or a bare ID → channel.
async fn resolve(input: &str) -> Result<Channel, String> {
    let input = input.trim().trim_end_matches('/');
    let id = if is_channel_id(input) {
        input.to_string()
    } else if let Some(at) = input.find("/channel/") {
        input[at + 9..].chars().take(24).collect()
    } else {
        let url = if input.starts_with('@') {
            format!("https://www.youtube.com/{input}")
        } else if input.starts_with("http") {
            input.to_string()
        } else if input.contains("youtube.com") || input.contains("youtu.be") {
            format!("https://{input}")
        } else {
            format!("https://www.youtube.com/@{input}")
        };
        channel_id_in_page(&get_text(&url).await?).ok_or("Не нашёл канал по этой ссылке")?
    };
    if !is_channel_id(&id) {
        return Err("Не похоже на ссылку на канал YouTube".into());
    }
    let feed = fetch_feed(&id).await?;
    Ok(Channel {
        id,
        title: feed.title,
        google: false,
    })
}

/// Fetches every channel's feed, keeps the old copy for those that fail,
/// and reports new videos (not in the previous copy) for notifications.
async fn refresh(app: &AppHandle) -> Vec<Video> {
    let _guard = REFRESHING.acquire().await;
    sync_subscriptions(app).await;
    let channels = load(app).channels;
    let ids: Vec<String> = channels.iter().map(|c| c.id.clone()).collect();
    let results: Vec<(String, Result<Feed, String>)> = stream::iter(ids)
        .map(fetch_tagged)
        .buffer_unordered(PARALLEL)
        .collect()
        .await;

    let mut fresh = Vec::new();
    with_cache(app, |cache| {
        let mut failed = 0;
        for (id, result) in results {
            match result {
                Ok(feed) => {
                    if let Some(old) = cache.feeds.get(&id) {
                        let known: HashSet<&str> =
                            old.videos.iter().map(|v| v.id.as_str()).collect();
                        let newest_old = old.videos.first().map_or(0, |v| v.published);
                        fresh.extend(
                            feed.videos
                                .iter()
                                .filter(|v| {
                                    !known.contains(v.id.as_str()) && v.published > newest_old
                                })
                                .cloned(),
                        );
                    }
                    cache.feeds.insert(id, feed);
                }
                Err(_) => failed += 1,
            }
        }
        // Channels removed since stay out of the cache.
        let ids: HashSet<&str> = channels.iter().map(|c| c.id.as_str()).collect();
        cache.feeds.retain(|id, _| ids.contains(id.as_str()));
        cache.refreshed = now_ms();
        cache.failed = failed;
        let _ = write_json(app, CACHE_FILE, cache);
    });
    fetch_details(app).await;
    let _ = app.emit("youtube:changed", ());
    fresh
}

// ---- Google account ----------------------------------------------------------------

/// Brings the channel list in line with the account's subscriptions, when
/// sync is on and the last sync is old enough.
async fn sync_subscriptions(app: &AppHandle) {
    if !load(app).sync_google || !with_cache(app, |c| now_ms() - c.subs_synced > SUBS_EVERY_MS) {
        return;
    }
    let result = fetch_subscriptions().await;
    let error = match result {
        Ok(subs) => {
            // Load again: the settings may have changed during the request.
            let mut saved = load(app);
            if saved.sync_google {
                merge_subscriptions(&mut saved, subs);
                let _ = save(app, &saved);
            }
            None
        }
        Err(e) => Some(e),
    };
    with_cache(app, |c| {
        c.subs_synced = now_ms();
        c.google_error = error;
        let _ = write_json(app, CACHE_FILE, c);
    });
}

async fn fetch_subscriptions() -> Result<Vec<Channel>, String> {
    if !gcal::can_use(gcal::YOUTUBE_SCOPE).await {
        return Err("Нет доступа к YouTube: войдите в Google в разделе «Google Календарь» (заново, если вход был раньше).".into());
    }
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..SUBS_PAGES_MAX {
        let mut url = format!("{YT_API}/subscriptions?part=snippet&mine=true&maxResults=50");
        if let Some(token) = &page {
            url.push_str("&pageToken=");
            url.push_str(
                &percent_encoding::utf8_percent_encode(token, percent_encoding::NON_ALPHANUMERIC)
                    .to_string(),
            );
        }
        let v = gcal::google_get(&url).await?;
        for item in v["items"].as_array().into_iter().flatten() {
            let snippet = &item["snippet"];
            if let Some(id) = snippet["resourceId"]["channelId"].as_str() {
                let title = snippet["title"].as_str().unwrap_or(id).to_string();
                out.push(Channel {
                    id: id.to_string(),
                    title,
                    google: true,
                });
            }
        }
        page = v["nextPageToken"].as_str().map(str::to_string);
        if page.is_none() {
            break;
        }
    }
    Ok(out)
}

/// Adds new subscriptions, drops channels that came from ones since cancelled;
/// channels added by hand stay either way.
fn merge_subscriptions(saved: &mut Saved, subs: Vec<Channel>) {
    let subscribed: HashSet<&str> = subs.iter().map(|c| c.id.as_str()).collect();
    saved
        .channels
        .retain(|c| !c.google || subscribed.contains(c.id.as_str()));
    // A cancelled subscription forgets its "removed from the panel" mark.
    saved.ignored.retain(|id| subscribed.contains(id.as_str()));
    let known: HashSet<String> = saved.channels.iter().map(|c| c.id.clone()).collect();
    saved.channels.extend(
        subs.into_iter()
            .filter(|c| !known.contains(&c.id) && !saved.ignored.contains(&c.id)),
    );
}

/// Duration and live state for the videos that will show and don't have them
/// yet; live and upcoming ones are checked again each time.
async fn fetch_details(app: &AppHandle) {
    if !gcal::can_use(gcal::YOUTUBE_SCOPE).await {
        return;
    }
    let ids: Vec<String> = with_cache(app, |c| {
        let mut videos: Vec<&Video> = c.feeds.values().flat_map(|f| f.videos.iter()).collect();
        videos.sort_by(|a, b| b.published.cmp(&a.published));
        videos.truncate(FEED_MAX);
        videos
            .into_iter()
            .filter(|v| c.details.get(&v.id).is_none_or(|d| d.live.is_some()))
            .map(|v| v.id.clone())
            .collect()
    });
    for chunk in ids.chunks(50) {
        let url = format!(
            "{YT_API}/videos?part=contentDetails,snippet,liveStreamingDetails&maxResults=50&id={}\
             &fields=items(id,contentDetails/duration,snippet/liveBroadcastContent,liveStreamingDetails/scheduledStartTime)",
            chunk.join(",")
        );
        let Ok(v) = gcal::google_get(&url).await else {
            break;
        };
        let mut found: HashMap<String, Details> = v["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let live = item["snippet"]["liveBroadcastContent"]
                    .as_str()
                    .filter(|l| *l == "live" || *l == "upcoming");
                let details = Details {
                    duration: item["contentDetails"]["duration"]
                        .as_str()
                        .and_then(parse_duration),
                    starts: live
                        .filter(|l| *l == "upcoming")
                        .and(item["liveStreamingDetails"]["scheduledStartTime"].as_str())
                        .and_then(parse_time),
                    live: live.map(str::to_string),
                };
                Some((item["id"].as_str()?.to_string(), details))
            })
            .collect();
        with_cache(app, |c| {
            for id in chunk {
                // Missing from the answer: private or deleted; don't ask again.
                c.details
                    .insert(id.clone(), found.remove(id).unwrap_or_default());
            }
        });
    }
    with_cache(app, |c| {
        let present: HashSet<String> = c
            .feeds
            .values()
            .flat_map(|f| f.videos.iter().map(|v| v.id.clone()))
            .collect();
        c.details.retain(|id, _| present.contains(id));
        let _ = write_json(app, CACHE_FILE, c);
    });
}

fn notify_new(app: &AppHandle, mut fresh: Vec<Video>) {
    if fresh.is_empty() || !load(app).notify {
        return;
    }
    fresh.sort_by(|a, b| b.published.cmp(&a.published));
    if fresh.len() > TOASTS_MAX {
        let channels: Vec<&str> = fresh
            .iter()
            .take(3)
            .map(|v| v.channel_title.as_str())
            .collect();
        notify(
            app,
            &format!("Новых видео: {}", fresh.len()),
            &channels.join(", "),
        );
    } else {
        for v in &fresh {
            notify(app, &v.channel_title, &v.title);
        }
    }
}

#[cfg(windows)]
fn notify(app: &AppHandle, title: &str, body: &str) {
    crate::alerts::notify(app, title, body);
}

#[cfg(not(windows))]
fn notify(_app: &AppHandle, _title: &str, _body: &str) {}

/// Refreshes in the background every 15 minutes.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if !load(&app).channels.is_empty() {
                let fresh = refresh(&app).await;
                notify_new(&app, fresh);
            }
            net::sleep(REFRESH_EVERY).await;
        }
    });
}

// ---- commands ----------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    channels: Vec<Channel>,
    notify: bool,
    hide_shorts: bool,
    sync_google: bool,
    /// Why the last subscriptions sync failed.
    google_error: Option<String>,
}

#[tauri::command]
pub fn youtube_settings(app: AppHandle) -> Settings {
    let s = load(&app);
    let google_error = if s.sync_google {
        with_cache(&app, |c| c.google_error.clone())
    } else {
        None
    };
    Settings {
        channels: s.channels,
        notify: s.notify,
        hide_shorts: s.hide_shorts,
        sync_google: s.sync_google,
        google_error,
    }
}

#[tauri::command]
pub async fn youtube_add(app: AppHandle, input: String) -> Result<Channel, String> {
    let channel = resolve(&input).await?;
    let mut saved = load(&app);
    if !saved.channels.iter().any(|c| c.id == channel.id) {
        saved.channels.push(channel.clone());
        save(&app, &saved)?;
    }
    refresh(&app).await;
    Ok(channel)
}

/// Picks a Takeout subscriptions.csv and adds its channels; returns how many were new.
#[tauri::command]
pub async fn youtube_import(app: AppHandle) -> Result<usize, String> {
    let picker = app.clone();
    let paths =
        tauri::async_runtime::spawn_blocking(move || crate::apps::pick_paths(&picker, false))
            .await
            .map_err(|e| e.to_string())??;
    let Some(path) = paths.first() else {
        return Ok(0);
    };
    let csv =
        std::fs::read_to_string(path).map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
    let found: Vec<Channel> = parse_takeout(&csv)
        .into_iter()
        .map(|(id, title)| Channel {
            id,
            title,
            google: false,
        })
        .collect();
    if found.is_empty() {
        return Err(
            "В файле нет каналов. Нужен subscriptions.csv из Google Takeout (YouTube → подписки)."
                .into(),
        );
    }
    let mut saved = load(&app);
    let known: HashSet<String> = saved.channels.iter().map(|c| c.id.clone()).collect();
    let new: Vec<Channel> = found
        .into_iter()
        .filter(|c| !known.contains(&c.id))
        .collect();
    let count = new.len();
    saved.channels.extend(new);
    save(&app, &saved)?;
    refresh(&app).await;
    Ok(count)
}

#[tauri::command]
pub fn youtube_remove(app: AppHandle, id: String) -> Result<(), String> {
    let mut saved = load(&app);
    if saved.channels.iter().any(|c| c.id == id && c.google) {
        saved.ignored.insert(id.clone());
    }
    saved.channels.retain(|c| c.id != id);
    save(&app, &saved)?;
    with_cache(&app, |cache| {
        cache.feeds.remove(&id);
        let _ = write_json(&app, CACHE_FILE, cache);
    });
    let _ = app.emit("youtube:changed", ());
    Ok(())
}

#[tauri::command]
pub async fn youtube_set_options(
    app: AppHandle,
    notify: Option<bool>,
    hide_shorts: Option<bool>,
    sync_google: Option<bool>,
) -> Result<(), String> {
    let mut saved = load(&app);
    if let Some(n) = notify {
        saved.notify = n;
    }
    if let Some(h) = hide_shorts {
        saved.hide_shorts = h;
    }
    let start_sync = sync_google == Some(true) && !saved.sync_google;
    if let Some(on) = sync_google {
        saved.sync_google = on;
        if !on {
            // The channels stay, as if added by hand.
            saved.channels.iter_mut().for_each(|c| c.google = false);
            saved.ignored.clear();
        }
    }
    save(&app, &saved)?;
    if start_sync {
        with_cache(&app, |c| c.subs_synced = 0);
        refresh(&app).await;
        if let Some(e) = with_cache(&app, |c| c.google_error.clone()) {
            return Err(e);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn youtube_set_watched(app: AppHandle, id: String, watched: bool) -> Result<(), String> {
    let mut saved = load(&app);
    saved.watched.retain(|w| w != &id);
    if watched {
        saved.watched.push_back(id);
        while saved.watched.len() > WATCHED_MAX {
            saved.watched.pop_front();
        }
    }
    save(&app, &saved)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    #[serde(flatten)]
    video: Video,
    watched: bool,
    #[serde(flatten)]
    details: Details,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedView {
    videos: Vec<FeedItem>,
    /// Unix ms of the last refresh; 0 before the first one.
    refreshed: i64,
    /// Channels whose feed failed in the last refresh (their old videos still show).
    failed: usize,
    channels: usize,
}

/// All channels' videos, newest first. Refreshes first when asked, or when
/// the cache is older than the background interval.
#[tauri::command]
pub async fn youtube_feed(app: AppHandle, force: bool) -> Result<FeedView, String> {
    let saved = load(&app);
    let stale = with_cache(&app, |c| {
        now_ms() - c.refreshed > REFRESH_EVERY.as_millis() as i64
    });
    if !saved.channels.is_empty() && (force || stale) {
        // New videos found here would otherwise never be announced.
        let fresh = refresh(&app).await;
        notify_new(&app, fresh);
    }
    let watched: HashSet<&str> = saved.watched.iter().map(String::as_str).collect();
    Ok(with_cache(&app, |cache| {
        let mut videos: Vec<FeedItem> = cache
            .feeds
            .values()
            .flat_map(|f| f.videos.iter())
            .filter(|v| !(saved.hide_shorts && v.short))
            .map(|v| FeedItem {
                watched: watched.contains(v.id.as_str()),
                details: cache.details.get(&v.id).cloned().unwrap_or_default(),
                video: v.clone(),
            })
            .collect();
        videos.sort_by(|a, b| b.video.published.cmp(&a.video.published));
        videos.truncate(FEED_MAX);
        FeedView {
            videos,
            refreshed: cache.refreshed,
            failed: cache.failed,
            channels: saved.channels.len(),
        }
    }))
}
