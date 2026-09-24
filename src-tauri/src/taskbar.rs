//! A Dock Panel button at the left end of the Windows 11 taskbar.
//!
//! Windows has no API for putting things on the taskbar any more (desk bands
//! are gone), so this is a small layered popup *owned* by the taskbar window:
//! owned windows always stay above their owner, so the taskbar never covers
//! the button, and it follows the taskbar's z-order in fullscreen apps. A timer
//! keeps it on the taskbar's left end and hides it when the taskbar is hidden
//! (auto-hide) or aligned left, where the Start button lives.
//!
//! Everything runs on a dedicated thread with its own message loop. When
//! Explorer restarts, the taskbar is destroyed along with our owned window, and
//! the thread waits for the new taskbar and starts over.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;

static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Takes effect on the button's next timer tick.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
}

pub fn start(app: &AppHandle) {
    #[cfg(windows)]
    native::start(app);
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
mod native {
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    use std::time::Duration;

    use tauri::AppHandle;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, MonitorFromWindow,
        SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
        DIB_RGB_COLORS, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, GetWindowRect,
        IsWindowVisible, LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer, ShowWindow, TranslateMessage,
        UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_DESTROY,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_TIMER, WNDCLASSW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    use crate::panel;

    /// winuser.h; the windows crate only exports it with the Controls feature.
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const CLASS: PCWSTR = w!("DockPanelTaskbarButton");
    const TIMER_MS: u32 = 400;
    /// Sizes in DIPs, matching the Windows 11 taskbar buttons.
    const WIDTH: f64 = 44.0;
    const PLATE: f64 = 40.0;
    const RADIUS: f64 = 4.0;
    const ICON: f64 = 24.0;
    const LEFT_GAP: f64 = 6.0;

    static APP: OnceLock<AppHandle> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);

    /// Icon pixels, straight RGBA.
    struct Icon {
        rgba: Vec<u8>,
        width: usize,
        height: usize,
    }

    #[derive(Clone, Copy, PartialEq, Default)]
    struct Look {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        scale_pct: u32,
        light: bool,
        hover: bool,
        pressed: bool,
    }

    #[derive(Default)]
    struct State {
        taskbar: HWND,
        icon: Option<Icon>,
        shown: bool,
        drawn: Option<Look>,
        hover: bool,
        pressed: bool,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::new(State::default());
    }

    pub fn start(app: &AppHandle) {
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = APP.set(app.clone());
        let icon = app.default_window_icon().map(|i| Icon {
            rgba: i.rgba().to_vec(),
            width: i.width() as usize,
            height: i.height() as usize,
        });
        std::thread::Builder::new()
            .name("taskbar-button".into())
            .spawn(move || run(icon))
            .ok();
    }

    fn run(icon: Option<Icon>) {
        STATE.with_borrow_mut(|s| s.icon = icon);
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else { return };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: CLASS,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return;
        }

        loop {
            // No taskbar yet (Explorer starting or restarting): wait for it.
            let Ok(taskbar) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }) else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            STATE.with_borrow_mut(|s| {
                s.taskbar = taskbar;
                s.shown = false;
                s.drawn = None;
                s.hover = false;
                s.pressed = false;
            });
            let created = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                    CLASS,
                    w!("Dock Panel"),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    Some(taskbar),
                    None,
                    Some(module.into()),
                    None,
                )
            };
            if let Ok(hwnd) = created {
                unsafe { SetTimer(Some(hwnd), 1, TIMER_MS, None) };
                update(hwnd);
                let mut msg = MSG::default();
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_TIMER => update(hwnd),
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                let entered = STATE.with_borrow_mut(|s| !std::mem::replace(&mut s.hover, true));
                if entered {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = unsafe { TrackMouseEvent(&mut track) };
                    update(hwnd);
                }
            }
            WM_MOUSELEAVE => {
                STATE.with_borrow_mut(|s| s.hover = false);
                update(hwnd);
            }
            WM_LBUTTONDOWN => {
                STATE.with_borrow_mut(|s| s.pressed = true);
                unsafe { SetCapture(hwnd) };
                update(hwnd);
            }
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                let was_pressed = STATE.with_borrow_mut(|s| std::mem::replace(&mut s.pressed, false));
                update(hwnd);
                if was_pressed && cursor_inside(hwnd, lparam) {
                    if let Some(app) = APP.get() {
                        panel::toggle(app);
                    }
                }
            }
            WM_DESTROY => unsafe { PostQuitMessage(0) },
            _ => {}
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// Client coordinates from a mouse message's lParam, checked against the window size.
    fn cursor_inside(hwnd: HWND, lparam: LPARAM) -> bool {
        let x = (lparam.0 & 0xFFFF) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
        let mut r = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        x >= 0 && y >= 0 && x < r.right - r.left && y < r.bottom - r.top
    }

    fn reg_dword(path: PCWSTR, name: PCWSTR) -> Option<u32> {
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                path,
                name,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        err.is_ok().then_some(value)
    }

    /// Where the button should be, or `None` when it should be hidden.
    fn placement(taskbar: HWND) -> Option<(i32, i32, i32, i32, u32)> {
        if !super::ENABLED.load(Ordering::SeqCst) || !unsafe { IsWindowVisible(taskbar) }.as_bool() {
            return None;
        }
        // Left-aligned taskbar: the Start button is where we would go.
        let advanced = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");
        if reg_dword(advanced, w!("TaskbarAl")) == Some(0) {
            return None;
        }

        let mut bar = RECT::default();
        unsafe { GetWindowRect(taskbar, &mut bar) }.ok()?;
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let monitor = unsafe { MonitorFromWindow(taskbar, MONITOR_DEFAULTTONEAREST) };
        unsafe { GetMonitorInfoW(monitor, &mut info) }.ok().ok()?;
        let top = bar.top.max(info.rcMonitor.top);
        let bottom = bar.bottom.min(info.rcMonitor.bottom);
        let height = bar.bottom - bar.top;
        // Auto-hidden: only a sliver of the taskbar is on screen.
        if height <= 0 || bottom - top < height / 2 {
            return None;
        }

        let dpi = unsafe { GetDpiForWindow(taskbar) }.max(96);
        let scale = dpi as f64 / 96.0;
        let w = (WIDTH * scale).round() as i32;
        let x = bar.left.max(info.rcMonitor.left) + (LEFT_GAP * scale).round() as i32;
        Some((x, top, w, bottom - top, dpi * 100 / 96))
    }

    fn update(hwnd: HWND) {
        STATE.with_borrow_mut(|s| {
            let Some((x, y, w, h, scale_pct)) = placement(s.taskbar) else {
                if s.shown {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    s.shown = false;
                }
                return;
            };
            let light = reg_dword(
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("SystemUsesLightTheme"),
            ) == Some(1);
            let look = Look { x, y, w, h, scale_pct, light, hover: s.hover, pressed: s.pressed };
            if s.drawn != Some(look) {
                if draw(hwnd, &look, s.icon.as_ref()).is_some() {
                    s.drawn = Some(look);
                }
            }
            if !s.shown {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                s.shown = true;
            }
        });
    }

    /// Premultiplied BGRA "over" blend of one pixel.
    fn over(dst: &mut [u8], (b, g, r, a): (f64, f64, f64, f64)) {
        let keep = 1.0 - a / 255.0;
        dst[0] = (b + dst[0] as f64 * keep).round().min(255.0) as u8;
        dst[1] = (g + dst[1] as f64 * keep).round().min(255.0) as u8;
        dst[2] = (r + dst[2] as f64 * keep).round().min(255.0) as u8;
        dst[3] = (a + dst[3] as f64 * keep).round().min(255.0) as u8;
    }

    /// The button as premultiplied BGRA, top-down rows.
    fn pixels(look: &Look, icon: Option<&Icon>) -> Vec<u8> {
        let (w, h) = (look.w as usize, look.h as usize);
        let scale = look.scale_pct as f64 / 100.0;
        // Alpha 1 everywhere: fully transparent pixels of a layered window
        // don't receive the mouse.
        let mut out = [0u8, 0, 0, 1].repeat(w * h);

        // Hover plate, like the taskbar's own buttons.
        let plate_alpha = match (look.hover, look.pressed, look.light) {
            (_, true, false) => 0.045,
            (true, false, false) => 0.075,
            (_, true, true) => 0.035,
            (true, false, true) => 0.06,
            _ => 0.0,
        };
        if plate_alpha > 0.0 {
            let tone = if look.light { 0.0 } else { 255.0 };
            let side = (PLATE * scale).min(w as f64).min(h as f64);
            let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
            let half = side / 2.0;
            let radius = RADIUS * scale;
            for y in 0..h {
                for x in 0..w {
                    // Signed distance to a rounded square, antialiased over one pixel.
                    let dx = ((x as f64 + 0.5 - cx).abs() - (half - radius)).max(0.0);
                    let dy = ((y as f64 + 0.5 - cy).abs() - (half - radius)).max(0.0);
                    let d = (dx * dx + dy * dy).sqrt() - radius;
                    let cover = (0.5 - d).clamp(0.0, 1.0);
                    if cover > 0.0 {
                        let a = 255.0 * plate_alpha * cover;
                        let c = tone * plate_alpha * cover;
                        let i = (y * w + x) * 4;
                        over(&mut out[i..i + 4], (c, c, c, a));
                    }
                }
            }
        }

        // App icon, box-filtered down to the taskbar icon size.
        if let Some(icon) = icon {
            let size = ((ICON * scale).round() as usize).min(w).min(h).max(1);
            let (ox, oy) = ((w - size) / 2, (h - size) / 2 + usize::from(look.pressed));
            for ty in 0..size {
                let (y0, y1) = (ty * icon.height / size, ((ty + 1) * icon.height / size).max(ty * icon.height / size + 1));
                for tx in 0..size {
                    let (x0, x1) = (tx * icon.width / size, ((tx + 1) * icon.width / size).max(tx * icon.width / size + 1));
                    let (mut r, mut g, mut b, mut a, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
                    for sy in y0..y1.min(icon.height) {
                        for sx in x0..x1.min(icon.width) {
                            let p = &icon.rgba[(sy * icon.width + sx) * 4..][..4];
                            let pa = p[3] as f64 / 255.0;
                            r += p[0] as f64 * pa;
                            g += p[1] as f64 * pa;
                            b += p[2] as f64 * pa;
                            a += p[3] as f64;
                            n += 1.0;
                        }
                    }
                    if n > 0.0 && a > 0.0 {
                        let (x, y) = (ox + tx, oy + ty);
                        if y < h {
                            let i = (y * w + x) * 4;
                            over(&mut out[i..i + 4], (b / n, g / n, r / n, a / n));
                        }
                    }
                }
            }
        }
        out
    }

    fn draw(hwnd: HWND, look: &Look, icon: Option<&Icon>) -> Option<()> {
        let data = pixels(look, icon);
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: look.w,
                    biHeight: -look.h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return None;
            };
            std::ptr::copy_nonoverlapping(data.as_ptr(), bits as *mut u8, data.len());
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let result = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&POINT { x: look.x, y: look.y }),
                Some(&SIZE { cx: look.w, cy: look.h }),
                Some(dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            result.ok()
        }
    }
}
