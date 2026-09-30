//! Local listening statistics for Spotify: how long, what and when.
//!
//! Spotify's API only knows the last 50 plays, so the panel keeps its own log.
//! It reads the Windows media session (like the "now playing" widget) every few
//! seconds, adds up the time actually listened and counts a play once a track
//! has run for 30 seconds, as Spotify does. Nothing leaves the machine; the log
//! is `listening.json` in the app's data folder, per-day figures for the last
//! three months.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::media::NowPlaying;

const FILE: &str = "listening.json";
/// How often the player is read while Spotify has a track, and while it doesn't.
const TICK: Duration = Duration::from_secs(5);
const IDLE_TICK: Duration = Duration::from_secs(15);
/// Unsaved listening is written out at least this often.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// Longest stretch credited for one reading: a sleeping PC must not add hours.
const MAX_STEP_MS: u64 = 2 * TICK.as_secs() * 1000;
/// A track counts as played after this much listening.
const PLAY_AFTER_MS: u64 = 30_000;
/// A position this far back, near the start, means the same track began again.
const REPEAT_JUMP_MS: u64 = 10_000;
const REPEAT_START_MS: u64 = 5_000;
/// Days of detail kept; the ranges the widget offers stay within it.
const KEEP_DAYS: i64 = 90;
/// Bars in the daily chart at most.
const CHART_DAYS: i64 = 14;
const TOP_N: usize = 5;
/// What Spotify calls an advert in its media session.
const ADS: [&str; 3] = ["advertisement", "реклама", "spotify"];

// ---- the log -----------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct TrackStat {
    title: String,
    artist: String,
    plays: u32,
    ms: u64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Day {
    ms: u64,
    plays: u32,
    /// Milliseconds listened in each hour of the day.
    hours: [u64; 24],
    artists: HashMap<String, u64>,
    tracks: HashMap<String, TrackStat>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Stats {
    /// `YYYY-MM-DD` (local) → that day.
    days: BTreeMap<String, Day>,
}

fn track_key(artist: &str, title: &str) -> String {
    format!("{artist}\u{1f}{title}")
}

impl Stats {
    fn day(&mut self, day: &str) -> &mut Day {
        self.days.entry(day.to_string()).or_default()
    }

    fn track(&mut self, day: &str, artist: &str, title: &str) -> &mut TrackStat {
        self.day(day).tracks.entry(track_key(artist, title)).or_insert_with(|| TrackStat {
            title: title.to_string(),
            artist: artist.to_string(),
            ..Default::default()
        })
    }

    fn listen(&mut self, day: &str, hour: usize, artist: &str, title: &str, ms: u64) {
        let d = self.day(day);
        d.ms += ms;
        d.hours[hour.min(23)] += ms;
        if !artist.is_empty() {
            *d.artists.entry(artist.to_string()).or_default() += ms;
        }
        self.track(day, artist, title).ms += ms;
    }

    fn play(&mut self, day: &str, artist: &str, title: &str) {
        self.day(day).plays += 1;
        self.track(day, artist, title).plays += 1;
    }

    /// Drops the days beyond what is kept.
    fn prune(&mut self, today: i64) {
        let oldest = day_key(today - KEEP_DAYS);
        self.days.retain(|k, _| k.as_str() >= oldest.as_str());
    }
}

// ---- dates -------------------------------------------------------------------------

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn day_key(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's day number and the hour of day, local time.
fn local_now() -> (i64, usize) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (days_from_civil(t.wYear.into(), t.wMonth.into(), t.wDay.into()), t.wHour as usize)
}

// ---- watching the player -----------------------------------------------------------

/// What one reading of the player amounted to.
#[derive(PartialEq, Debug)]
enum Outcome {
    Nothing,
    Listened,
    /// The track just reached its 30 seconds: worth saving at once.
    Played,
}

struct Current {
    key: String,
    listened_ms: u64,
    counted: bool,
    position_ms: Option<u64>,
    /// Playing at the previous reading, so the time since then was listened.
    was_playing: bool,
}

#[derive(Default)]
struct Tracker {
    current: Option<Current>,
}

fn is_music(np: &NowPlaying) -> bool {
    np.source.to_lowercase().contains("spotify")
        && !np.title.is_empty()
        && !ADS.contains(&np.title.to_lowercase().as_str())
}

impl Tracker {
    /// Takes a reading of the player `elapsed_ms` after the last one and files it in `stats`.
    fn observe(&mut self, np: Option<&NowPlaying>, elapsed_ms: u64, day: &str, hour: usize, stats: &mut Stats) -> Outcome {
        let Some(np) = np.filter(|n| is_music(n)) else {
            self.current = None;
            return Outcome::Nothing;
        };
        let key = track_key(&np.artist, &np.title);
        let fresh = match &self.current {
            None => true,
            Some(c) if c.key != key => true,
            // The same track from its start again: a new play.
            Some(c) => c
                .position_ms
                .zip(np.position_ms)
                .is_some_and(|(was, now)| now + REPEAT_JUMP_MS < was && now < REPEAT_START_MS),
        };
        if fresh {
            self.current = Some(Current { key, listened_ms: 0, counted: false, position_ms: None, was_playing: false });
        }
        let Some(cur) = self.current.as_mut() else { return Outcome::Nothing };
        let credited = if cur.was_playing && np.playing { elapsed_ms.min(MAX_STEP_MS) } else { 0 };
        cur.was_playing = np.playing;
        cur.position_ms = np.position_ms;
        if credited == 0 {
            return Outcome::Nothing;
        }

        stats.listen(day, hour, &np.artist, &np.title, credited);
        cur.listened_ms += credited;
        let needed = np.duration_ms.map_or(PLAY_AFTER_MS, |d| d.min(PLAY_AFTER_MS));
        if !cur.counted && cur.listened_ms >= needed {
            cur.counted = true;
            stats.play(day, &np.artist, &np.title);
            return Outcome::Played;
        }
        Outcome::Listened
    }
}

// ---- summary for the widget --------------------------------------------------------

#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRow {
    name: String,
    ms: u64,
}

#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    title: String,
    artist: String,
    plays: u32,
    ms: u64,
}

#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DayBar {
    /// `YYYY-MM-DD`.
    day: String,
    ms: u64,
}

#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    total_ms: u64,
    plays: u32,
    /// Days in the range with any listening.
    active_days: u32,
    artists: Vec<ArtistRow>,
    tracks: Vec<TrackRow>,
    /// Milliseconds listened in each hour of the day, over the range.
    hours: [u64; 24],
    /// The last days of the range (at most two weeks), oldest first.
    daily: Vec<DayBar>,
}

impl Stats {
    /// The `days` days up to and including `today`.
    fn summarize(&self, days: i64, today: i64) -> Summary {
        let from = day_key(today - days + 1);
        let range = || self.days.iter().filter(|(k, _)| k.as_str() >= from.as_str());

        let mut artists: HashMap<&str, u64> = HashMap::new();
        let mut tracks: HashMap<&str, TrackStat> = HashMap::new();
        let mut hours = [0u64; 24];
        let (mut total_ms, mut plays, mut active_days) = (0, 0, 0);
        for (_, d) in range() {
            total_ms += d.ms;
            plays += d.plays;
            active_days += u32::from(d.ms > 0);
            for (h, ms) in d.hours.iter().enumerate() {
                hours[h] += ms;
            }
            for (name, ms) in &d.artists {
                *artists.entry(name).or_default() += ms;
            }
            for (key, t) in &d.tracks {
                let sum = tracks.entry(key).or_insert_with(|| TrackStat { plays: 0, ms: 0, ..t.clone() });
                sum.plays += t.plays;
                sum.ms += t.ms;
            }
        }

        let mut artists: Vec<ArtistRow> =
            artists.into_iter().map(|(name, ms)| ArtistRow { name: name.to_string(), ms }).collect();
        artists.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.name.cmp(&b.name)));
        artists.truncate(TOP_N);

        // Only tracks that were played, not just sampled.
        let mut tracks: Vec<TrackRow> = tracks
            .into_values()
            .filter(|t| t.plays > 0)
            .map(|t| TrackRow { title: t.title, artist: t.artist, plays: t.plays, ms: t.ms })
            .collect();
        tracks.sort_by(|a, b| {
            b.plays.cmp(&a.plays).then(b.ms.cmp(&a.ms)).then_with(|| a.title.cmp(&b.title))
        });
        tracks.truncate(TOP_N);

        let daily = (0..days.min(CHART_DAYS))
            .rev()
            .map(|back| {
                let day = day_key(today - back);
                let ms = self.days.get(&day).map_or(0, |d| d.ms);
                DayBar { day, ms }
            })
            .collect();

        Summary { total_ms, plays, active_days, artists, tracks, hours, daily }
    }
}

// ---- state, thread, commands -------------------------------------------------------

static APP: OnceLock<AppHandle> = OnceLock::new();
static STATS: Mutex<Option<Stats>> = Mutex::new(None);

fn with_stats<R>(f: impl FnOnce(&mut Stats) -> R) -> R {
    let mut guard = STATS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Stats::default))
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(FILE))
}

fn save() {
    let Some(p) = APP.get().and_then(path) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(text) = with_stats(|s| serde_json::to_string(s)) else { return };
    // Through a temp file, so a crash mid-write can't leave a torn log.
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let saved = path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Stats>(&t).ok())
        .unwrap_or_default();
    with_stats(|s| *s = saved);
    std::thread::Builder::new().name("listening".into()).spawn(watch).ok();
}

fn watch() {
    let mut tracker = Tracker::default();
    let mut last = Instant::now();
    let mut last_save = Instant::now();
    let mut unsaved = false;
    loop {
        let np = crate::media::win::now_playing().ok().flatten();
        let now = Instant::now();
        let elapsed_ms = now.duration_since(last).as_millis() as u64;
        last = now;

        let (today, hour) = local_now();
        let day = day_key(today);
        let outcome = with_stats(|s| tracker.observe(np.as_ref(), elapsed_ms, &day, hour, s));
        unsaved |= outcome != Outcome::Nothing;
        if unsaved && (outcome == Outcome::Played || last_save.elapsed() >= SAVE_EVERY) {
            with_stats(|s| s.prune(today));
            save();
            unsaved = false;
            last_save = Instant::now();
        }
        std::thread::sleep(if tracker.current.is_some() { TICK } else { IDLE_TICK });
    }
}

/// Figures for the last `days` days (clamped to what is kept).
#[tauri::command]
pub fn listening_summary(days: u32) -> Summary {
    let days = i64::from(days).clamp(1, KEEP_DAYS);
    let (today, _) = local_now();
    with_stats(|s| s.summarize(days, today))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn np(title: &str, artist: &str, playing: bool, position_ms: Option<u64>) -> NowPlaying {
        NowPlaying {
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            source: "Spotify.exe".into(),
            playing,
            can_prev: true,
            can_next: true,
            can_seek: true,
            position_ms,
            duration_ms: Some(200_000),
        }
    }

    /// Feeds `n` readings, 5 s apart, of the same state.
    fn feed(t: &mut Tracker, s: &mut Stats, n: usize, np: &NowPlaying) -> Vec<Outcome> {
        (0..n).map(|_| t.observe(Some(np), 5_000, "2026-01-01", 10, s)).collect()
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(day_key(0), "1970-01-01");
        assert_eq!(day_key(days_from_civil(2026, 9, 30)), "2026-09-30");
        assert_eq!(day_key(days_from_civil(2024, 2, 29) + 1), "2024-03-01");
        assert_eq!(day_key(days_from_civil(2026, 1, 1) - 1), "2025-12-31");
    }

    #[test]
    fn counts_a_play_after_thirty_seconds() {
        let (mut t, mut s) = (Tracker::default(), Stats::default());
        let out = feed(&mut t, &mut s, 9, &np("Song", "Band", true, Some(0)));
        // The first reading only starts the track; 30 s are listened by the 7th.
        assert_eq!(out.iter().filter(|o| **o == Outcome::Played).count(), 1);
        assert_eq!(out[6], Outcome::Played);
        let d = &s.days["2026-01-01"];
        assert_eq!((d.plays, d.ms, d.hours[10]), (1, 40_000, 40_000));
        assert_eq!(d.artists["Band"], 40_000);
    }

    #[test]
    fn a_skipped_track_is_listened_but_not_played() {
        let (mut t, mut s) = (Tracker::default(), Stats::default());
        feed(&mut t, &mut s, 4, &np("Song", "Band", true, Some(0)));
        feed(&mut t, &mut s, 4, &np("Other", "Band", true, Some(0)));
        let d = &s.days["2026-01-01"];
        assert_eq!(d.plays, 0);
        assert_eq!(d.ms, 30_000);
        assert!(s.summarize(1, days_from_civil(2026, 1, 1)).tracks.is_empty());
    }

    #[test]
    fn pause_adds_nothing_and_resume_continues_the_same_play() {
        let (mut t, mut s) = (Tracker::default(), Stats::default());
        feed(&mut t, &mut s, 5, &np("Song", "Band", true, Some(0)));
        feed(&mut t, &mut s, 20, &np("Song", "Band", false, Some(20_000)));
        assert_eq!(s.days["2026-01-01"].ms, 20_000);
        // The first reading after a pause credits nothing, the rest do.
        feed(&mut t, &mut s, 4, &np("Song", "Band", true, Some(20_000)));
        assert_eq!(s.days["2026-01-01"].ms, 35_000);
        assert_eq!(s.days["2026-01-01"].plays, 1);
    }

    #[test]
    fn a_track_started_again_counts_again() {
        let (mut t, mut s) = (Tracker::default(), Stats::default());
        feed(&mut t, &mut s, 9, &np("Song", "Band", true, Some(60_000)));
        // Back to the start of the same track.
        feed(&mut t, &mut s, 9, &np("Song", "Band", true, Some(1_000)));
        assert_eq!(s.days["2026-01-01"].plays, 2);
    }

    #[test]
    fn a_long_gap_is_capped() {
        let (mut t, mut s) = (Tracker::default(), Stats::default());
        let n = np("Song", "Band", true, Some(0));
        t.observe(Some(&n), 0, "2026-01-01", 10, &mut s);
        t.observe(Some(&n), 3_600_000, "2026-01-01", 10, &mut s);
        assert_eq!(s.days["2026-01-01"].ms, MAX_STEP_MS);
    }

    #[test]
    fn ignores_other_players_and_ads() {
        let (mut t, mut s) = (Tracker::default(), Stats::default());
        let mut chrome = np("Video", "Channel", true, Some(0));
        chrome.source = "chrome.exe".into();
        feed(&mut t, &mut s, 9, &chrome);
        feed(&mut t, &mut s, 9, &np("Advertisement", "Spotify", true, Some(0)));
        assert!(s.days.is_empty());
        assert_eq!(t.observe(None, 5_000, "2026-01-01", 10, &mut s), Outcome::Nothing);
    }

    #[test]
    fn summary_ranks_and_limits_to_the_range() {
        let today = days_from_civil(2026, 3, 10);
        let mut s = Stats::default();
        for (offset, artist, title, ms, plays) in [
            (0, "A", "x", 300_000, 3),
            (1, "A", "x", 100_000, 1),
            (1, "B", "y", 500_000, 2),
            (20, "C", "z", 900_000, 9),
        ] {
            let day = day_key(today - offset);
            s.listen(&day, 9, artist, title, ms);
            for _ in 0..plays {
                s.play(&day, artist, title);
            }
        }
        let week = s.summarize(7, today);
        assert_eq!((week.total_ms, week.plays, week.active_days), (900_000, 6, 2));
        assert_eq!(week.artists[0], ArtistRow { name: "B".into(), ms: 500_000 });
        assert_eq!(week.tracks[0], TrackRow { title: "x".into(), artist: "A".into(), plays: 4, ms: 400_000 });
        assert_eq!(week.hours[9], 900_000);
        assert_eq!(week.daily.len(), 7);
        assert_eq!(week.daily[6], DayBar { day: "2026-03-10".into(), ms: 300_000 });
        assert_eq!(week.daily[0].ms, 0);

        let month = s.summarize(30, today);
        assert_eq!(month.total_ms, 1_800_000);
        assert_eq!(month.artists[0].name, "C");
        assert_eq!(month.daily.len(), 14);
    }

    #[test]
    fn prune_drops_old_days() {
        let today = days_from_civil(2026, 6, 1);
        let mut s = Stats::default();
        s.listen(&day_key(today - KEEP_DAYS - 1), 1, "A", "x", 1);
        s.listen(&day_key(today - KEEP_DAYS), 1, "A", "x", 1);
        s.prune(today);
        assert_eq!(s.days.len(), 1);
    }
}
