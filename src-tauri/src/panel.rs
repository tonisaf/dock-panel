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
pub const DEFAULT_WIDTH: u32 = 440;
pub const MIN_WIDTH: u32 = 360;
pub const MAX_WIDTH: u32 = 1280;
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
/// Dock to the right screen edge instead of the left.
static RIGHT: AtomicBool = AtomicBool::new(false);
static SHORTCUT: Mutex<String> = Mutex::new(String::new());
/// Set while a modal dialog owned by the panel is open: its focus steal must not hide us.
static KEEP_OPEN: AtomicBool = AtomicBool::new(false);

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
            if !KEEP_OPEN.load(Ordering::SeqCst) {
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
    let width = (WIDTH.load(Ordering::SeqCst) as f64 * scale).round() as u32;
    let height = area.size.height.saturating_sub(2 * margin as u32);

    let _ = win.set_size(PhysicalSize::new(width, height));
    let x = if RIGHT.load(Ordering::SeqCst) {
        area.position.x + area.size.width as i32 - width as i32 - margin
    } else {
        area.position.x + margin
    };
    let _ = win.set_position(PhysicalPosition::new(x, area.position.y + margin));
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
}

fn current_settings() -> PanelSettings {
    PanelSettings {
        width: WIDTH.load(Ordering::SeqCst),
        edge: if RIGHT.load(Ordering::SeqCst) { "right" } else { "left" },
        shortcut: shortcut_text(),
        taskbar_button: crate::taskbar::enabled(),
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
        if let (Ok(scale), Ok(size), Ok(pos)) = (win.scale_factor(), win.outer_size(), win.outer_position()) {
            let new_w = (width as f64 * scale).round() as u32;
            let _ = win.set_size(PhysicalSize::new(new_w, size.height));
            // Docked right: grow leftwards so the right edge stays put.
            if RIGHT.load(Ordering::SeqCst) {
                let _ = win.set_position(PhysicalPosition::new(pos.x + size.width as i32 - new_w as i32, pos.y));
            }
        }
    }
    if persist {
        save_settings(&app);
    }
    width
}

#[cfg(windows)]
mod native {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
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
