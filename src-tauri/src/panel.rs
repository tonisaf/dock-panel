//! The slide-out panel window: placement, show/hide lifecycle, native styling.
//!
//! Hide is two-phase so the UI can play its exit animation: Rust emits
//! `panel:hide`, the frontend animates out and calls `hide_panel`. A watchdog
//! hides the window anyway if the frontend never answers.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub const LABEL: &str = "main";
/// Panel width in logical pixels: default and the range the user can pick.
/// One column of the default 500 px widgets (see `panelWidthFor` in panelWidth.ts).
pub const DEFAULT_WIDTH: u32 = 540;
pub const MIN_WIDTH: u32 = 360;
pub const MAX_WIDTH: u32 = 2400;
const SETTINGS_FILE: &str = "panel.json";
pub const DEFAULT_SHORTCUT: &str = "Ctrl+Space";
/// Gap between the panel and the screen edges, logical pixels.
const MARGIN: f64 = 8.0;
/// Upper bound for the frontend exit animation before we force-hide.
const HIDE_WATCHDOG: Duration = Duration::from_millis(450);
/// A show request this soon after a hide is ignored. Clicking the tray icon
/// while the panel is open first blurs it (hide), then fires a click (show).
const RESHOW_DEBOUNCE_MS: u64 = 250;

static VISIBLE: AtomicBool = AtomicBool::new(false);
static HIDING: AtomicBool = AtomicBool::new(false);
static LAST_HIDE_MS: AtomicU64 = AtomicU64::new(0);
static WIDTH: AtomicU32 = AtomicU32::new(DEFAULT_WIDTH);
/// Temporary width on top of `WIDTH` (logical px), e.g. while a letter is open
/// next to the mail list. Never saved; dropped when the panel hides.
static EXTRA: AtomicU32 = AtomicU32::new(0);
/// Bumped by every extra-width change; a running animation stops when it moves on.
static RESIZE_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Held while an animation frame or a hide touches EXTRA.
static RESIZE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
const RESIZE_ANIMATION: Duration = Duration::from_millis(200);
const RESIZE_FRAME: Duration = Duration::from_millis(8);

/// The window's current logical width: the user's width plus any extra.
fn window_width() -> u32 {
    WIDTH.load(Ordering::SeqCst) + EXTRA.load(Ordering::SeqCst)
}

/// Full-screen mode: the window spans the whole width of its monitor (home on
/// the left, the other tabs on the right). Kept while the app runs.
static FULL: AtomicBool = AtomicBool::new(false);

/// Switches full-screen mode and re-places the window; returns the new state.
#[tauri::command]
pub fn panel_set_fullscreen(app: AppHandle, on: bool) -> bool {
    FULL.store(on, Ordering::SeqCst);
    if on {
        // The letter pane's extra width means nothing on a full-width window.
        RESIZE_GENERATION.fetch_add(1, Ordering::SeqCst);
        EXTRA.store(0, Ordering::SeqCst);
    }
    if let Some(win) = window(&app) {
        place_on_cursor_monitor(&app, &win);
    }
    on
}

#[tauri::command]
pub fn panel_fullscreen() -> bool {
    FULL.load(Ordering::SeqCst)
}

/// Resizes the window to `window_width()`; docked right, it grows leftwards
/// so the right edge stays put.
fn apply_width(win: &WebviewWindow) {
    if FULL.load(Ordering::SeqCst) {
        return;
    }
    let (Ok(scale), Ok(inner), Ok(outer), Ok(pos)) =
        (win.scale_factor(), win.inner_size(), win.outer_size(), win.outer_position())
    else {
        return;
    };
    let new_w = (window_width() as f64 * scale).round() as u32;
    // `set_size` takes the inner size. Feeding it the outer height grew the
    // window by its frame on every call, so a drag pushed the bottom off screen.
    // The height is the monitor's, as when the panel is placed.
    let height = win
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| m.work_area().size.height.saturating_sub(2 * (MARGIN * m.scale_factor()).round() as u32))
        .filter(|&h| h > 0)
        .unwrap_or(inner.height);
    let _ = win.set_size(PhysicalSize::new(new_w, height));
    if RIGHT.load(Ordering::SeqCst) {
        // Keep the right edge where it was.
        let new_outer_w = new_w + outer.width.saturating_sub(inner.width);
        let _ = win.set_position(PhysicalPosition::new(pos.x + outer.width as i32 - new_outer_w as i32, pos.y));
    }
}
/// Dock to the right screen edge instead of the left.
static RIGHT: AtomicBool = AtomicBool::new(false);
static SHORTCUT: Mutex<String> = Mutex::new(String::new());
/// Set while a modal dialog owned by the panel is open: its focus steal must not hide us.
static KEEP_OPEN: AtomicBool = AtomicBool::new(false);
/// Pinned by the user: clicking elsewhere doesn't hide the panel. The hotkey,
/// the tray and taskbar buttons and the panel's own hide button still do.
static PINNED: AtomicBool = AtomicBool::new(false);
/// Held open by "blackout around the panel": the panel stays visible and on
/// top of the black screens until the blackout ends.
static HELD: AtomicBool = AtomicBool::new(false);

/// Keeps the panel shown and above everything (or lets it go again).
pub fn hold_open(app: &AppHandle, on: bool) {
    HELD.store(on, Ordering::SeqCst);
    if let Some(win) = window(app) {
        let _ = win.set_always_on_top(on);
    }
}

fn shortcut_text() -> String {
    let s = SHORTCUT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if s.is_empty() { DEFAULT_SHORTCUT.to_string() } else { s }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Loads saved settings, then the one-time native setup: acrylic backdrop,
/// rounded corners, no Alt+Tab entry. Returns the toggle shortcut to register.
pub fn init(app: &AppHandle) -> String {
    let saved = settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .unwrap_or_default();
    if let Some(w) = saved["width"].as_u64() {
        WIDTH.store((w as u32).clamp(MIN_WIDTH, MAX_WIDTH), Ordering::SeqCst);
    }
    RIGHT.store(saved["edge"] == "right", Ordering::SeqCst);
    if let Some(s) = saved["shortcut"].as_str() {
        *SHORTCUT.lock().unwrap_or_else(|e| e.into_inner()) = s.to_string();
    }
    crate::taskbar::set_enabled(saved["taskbarButton"].as_bool().unwrap_or(true));
    crate::taskbar::set_player_enabled(saved["taskbarPlayer"].as_bool().unwrap_or(true));
    crate::taskbar::set_mail_enabled(saved["taskbarMail"].as_bool().unwrap_or(true));
    crate::taskbar::set_tasks_enabled(saved["taskbarTasks"].as_bool().unwrap_or(true));
    crate::taskbar::set_agents_enabled(saved["taskbarAgents"].as_bool().unwrap_or(true));
    crate::taskbar::set_mic_enabled(saved["taskbarMic"].as_bool().unwrap_or(true));
    crate::taskbar::set_pomodoro_enabled(saved["taskbarPomodoro"].as_bool().unwrap_or(true));
    let shortcut = shortcut_text();

    let Some(win) = window(app) else { return shortcut };

    #[cfg(windows)]
    {
        if let Err(e) = window_vibrancy::apply_acrylic(&win, Some((18, 18, 22, 150))) {
            eprintln!("acrylic unavailable: {e}");
        }
        if let Ok(hwnd) = win.hwnd() {
            native::style(hwnd.0 as _);
        }
    }

    let handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            if !KEEP_OPEN.load(Ordering::SeqCst) && !PINNED.load(Ordering::SeqCst) && !HELD.load(Ordering::SeqCst) {
                request_hide(&handle);
            }
        }
    });
    shortcut
}

pub fn toggle(app: &AppHandle) {
    if VISIBLE.load(Ordering::SeqCst) && !HIDING.load(Ordering::SeqCst) {
        request_hide(app);
    } else {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    if now_ms().saturating_sub(LAST_HIDE_MS.load(Ordering::SeqCst)) < RESHOW_DEBOUNCE_MS {
        return;
    }
    let Some(win) = window(app) else { return };

    place_on_cursor_monitor(app, &win);
    HIDING.store(false, Ordering::SeqCst);
    VISIBLE.store(true, Ordering::SeqCst);
    let _ = win.show();
    let _ = win.set_focus();
    let _ = app.emit_to(LABEL, "panel:show", ());
}

/// Widens the window by `extra` logical px for now (0 goes back to the user's
/// width), as far as the screen allows; returns the extra actually applied.
/// With `animate` the width eases there over `RESIZE_ANIMATION` and the call
/// returns once it has; a newer call takes over a running animation.
#[tauri::command]
pub async fn panel_set_extra_width(app: AppHandle, extra: u32, animate: Option<bool>) -> u32 {
    if FULL.load(Ordering::SeqCst) {
        return 0;
    }
    let Some(win) = window(&app) else { return 0 };
    let room = win
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let scale = m.scale_factor();
            let screen = (m.work_area().size.width as f64 / scale) as u32;
            screen.saturating_sub(2 * MARGIN as u32 + WIDTH.load(Ordering::SeqCst))
        })
        .unwrap_or(0);
    let target = extra.min(room);
    let generation = RESIZE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let from = EXTRA.load(Ordering::SeqCst);
    if !animate.unwrap_or(false) || from == target || !VISIBLE.load(Ordering::SeqCst) {
        let _frame = RESIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if RESIZE_GENERATION.load(Ordering::SeqCst) != generation {
            return EXTRA.load(Ordering::SeqCst);
        }
        EXTRA.store(target, Ordering::SeqCst);
        apply_width(&win);
        return target;
    }

    let _ = tauri::async_runtime::spawn_blocking(move || {
        let start = std::time::Instant::now();
        loop {
            let t = (start.elapsed().as_secs_f64() / RESIZE_ANIMATION.as_secs_f64()).min(1.0);
            // Ease-out cubic: quick start, soft landing.
            let eased = 1.0 - (1.0 - t).powi(3);
            let now = from as f64 + (target as f64 - from as f64) * eased;
            {
                // Check and store together, so a hide can't slip in between
                // and have its reset overwritten by a stale frame.
                let _frame = RESIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                if RESIZE_GENERATION.load(Ordering::SeqCst) != generation {
                    return; // a newer resize took over
                }
                EXTRA.store(now.round() as u32, Ordering::SeqCst);
            }
            apply_width(&win);
            if t >= 1.0 {
                return;
            }
            std::thread::sleep(RESIZE_FRAME);
        }
    })
    .await;
    target
}
/// Pins the panel open (or unpins it); returns the new state.
#[tauri::command]
pub fn panel_set_pinned(on: bool) -> bool {
    PINNED.store(on, Ordering::SeqCst);
    on
}

/// Shows the panel (if hidden) on the given tab, e.g. from a taskbar or toast click.
pub fn show_tab(app: &AppHandle, tab: &str) {
    if !VISIBLE.load(Ordering::SeqCst) || HIDING.load(Ordering::SeqCst) {
        show(app);
    }
    let _ = app.emit_to(LABEL, "panel:tab", tab);
}

/// Opens the panel on a tab, and a note in it if given: clicks in the widgets
/// on the desktop, which live in windows of their own.
#[tauri::command]
pub fn panel_open(app: AppHandle, tab: String, note: Option<String>) {
    show_tab(&app, &tab);
    if let Some(note) = note {
        let _ = app.emit_to(LABEL, "panel:note", note);
    }
}

/// Runs `f` (a modal dialog) with hide-on-blur off, then gives focus back to the panel.
pub fn keep_open_while<R>(app: &AppHandle, f: impl FnOnce() -> R) -> R {
    KEEP_OPEN.store(true, Ordering::SeqCst);
    let result = f();
    KEEP_OPEN.store(false, Ordering::SeqCst);
    if let Some(win) = window(app) {
        let _ = win.set_focus();
    }
    result
}

/// Raw handle of the panel window, to own modal dialogs.
pub fn hwnd(app: &AppHandle) -> Option<isize> {
    #[cfg(windows)]
    {
        window(app)?.hwnd().ok().map(|h| h.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Ask the frontend to animate out; it calls `hide_panel` when done.
pub fn request_hide(app: &AppHandle) {
    if !VISIBLE.load(Ordering::SeqCst) || HIDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = app.emit_to(LABEL, "panel:hide", ());

    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HIDE_WATCHDOG);
        if HIDING.load(Ordering::SeqCst) {
            finish_hide(&handle);
        }
    });
}

pub fn finish_hide(app: &AppHandle) {
    // Flags first: hiding blurs the window, and that blur must see "not visible".
    VISIBLE.store(false, Ordering::SeqCst);
    HIDING.store(false, Ordering::SeqCst);
    LAST_HIDE_MS.store(now_ms(), Ordering::SeqCst);
    // The next show starts at the user's own width; a running resize stops.
    {
        let _frame = RESIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        RESIZE_GENERATION.fetch_add(1, Ordering::SeqCst);
        EXTRA.store(0, Ordering::SeqCst);
    }
    if let Some(win) = window(app) {
        let _ = win.hide();
    }
}

/// Dock the panel to the left edge of whichever monitor holds the cursor.
fn place_on_cursor_monitor(app: &AppHandle, win: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let margin = (MARGIN * scale).round() as i32;
    let full_width = area.size.width.saturating_sub(2 * margin as u32);
    let width = if FULL.load(Ordering::SeqCst) {
        full_width
    } else {
        ((window_width() as f64 * scale).round() as u32).min(full_width)
    };
    let height = area.size.height.saturating_sub(2 * margin as u32);

    let _ = win.set_size(PhysicalSize::new(width, height));
    let x = if RIGHT.load(Ordering::SeqCst) {
        area.position.x + area.size.width as i32 - width as i32 - margin
    } else {
        area.position.x + margin
    };
    let _ = win.set_position(PhysicalPosition::new(x, area.position.y + margin));
    // Tauri moved it onto the right monitor (and its DPI); now line the visible
    // edge up with the margins, which the invisible resize borders would skew.
    #[cfg(windows)]
    if let Ok(hwnd) = win.hwnd() {
        native::place_visible(hwnd.0 as _, x, area.position.y + margin, width as i32, height as i32);
    }
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    finish_hide(&app);
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(SETTINGS_FILE))
}

#[derive(Serialize)]
pub struct PanelSettings {
    width: u32,
    edge: &'static str,
    shortcut: String,
    /// Show the Dock Panel button on the taskbar.
    #[serde(rename = "taskbarButton")]
    taskbar_button: bool,
    /// Show the mini player next to that button.
    #[serde(rename = "taskbarPlayer")]
    taskbar_player: bool,
    /// Show the unread-mail counter next to that button.
    #[serde(rename = "taskbarMail")]
    taskbar_mail: bool,
    /// Show the count of Google tasks due today next to that button.
    #[serde(rename = "taskbarTasks")]
    taskbar_tasks: bool,
    /// Show the count of Claude / Codex sessions waiting for the user.
    #[serde(rename = "taskbarAgents")]
    taskbar_agents: bool,
    /// Show the Discord microphone while the user is in a voice channel.
    #[serde(rename = "taskbarMic")]
    taskbar_mic: bool,
    /// Show the pomodoro countdown while a phase is under way.
    #[serde(rename = "taskbarPomodoro")]
    taskbar_pomodoro: bool,
}

fn current_settings() -> PanelSettings {
    PanelSettings {
        width: WIDTH.load(Ordering::SeqCst),
        edge: if RIGHT.load(Ordering::SeqCst) { "right" } else { "left" },
        shortcut: shortcut_text(),
        taskbar_button: crate::taskbar::enabled(),
        taskbar_player: crate::taskbar::player_enabled(),
        taskbar_mail: crate::taskbar::mail_enabled(),
        taskbar_tasks: crate::taskbar::tasks_enabled(),
        taskbar_agents: crate::taskbar::agents_enabled(),
        taskbar_mic: crate::taskbar::mic_enabled(),
        taskbar_pomodoro: crate::taskbar::pomodoro_enabled(),
    }
}

fn save_settings(app: &AppHandle) {
    if let (Some(path), Ok(text)) = (settings_path(app), serde_json::to_string(&current_settings())) {
        let _ = std::fs::write(path, text);
    }
}

#[tauri::command]
pub fn panel_settings() -> PanelSettings {
    current_settings()
}

#[tauri::command]
pub fn panel_set_edge(app: AppHandle, edge: String) -> PanelSettings {
    RIGHT.store(edge == "right", Ordering::SeqCst);
    save_settings(&app);
    if let Some(win) = window(&app) {
        place_on_cursor_monitor(&app, &win);
    }
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_button(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_enabled(on);
    save_settings(&app);
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_player(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_player_enabled(on);
    save_settings(&app);
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_mail(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_mail_enabled(on);
    save_settings(&app);
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_pomodoro(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_pomodoro_enabled(on);
    save_settings(&app);
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_mic(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_mic_enabled(on);
    save_settings(&app);
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_agents(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_agents_enabled(on);
    save_settings(&app);
    current_settings()
}

#[tauri::command]
pub fn panel_set_taskbar_tasks(app: AppHandle, on: bool) -> PanelSettings {
    crate::taskbar::set_tasks_enabled(on);
    save_settings(&app);
    current_settings()
}

/// Swaps the global toggle shortcut; the old one stays if the new one is taken.
#[tauri::command]
pub fn panel_set_shortcut(app: AppHandle, shortcut: String) -> Result<PanelSettings, String> {
    let new: Shortcut = shortcut.parse().map_err(|e| format!("Не получилось разобрать сочетание: {e}"))?;
    let gs = app.global_shortcut();
    let old = shortcut_text();
    if let Ok(old) = old.parse::<Shortcut>() {
        let _ = gs.unregister(old);
    }
    if let Err(e) = gs.register(new) {
        if let Ok(old) = old.parse::<Shortcut>() {
            let _ = gs.register(old);
        }
        return Err(format!("Сочетание занято другой программой ({e})"));
    }
    *SHORTCUT.lock().unwrap_or_else(|e| e.into_inner()) = shortcut.clone();
    save_settings(&app);
    crate::tray::set_shortcut_label(&shortcut);
    Ok(current_settings())
}

/// Lets the settings UI record a new combination without the current one
/// toggling the panel mid-recording.
#[tauri::command]
pub fn panel_suspend_shortcut(app: AppHandle, suspend: bool) {
    if let Ok(sc) = shortcut_text().parse::<Shortcut>() {
        let gs = app.global_shortcut();
        let _ = if suspend { gs.unregister(sc) } else { gs.register(sc) };
    }
}

/// Resizes the panel live (keeping its left edge); `persist` saves the choice,
/// which drag handles pass only on release.
#[tauri::command]
pub fn panel_set_width(app: AppHandle, width: u32, persist: bool) -> u32 {
    let width = width.clamp(MIN_WIDTH, MAX_WIDTH);
    WIDTH.store(width, Ordering::SeqCst);
    if let Some(win) = window(&app) {
        apply_width(&win);
    }
    if persist {
        save_settings(&app);
    }
    width
}

#[cfg(windows)]
pub(crate) mod native {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOZORDER,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    pub fn style(raw: *mut core::ffi::c_void) {
        let hwnd = HWND(raw);
        unsafe {
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as _,
                std::mem::size_of_val(&pref) as u32,
            );
            // Tool windows stay out of Alt+Tab and the taskbar.
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let ex = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        }
    }

    /// The invisible resize borders around the window (left, top, right,
    /// bottom), learnt from DWM whenever it can tell and remembered, since a
    /// hidden window may not report them.
    static FRAME: std::sync::Mutex<Option<(i32, i32, i32, i32)>> = std::sync::Mutex::new(None);

    fn frame(hwnd: HWND) -> (i32, i32, i32, i32) {
        let mut cached = FRAME.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let (mut outer, mut visible) = (RECT::default(), RECT::default());
            let known = GetWindowRect(hwnd, &mut outer).is_ok()
                && DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    &mut visible as *mut _ as _,
                    std::mem::size_of::<RECT>() as u32,
                )
                .is_ok()
                && visible.right > visible.left;
            if known {
                let borders = (
                    visible.left - outer.left,
                    visible.top - outer.top,
                    outer.right - visible.right,
                    outer.bottom - visible.bottom,
                );
                // Sane borders only: a minimised or mid-move window reports nonsense.
                if [borders.0, borders.1, borders.2, borders.3].iter().all(|b| (0..=32).contains(b)) {
                    *cached = Some(borders);
                }
            }
        }
        cached.unwrap_or((0, 0, 0, 0))
    }

    /// Places the window so its *visible* edge is the given rectangle
    /// (physical pixels): Windows 10/11 windows carry invisible resize borders
    /// around it, which otherwise shift it inwards on one side and past the
    /// screen edge on the other.
    pub fn place_visible(raw: *mut core::ffi::c_void, x: i32, y: i32, w: i32, h: i32) {
        let hwnd = HWND(raw);
        let (l, t, r, b) = frame(hwnd);
        unsafe {
            let _ = SetWindowPos(hwnd, None, x - l, y - t, w + l + r, h + t + b, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

#[cfg(test)]
mod tests {
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn parses_shortcuts_from_the_settings_ui() {
        for s in ["Ctrl+Space", "Ctrl+Alt+A", "Alt+Shift+1", "Super+Space", "Ctrl+Backquote", "F9", "Ctrl+Shift+F12"] {
            assert!(s.parse::<Shortcut>().is_ok(), "{s} should parse");
        }
    }
}
