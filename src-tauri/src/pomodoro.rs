//! Pomodoro timer: focus, short break, and a long break after every few
//! focus sessions. It runs here rather than in the webview so it keeps time
//! while the panel is hidden, and it is saved so a restart picks it up.
//! A phase's end raises a toast; clicking it starts the next phase.

use std::path::PathBuf;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const FILE: &str = "pomodoro.json";
const EVENT: &str = "pomodoro:changed";
const TICK: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Focus,
    Short,
    Long,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    focus_min: u32,
    short_min: u32,
    long_min: u32,
    /// A long break after this many focus sessions.
    long_every: u32,
    /// The next phase starts by itself when one ends.
    auto_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { focus_min: 25, short_min: 5, long_min: 15, long_every: 4, auto_start: false }
    }
}

impl Settings {
    fn duration_ms(&self, phase: Phase) -> u64 {
        let min = match phase {
            Phase::Focus => self.focus_min,
            Phase::Short => self.short_min,
            Phase::Long => self.long_min,
        };
        min.clamp(1, 180) as u64 * 60_000
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Timer {
    settings: Settings,
    phase: Phase,
    running: bool,
    /// Unix ms at which the running phase ends.
    ends_at: Option<u64>,
    /// Time left while paused or not started.
    remaining_ms: u64,
    /// Focus sessions finished in the current cycle (before the long break).
    done_in_cycle: u32,
    /// Focus sessions finished today, and which day that is.
    today: u32,
    day: String,
}

impl Default for Timer {
    fn default() -> Self {
        let settings = Settings::default();
        let remaining_ms = settings.duration_ms(Phase::Focus);
        Timer {
            settings,
            phase: Phase::Focus,
            running: false,
            ends_at: None,
            remaining_ms,
            done_in_cycle: 0,
            today: 0,
            day: String::new(),
        }
    }
}

impl Timer {
    fn left_ms(&self, now: u64) -> u64 {
        match (self.running, self.ends_at) {
            (true, Some(end)) => end.saturating_sub(now),
            _ => self.remaining_ms,
        }
    }

    fn start(&mut self, now: u64) {
        if !self.running {
            self.running = true;
            self.ends_at = Some(now + self.remaining_ms);
        }
    }

    fn pause(&mut self, now: u64) {
        if self.running {
            self.remaining_ms = self.left_ms(now);
            self.running = false;
            self.ends_at = None;
        }
    }

    /// Switches to `phase` at its full length, stopped.
    fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.running = false;
        self.ends_at = None;
        self.remaining_ms = self.settings.duration_ms(phase);
    }

    fn next_phase(&self) -> Phase {
        match self.phase {
            Phase::Focus if self.done_in_cycle >= self.settings.long_every.max(1) => Phase::Long,
            Phase::Focus => Phase::Short,
            _ => Phase::Focus,
        }
    }

    /// Ends the current phase; `counted` when it ran to the end (a skipped focus doesn't count).
    fn finish(&mut self, now: u64, day: &str, counted: bool) -> Phase {
        let ended = self.phase;
        if ended == Phase::Focus && counted {
            if self.day != day {
                self.day = day.to_string();
                self.today = 0;
            }
            self.today += 1;
            self.done_in_cycle += 1;
        }
        let next = self.next_phase();
        if ended == Phase::Long {
            self.done_in_cycle = 0;
        }
        self.set_phase(next);
        if counted && self.settings.auto_start {
            self.start(now);
        }
        ended
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static TIMER: Mutex<Option<Timer>> = Mutex::new(None);
/// Wakes the ticker when the timer changes, so it can sleep while nothing runs.
static WAKE: Condvar = Condvar::new();

fn with_timer<R>(f: impl FnOnce(&mut Timer) -> R) -> R {
    let mut guard = TIMER.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Timer::default))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(FILE))
}

fn save() {
    let Some(p) = APP.get().and_then(path) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = with_timer(|t| serde_json::to_string_pretty(t)) {
        let _ = std::fs::write(p, text);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    phase: Phase,
    running: bool,
    /// Unix ms; the UI counts down to it on its own.
    ends_at: Option<u64>,
    remaining_ms: u64,
    total_ms: u64,
    done_in_cycle: u32,
    today: u32,
    settings: Settings,
}

fn snapshot() -> State {
    let (now, day) = (now_ms(), today());
    with_timer(|t| State {
        phase: t.phase,
        running: t.running,
        ends_at: t.ends_at.filter(|_| t.running),
        remaining_ms: t.left_ms(now),
        total_ms: t.settings.duration_ms(t.phase),
        done_in_cycle: t.done_in_cycle,
        today: if t.day == day { t.today } else { 0 },
        settings: t.settings.clone(),
    })
}

/// Saves, and tells the UI.
fn changed() -> State {
    WAKE.notify_all();
    save();
    let state = snapshot();
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, &state);
    }
    state
}

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let saved = path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Timer>(&t).ok())
        .unwrap_or_default();
    with_timer(|t| *t = saved);
    std::thread::Builder::new()
        .name("pomodoro".into())
        .spawn(|| loop {
            tick();
            idle_until_running();
        })
        .ok();
}

/// Waits one tick while the timer runs; while it doesn't, until `changed` says
/// something did, so a stopped timer costs no wakeups.
fn idle_until_running() {
    let mut guard = TIMER.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if guard.as_ref().is_some_and(|t| t.running) {
            let _ = WAKE.wait_timeout(guard, TICK);
            return;
        }
        guard = WAKE.wait(guard).unwrap_or_else(|e| e.into_inner());
    }
}

fn tick() {
    let now = now_ms();
    let due = with_timer(|t| t.running && t.ends_at.is_some_and(|end| now >= end));
    if !due {
        return;
    }
    let day = today();
    let (ended, next, running, done, every) = with_timer(|t| {
        let ended = t.finish(now, &day, true);
        (ended, t.phase, t.running, t.done_in_cycle, t.settings.long_every)
    });
    changed();
    announce(ended, next, running, done, every);
}

fn minutes(phase: Phase) -> u32 {
    with_timer(|t| (t.settings.duration_ms(phase) / 60_000) as u32)
}

fn announce(ended: Phase, next: Phase, running: bool, done: u32, every: u32) {
    let Some(app) = APP.get() else { return };
    let title = match ended {
        Phase::Focus => "Помидор готов",
        _ => "Перерыв окончен",
    };
    let what = match next {
        Phase::Focus => format!("фокус, {} мин", minutes(next)),
        Phase::Short => format!("перерыв, {} мин", minutes(next)),
        Phase::Long => format!("длинный перерыв, {} мин", minutes(next)),
    };
    let progress = if ended == Phase::Focus { format!(" ({done} из {every})") } else { String::new() };
    let body = if running {
        format!("Дальше {what}, уже идёт.{progress}")
    } else {
        format!("Дальше {what}. Нажмите, чтобы начать.{progress}")
    };
    let on_click: Option<Box<dyn Fn() + Send + Sync>> = if running {
        None
    } else {
        Some(Box::new(|| {
            with_timer(|t| t.start(now_ms()));
            changed();
        }))
    };
    crate::alerts::notify_then(app, title, &body, on_click);
}

/// For the taskbar button: `(focus, seconds left, running)` while a phase is
/// under way, i.e. running or paused part-way.
pub fn taskbar_state() -> Option<(bool, u64, bool)> {
    let now = now_ms();
    with_timer(|t| {
        let left = t.left_ms(now);
        let started = t.running || left < t.settings.duration_ms(t.phase);
        started.then(|| (t.phase == Phase::Focus, left.div_ceil(1000), t.running))
    })
}

// ---- commands ----------------------------------------------------------------------

#[tauri::command]
pub fn pomodoro_state() -> State {
    snapshot()
}

#[tauri::command]
pub fn pomodoro_start() -> State {
    with_timer(|t| t.start(now_ms()));
    changed()
}

#[tauri::command]
pub fn pomodoro_pause() -> State {
    with_timer(|t| t.pause(now_ms()));
    changed()
}

/// Back to the start of the current phase, stopped.
#[tauri::command]
pub fn pomodoro_reset() -> State {
    with_timer(|t| t.set_phase(t.phase));
    changed()
}

/// On to the next phase without counting this one.
#[tauri::command]
pub fn pomodoro_skip() -> State {
    let day = today();
    with_timer(|t| t.finish(now_ms(), &day, false));
    changed()
}

#[tauri::command]
pub fn pomodoro_set_phase(phase: Phase) -> State {
    with_timer(|t| t.set_phase(phase));
    changed()
}

#[tauri::command]
pub fn pomodoro_set_settings(settings: Settings) -> State {
    with_timer(|t| {
        let reshape = !t.running && t.remaining_ms == t.settings.duration_ms(t.phase);
        t.settings = settings;
        // A phase that hasn't started takes the new length.
        if reshape {
            t.remaining_ms = t.settings.duration_ms(t.phase);
        }
    });
    changed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_to_a_long_break() {
        let mut t = Timer::default();
        t.settings.long_every = 2;
        t.start(0);
        assert_eq!(t.left_ms(60_000), 24 * 60_000);
        assert_eq!(t.finish(0, "d", true), Phase::Focus);
        assert_eq!((t.phase, t.running, t.today), (Phase::Short, false, 1));
        t.finish(0, "d", true);
        assert_eq!(t.phase, Phase::Focus);
        t.finish(0, "d", true);
        assert_eq!((t.phase, t.done_in_cycle), (Phase::Long, 2));
        t.finish(0, "d", true);
        assert_eq!((t.phase, t.done_in_cycle, t.today), (Phase::Focus, 0, 2));
    }

    #[test]
    fn skipping_focus_does_not_count_and_a_new_day_starts_over() {
        let mut t = Timer::default();
        t.finish(0, "d1", false);
        assert_eq!((t.phase, t.today), (Phase::Short, 0));
        t.set_phase(Phase::Focus);
        t.finish(0, "d1", true);
        t.set_phase(Phase::Focus);
        t.finish(0, "d2", true);
        assert_eq!(t.today, 1);
    }

    #[test]
    fn pause_keeps_the_time_left() {
        let mut t = Timer::default();
        t.start(1_000);
        t.pause(61_000);
        assert_eq!(t.left_ms(999_999), 24 * 60_000);
        t.start(100_000);
        assert_eq!(t.ends_at, Some(100_000 + 24 * 60_000));
    }
}
