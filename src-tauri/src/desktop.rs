//! Widgets on the desktop: each is a small window of its own showing one home
//! widget (the same page as the panel; the window label says which widget),
//! kept at the bottom of the z-order, right above the wallpaper.
//!
//! Widgets stand on an invisible grid, as on the macOS desktop: columns as
//! wide as a widget counted from the right edge of the work area, rows of
//! `STEP`. A dropped widget glides to the nearest free spot; one that grows
//! pushes the widgets below it down.
//!
//! While a widget is dragged it rises above all windows, and a see-through
//! overlay (`GRID_LABEL`) right below it shows the grid of its monitor and the
//! spot it will land on. The overlay is a WebView of its own, so it exists
//! only around a drag: made when the mouse goes down on a widget (it is ready
//! by the time the hold turns into a drag) and closed after the drop.
//!
//! The page sizes the window to its content (`desktop_fit`), moves it by
//! press-and-hold (`desktop_drag`) and asks for its menu on right click.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const FILE: &str = "desktop.json";
/// Window labels are this plus the widget id; the page reads its widget from it.
const PREFIX: &str = "desk-";
const EVENT: &str = "desktop:changed";
/// The grid overlay's window; not `PREFIX`-ed, it shows no widget.
const GRID_LABEL: &str = "deskgrid";
/// The overlay fades out for this long before it closes.
#[cfg_attr(not(windows), allow(dead_code))]
const GRID_FADE: Duration = Duration::from_millis(180);
/// A press that has not become a drag by then was a click: the overlay goes.
const GRID_UNUSED: Duration = Duration::from_millis(1500);
/// Logical width a widget window starts at, until the page sends the user's
/// widget width (`desktop_fit`), and the range it may ask for.
const WIDTH: f64 = 500.0;
const MIN_WIDTH: f64 = 300.0;
const MAX_WIDTH: f64 = 700.0;
const MAX_HEIGHT: f64 = 900.0;
/// Grid, logical px: margin from the work-area edges, gap between widgets,
/// and the row step a widget's top snaps to.
const INSET: f64 = 24.0;
const GAP: f64 = 16.0;
const STEP: f64 = 16.0;
/// The glide to the grid after a drop.
const GLIDE: Duration = Duration::from_millis(160);
const GLIDE_FRAME: Duration = Duration::from_millis(8);
/// Moves are saved this long after the last one, not on every step of a drag.
const SAVE_DELAY: Duration = Duration::from_millis(600);

/// A widget on the desktop and its window's outer position, physical px.
#[derive(Clone, Serialize, Deserialize)]
struct Placed {
    id: String,
    x: i32,
    y: i32,
}

static PLACED: Mutex<Vec<Placed>> = Mutex::new(Vec::new());
static SAVE_PENDING: AtomicBool = AtomicBool::new(false);
/// Held while a widget picks its spot, so two can't pick the same one.
static LAYOUT: Mutex<()> = Mutex::new(());
/// Native handles of the widget windows by id, and of the grid overlay: read
/// during a drag, when the window getters (a round trip to the main thread,
/// which is busy moving the window) would hang.
static HWNDS: Mutex<Vec<(String, isize)>> = Mutex::new(Vec::new());
static GRID_HWND: AtomicIsize = AtomicIsize::new(0);

fn placed() -> std::sync::MutexGuard<'static, Vec<Placed>> {
    PLACED.lock().unwrap_or_else(|e| e.into_inner())
}

fn ids() -> Vec<String> {
    placed().iter().map(|p| p.id.clone()).collect()
}

fn path(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(FILE))
}

fn save(app: &AppHandle) {
    let text = serde_json::to_string(&*placed());
    if let (Some(path), Ok(text)) = (path(app), text) {
        let _ = std::fs::write(path, text);
    }
}

/// Opens the windows of the widgets left on the desktop last time; each
/// settles onto the grid once it has measured itself.
pub fn init(app: &AppHandle) {
    let saved: Vec<Placed> = path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    *placed() = saved.clone();
    for p in saved {
        if let Err(e) = open(app, &p) {
            eprintln!("desktop widget {}: {e}", p.id);
        }
    }

    app.on_menu_event(|app, event| {
        let id = event.id.as_ref();
        if id == "desk-open" {
            crate::panel::show(app);
        } else if let Some(widget) = id.strip_prefix("desk-remove:") {
            remove(app, widget);
        }
    });

    #[cfg(windows)]
    {
        let lift = app.clone();
        let handle = app.clone();
        native::on_drag(
            move |hwnd| show_grid(&lift, hwnd, true),
            move |hwnd| {
                hide_grid(&handle);
                let handle = handle.clone();
                // Off the window procedure: snapping moves windows.
                std::thread::spawn(move || {
                    if let Some(win) = windows(&handle)
                        .into_iter()
                        .find(|w| w.hwnd().is_ok_and(|h| h.0 as isize == hwnd))
                    {
                        snap(&handle, &win, true);
                    }
                });
            },
        );
    }
}

fn open(app: &AppHandle, p: &Placed) -> tauri::Result<()> {
    let label = format!("{PREFIX}{}", p.id);
    if app.get_webview_window(&label).is_some() {
        return Ok(());
    }
    // Hidden until the page has measured itself (`desktop_fit`).
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("Dock Panel")
        .inner_size(WIDTH, 120.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .build()?;
    let _ = win.set_position(PhysicalPosition::new(p.x, p.y));

    #[cfg(windows)]
    {
        if let Err(e) = window_vibrancy::apply_acrylic(&win, Some((18, 18, 22, 150))) {
            eprintln!("acrylic unavailable: {e}");
        }
        if let Ok(hwnd) = win.hwnd() {
            // Rounded corners and no taskbar entry, as the panel.
            crate::panel::native::style(hwnd.0 as _);
            native::pin_to_desktop(hwnd.0 as _);
            hwnds().push((p.id.clone(), hwnd.0 as isize));
        }
    }

    let (handle, id) = (app.clone(), p.id.clone());
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(pos) = event {
            if let Some(p) = placed().iter_mut().find(|p| p.id == id) {
                (p.x, p.y) = (pos.x, pos.y);
            }
            save_soon(&handle);
            #[cfg(windows)]
            if let Some(hwnd) = hwnd_of(&id).filter(|&h| native::dragging() == h) {
                show_grid(&handle, hwnd, false);
            }
        }
    });
    Ok(())
}

fn hwnds() -> std::sync::MutexGuard<'static, Vec<(String, isize)>> {
    HWNDS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg_attr(not(windows), allow(dead_code))]
fn hwnd_of(id: &str) -> Option<isize> {
    hwnds().iter().find(|(i, _)| i == id).map(|&(_, h)| h)
}

/// The see-through, click-through window the grid is drawn in while a widget
/// is dragged.
fn open_grid(app: &AppHandle) {
    if app.get_webview_window(GRID_LABEL).is_some() {
        return;
    }
    let built = WebviewWindowBuilder::new(app, GRID_LABEL, WebviewUrl::App("index.html".into()))
        .title("Dock Panel")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .focused(false)
        .build();
    let win = match built {
        Ok(win) => win,
        Err(e) => return eprintln!("desktop grid: {e}"),
    };
    let _ = win.set_ignore_cursor_events(true);
    #[cfg(windows)]
    if let Ok(hwnd) = win.hwnd() {
        native::tool_window(hwnd.0 as _);
        GRID_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    }
}

/// What the overlay draws, physical px from the top-left of the work area.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
struct GridView {
    /// The grid's area inside the margins.
    area: Rect,
    /// Left edges of the columns.
    columns: Vec<i32>,
    width: i32,
    step: i32,
    /// Where the dragged widget will land.
    target: Option<Rect>,
}

/// Shows (`first`) or updates the grid under the widget being dragged. Runs
/// on the main thread mid-drag, so it reads windows natively.
#[cfg(windows)]
fn show_grid(app: &AppHandle, hwnd: isize, first: bool) {
    let Some(me) = native::rect_of(hwnd) else {
        return;
    };
    let Some((work, scale)) = native::work_area_at(me.x + me.w / 2, me.y + me.h / 2) else {
        return;
    };
    let grid = Grid::new(work, scale);
    let others: Vec<Rect> = hwnds()
        .iter()
        .filter(|&&(_, h)| h != hwnd)
        .filter_map(|&(_, h)| native::rect_of(h))
        .collect();
    let target = grid.place((me.w, me.h), (me.x, me.y), &others);
    let rel = |r: Rect| Rect {
        x: r.x - work.x,
        y: r.y - work.y,
        ..r
    };
    let view = GridView {
        area: rel(grid.area),
        columns: grid.columns(me.w).into_iter().map(|x| x - work.x).collect(),
        width: me.w,
        step: grid.step,
        target: target.map(|(x, y)| {
            rel(Rect {
                x,
                y,
                w: me.w,
                h: me.h,
            })
        }),
    };
    // Over the work area of the widget's monitor (it may have crossed to
    // another one), right below the widget.
    let overlay = GRID_HWND.load(Ordering::SeqCst);
    if overlay != 0 {
        native::place_below(overlay, hwnd, work);
    }
    let _ = app.emit_to(
        GRID_LABEL,
        if first { "grid:show" } else { "grid:update" },
        view,
    );
}

/// Closes the overlay, unless a widget is being dragged over it.
fn close_grid(app: &AppHandle) {
    #[cfg(windows)]
    if native::dragging() != 0 {
        return;
    }
    GRID_HWND.store(0, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window(GRID_LABEL) {
        let _ = win.destroy();
    }
}

/// Fades the overlay out after a drop, then closes it.
#[cfg(windows)]
fn hide_grid(app: &AppHandle) {
    let _ = app.emit_to(GRID_LABEL, "grid:hide", ());
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(GRID_FADE);
        close_grid(&app);
    });
}

/// The mouse went down on a widget: make the overlay now, so it is ready if
/// the press turns into a drag, and close it again if it doesn't.
#[tauri::command]
pub async fn desktop_grid_prepare(app: AppHandle) {
    open_grid(&app);
    std::thread::spawn(move || {
        std::thread::sleep(GRID_UNUSED);
        close_grid(&app);
    });
}

fn save_soon(app: &AppHandle) {
    if SAVE_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SAVE_DELAY);
        SAVE_PENDING.store(false, Ordering::SeqCst);
        save(&app);
    });
}

fn remove(app: &AppHandle, id: &str) {
    placed().retain(|p| p.id != id);
    hwnds().retain(|(i, _)| i != id);
    save(app);
    if let Some(win) = app.get_webview_window(&format!("{PREFIX}{id}")) {
        let _ = win.destroy();
    }
    let _ = app.emit(EVENT, ids());
}

/// The open widget windows.
fn windows(app: &AppHandle) -> Vec<WebviewWindow> {
    ids()
        .iter()
        .filter_map(|id| app.get_webview_window(&format!("{PREFIX}{id}")))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    fn right(&self) -> i32 {
        self.x + self.w
    }
    fn bottom(&self) -> i32 {
        self.y + self.h
    }
    fn overlaps_x(&self, other: &Rect) -> bool {
        self.x < other.right() && other.x < self.right()
    }
}

fn rect(win: &WebviewWindow) -> Option<Rect> {
    let (pos, size) = (win.outer_position().ok()?, win.outer_size().ok()?);
    Some(Rect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    })
}

/// The grid of one monitor, physical px.
#[derive(Clone, Copy, Debug)]
struct Grid {
    /// Work area inside the margins.
    area: Rect,
    gap: i32,
    step: i32,
}

impl Grid {
    /// The grid of a monitor's work area at its scale factor.
    fn new(work: Rect, scale: f64) -> Grid {
        let px = |v: f64| (v * scale).round() as i32;
        Grid {
            area: Rect {
                x: work.x + px(INSET),
                y: work.y + px(INSET),
                w: work.w - 2 * px(INSET),
                h: work.h - 2 * px(INSET),
            },
            gap: px(GAP),
            step: px(STEP),
        }
    }

    fn for_point(app: &AppHandle, x: i32, y: i32) -> Option<Grid> {
        let m = app
            .monitor_from_point(x as f64, y as f64)
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())?;
        let work = m.work_area();
        let work = Rect {
            x: work.position.x,
            y: work.position.y,
            w: work.size.width as i32,
            h: work.size.height as i32,
        };
        Some(Grid::new(work, m.scale_factor()))
    }

    /// Left edges of the columns for a widget `w` wide, rightmost first.
    fn columns(&self, w: i32) -> Vec<i32> {
        let count = ((self.area.w + self.gap) / (w + self.gap)).max(1);
        (0..count)
            .map(|c| self.area.right() - w - c * (w + self.gap))
            .collect()
    }

    /// Where a widget of `size` dropped at `want` goes: the nearest spot in a
    /// column, its top on a row or just below / above another widget, clear
    /// of `others` by a gap. None if the monitor is full.
    fn place(&self, (w, h): (i32, i32), want: (i32, i32), others: &[Rect]) -> Option<(i32, i32)> {
        let top = self.area.y;
        let lowest = (self.area.bottom() - h).max(top);
        let mut best: Option<((i32, i32), i64)> = None;
        for x in self.columns(w) {
            let me = Rect { x, y: 0, w, h };
            let column: Vec<&Rect> = others.iter().filter(|o| o.overlaps_x(&me)).collect();
            let rows = (0..=(lowest - top) / self.step.max(1)).map(|k| top + k * self.step);
            let beside = column
                .iter()
                .flat_map(|o| [o.bottom() + self.gap, o.y - self.gap - h]);
            for y in rows.chain(beside) {
                if y < top || y > lowest {
                    continue;
                }
                let free = column
                    .iter()
                    .all(|o| y >= o.bottom() + self.gap || y + h + self.gap <= o.y);
                if !free {
                    continue;
                }
                let (dx, dy) = ((x - want.0) as i64, (y - want.1) as i64);
                let d = dx * dx + dy * dy;
                if best.is_none_or(|(_, b)| d < b) {
                    best = Some(((x, y), d));
                }
            }
        }
        best.map(|(p, _)| p)
    }
}

/// Moves a widget onto the nearest free spot of the grid (gliding with
/// `animate`), then pushes aside whatever its new size now overlaps.
fn snap(app: &AppHandle, win: &WebviewWindow, animate: bool) {
    let _layout = LAYOUT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(me) = rect(win) else { return };
    let Some(grid) = Grid::for_point(app, me.x + me.w / 2, me.y + me.h / 2) else {
        return;
    };
    let others: Vec<Rect> = windows(app)
        .iter()
        .filter(|w| w.label() != win.label())
        .filter_map(rect)
        .collect();
    let Some((x, y)) = grid.place((me.w, me.h), (me.x, me.y), &others) else {
        return;
    };
    if animate {
        glide(win, (me.x, me.y), (x, y));
    } else {
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
    push_down(app, win);
}

/// Widgets below this one in its column move down until they clear it (and
/// each other) by a gap.
fn push_down(app: &AppHandle, win: &WebviewWindow) {
    let Some(me) = rect(win) else { return };
    let Some(grid) = Grid::for_point(app, me.x + me.w / 2, me.y + me.h / 2) else {
        return;
    };
    let mut below: Vec<(WebviewWindow, Rect)> = windows(app)
        .into_iter()
        .filter(|w| w.label() != win.label())
        .filter_map(|w| rect(&w).map(|r| (w, r)))
        .filter(|(_, r)| r.overlaps_x(&me) && r.y >= me.y)
        .collect();
    below.sort_by_key(|(_, r)| r.y);
    let mut floor = me.bottom() + grid.gap;
    for (w, r) in below {
        if r.y < floor {
            let _ = w.set_position(PhysicalPosition::new(r.x, floor));
            floor += r.h + grid.gap;
        } else {
            floor = r.bottom() + grid.gap;
        }
    }
}

fn glide(win: &WebviewWindow, from: (i32, i32), to: (i32, i32)) {
    let start = Instant::now();
    loop {
        let t = (start.elapsed().as_secs_f64() / GLIDE.as_secs_f64()).min(1.0);
        // Ease-out cubic, as the panel's own resize.
        let eased = 1.0 - (1.0 - t).powi(3);
        let at = |a: i32, b: i32| (a as f64 + (b - a) as f64 * eased).round() as i32;
        let _ = win.set_position(PhysicalPosition::new(at(from.0, to.0), at(from.1, to.1)));
        if t >= 1.0 {
            return;
        }
        std::thread::sleep(GLIDE_FRAME);
    }
}

/// Ids of the widgets on the desktop.
#[tauri::command]
pub fn desktop_widgets() -> Vec<String> {
    ids()
}

/// Puts a widget on the desktop or takes it off; returns the new list. A new
/// one starts at the top right and settles into the first free spot from there.
/// Async: creating a window from a sync command deadlocks on Windows.
#[tauri::command]
pub async fn desktop_set(app: AppHandle, id: String, on: bool) -> Result<Vec<String>, String> {
    if !on {
        remove(&app, &id);
        return Ok(ids());
    }
    if placed().iter().any(|p| p.id == id) {
        return Ok(ids());
    }
    let (x, y) = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| Grid::for_point(&app, m.position().x, m.position().y))
        .map(|g| (g.area.right(), g.area.y))
        .unwrap_or((100, 100));
    let p = Placed { id, x, y };
    placed().push(p.clone());
    save(&app);
    open(&app, &p).map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT, ids());
    Ok(ids())
}

/// Sizes the calling widget window: the user's widget width, its content's
/// height. The first time it also takes its spot on the grid and shows; later
/// a grown widget pushes the ones below it down, and a new width (the grid's
/// columns moved with it) sends it to its spot again.
#[tauri::command]
pub async fn desktop_fit(app: AppHandle, window: WebviewWindow, width: f64, height: f64) {
    let old_width = window.outer_size().map(|s| s.width).unwrap_or(0);
    let _ = window.set_size(LogicalSize::new(
        width.clamp(MIN_WIDTH, MAX_WIDTH),
        height.clamp(1.0, MAX_HEIGHT),
    ));
    if window.is_visible().unwrap_or(true) {
        if window.outer_size().map(|s| s.width).unwrap_or(0) != old_width {
            snap(&app, &window, true);
        } else {
            push_down(&app, &window);
        }
        return;
    }
    snap(&app, &window, false);
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        native::show_quietly(hwnd.0 as _);
        return;
    }
    let _ = window.show();
}

/// Blurs the wallpaper under the calling widget window, or lets it show clear.
#[tauri::command]
pub fn desktop_backdrop(window: WebviewWindow, blur: bool) {
    #[cfg(windows)]
    {
        let result = if blur {
            window_vibrancy::apply_acrylic(&window, Some((18, 18, 22, 150)))
        } else {
            window_vibrancy::clear_acrylic(&window)
        };
        if let Err(e) = result {
            eprintln!("desktop backdrop: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = (window, blur);
}

/// Starts moving the calling window with the mouse (the button is still
/// down); the drop snaps it to the grid.
#[tauri::command]
pub fn desktop_drag(window: WebviewWindow) {
    let _ = window.start_dragging();
}

/// The right-click menu of a widget window.
#[tauri::command]
pub fn desktop_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let Some(id) = window.label().strip_prefix(PREFIX) else {
        return Ok(());
    };
    let menu = (|| {
        let open = MenuItem::with_id(&app, "desk-open", "Открыть панель", true, None::<&str>)?;
        let remove = MenuItem::with_id(
            &app,
            format!("desk-remove:{id}"),
            "Убрать с рабочего стола",
            true,
            None::<&str>,
        )?;
        Menu::with_items(&app, &[&open, &remove])
    })()
    .map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

#[cfg(windows)]
mod native {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::OnceLock;

    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongPtrW, GetWindowRect, SendMessageW, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, GWLP_HWNDPARENT, GWL_EXSTYLE, HWND_BOTTOM, HWND_NOTOPMOST,
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SW_SHOWNOACTIVATE, WINDOWPOS, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCACTIVATE,
        WM_WINDOWPOSCHANGING, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    use super::Rect;

    type DragHandler = Box<dyn Fn(isize) + Send + Sync>;
    static ON_LIFT: OnceLock<DragHandler> = OnceLock::new();
    static ON_DROP: OnceLock<DragHandler> = OnceLock::new();
    /// The widget window being dragged, 0 for none.
    static DRAGGING: AtomicIsize = AtomicIsize::new(0);

    /// Called with the window's handle when a drag of it starts and when it ends.
    pub fn on_drag(
        lift: impl Fn(isize) + Send + Sync + 'static,
        drop: impl Fn(isize) + Send + Sync + 'static,
    ) {
        let _ = ON_LIFT.set(Box::new(lift));
        let _ = ON_DROP.set(Box::new(drop));
    }

    pub fn dragging() -> isize {
        DRAGGING.load(Ordering::SeqCst)
    }

    /// Owned by the desktop (so "Show desktop" leaves it be) and held at the
    /// bottom of the z-order, even when clicked.
    pub fn pin_to_desktop(raw: *mut core::ffi::c_void) {
        let hwnd = HWND(raw);
        unsafe {
            if let Ok(desktop) = FindWindowW(w!("Progman"), None) {
                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, desktop.0 as isize);
            }
            let _ = SetWindowSubclass(hwnd, Some(subclass), 1, 0);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    unsafe extern "system" fn subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            // Held at the bottom, except while dragged: then it is lifted
            // above everything (see WM_ENTERSIZEMOVE).
            WM_WINDOWPOSCHANGING if DRAGGING.load(Ordering::SeqCst) != hwnd.0 as isize => {
                let pos = &mut *(lparam.0 as *mut WINDOWPOS);
                pos.hwndInsertAfter = HWND_BOTTOM;
                pos.flags &= !SWP_NOZORDER;
            }
            // Windows 11 draws the acrylic backdrop only while the window
            // looks active, and a desktop widget never is: always look active.
            WM_NCACTIVATE => return DefSubclassProc(hwnd, msg, WPARAM(1), lparam),
            WM_ENTERSIZEMOVE => {
                DRAGGING.store(hwnd.0 as isize, Ordering::SeqCst);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                if let Some(f) = ON_LIFT.get() {
                    f(hwnd.0 as isize);
                }
            }
            WM_EXITSIZEMOVE => {
                DRAGGING.store(0, Ordering::SeqCst);
                // Back down: the rule above turns this into HWND_BOTTOM, which drops topmost too.
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_NOTOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                if let Some(f) = ON_DROP.get() {
                    f(hwnd.0 as isize);
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    pub fn show_quietly(raw: *mut core::ffi::c_void) {
        unsafe {
            let _ = ShowWindow(HWND(raw), SW_SHOWNOACTIVATE);
            // Never activated, so tell it once that it is (see WM_NCACTIVATE above).
            SendMessageW(HWND(raw), WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(0)));
        }
    }

    /// Outer rectangle of a window, physical px.
    pub fn rect_of(hwnd: isize) -> Option<Rect> {
        let mut r = RECT::default();
        unsafe { GetWindowRect(HWND(hwnd as _), &mut r).ok()? };
        Some(Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        })
    }

    /// Work area and scale factor of the monitor nearest to a point.
    pub fn work_area_at(x: i32, y: i32) -> Option<(Rect, f64)> {
        unsafe {
            let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return None;
            }
            let (mut dpi, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
            let w = info.rcWork;
            Some((
                Rect {
                    x: w.left,
                    y: w.top,
                    w: w.right - w.left,
                    h: w.bottom - w.top,
                },
                dpi as f64 / 96.0,
            ))
        }
    }

    /// Shows the overlay over `area`, right below `above` in the z-order.
    pub fn place_below(overlay: isize, above: isize, area: Rect) {
        unsafe {
            let _ = SetWindowPos(
                HWND(overlay as _),
                Some(HWND(above as _)),
                area.x,
                area.y,
                area.w,
                area.h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    /// No taskbar or Alt+Tab entry, never takes focus.
    pub fn tool_window(raw: *mut core::ffi::c_void) {
        let hwnd = HWND(raw);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let ex = (ex | WS_EX_TOOLWINDOW.0 as isize | WS_EX_NOACTIVATE.0 as isize)
                & !(WS_EX_APPWINDOW.0 as isize);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Grid, Rect};

    /// 1000×800 work area inside the margins, 300-wide widgets: columns at 700 and 380.
    fn grid() -> Grid {
        Grid {
            area: Rect {
                x: 0,
                y: 0,
                w: 1000,
                h: 800,
            },
            gap: 20,
            step: 10,
        }
    }

    #[test]
    fn columns_count_from_the_right() {
        assert_eq!(grid().columns(300), vec![700, 380, 60]);
    }

    #[test]
    fn a_drop_lands_on_the_nearest_column_and_row() {
        assert_eq!(grid().place((300, 200), (400, 133), &[]), Some((380, 130)));
    }

    #[test]
    fn a_drop_stays_on_screen() {
        assert_eq!(grid().place((300, 200), (2000, 900), &[]), Some((700, 600)));
        assert_eq!(grid().place((300, 200), (-50, -50), &[]), Some((60, 0)));
    }

    #[test]
    fn a_drop_onto_another_widget_goes_beside_it() {
        let other = Rect {
            x: 700,
            y: 0,
            w: 300,
            h: 205,
        };
        // Just below it, off the row grid, rather than overlapping.
        assert_eq!(
            grid().place((300, 100), (700, 150), &[other]),
            Some((700, 225))
        );
    }

    #[test]
    fn a_full_column_sends_it_to_the_next() {
        let other = Rect {
            x: 700,
            y: 0,
            w: 300,
            h: 800,
        };
        assert_eq!(
            grid().place((300, 100), (700, 100), &[other]),
            Some((380, 100))
        );
    }
}
