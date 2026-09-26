//! A Dock Panel button at the left end of the Windows 11 taskbar, with a mini
//! player next to it while something is playing (or paused).
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
//! the thread waits for the new taskbar and starts over. A second thread polls
//! the system media session for the player.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;

static ENABLED: AtomicBool = AtomicBool::new(true);
static PLAYER: AtomicBool = AtomicBool::new(true);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

pub fn player_enabled() -> bool {
    PLAYER.load(Ordering::SeqCst)
}

/// Both take effect on the button's next timer tick.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
}

pub fn set_player_enabled(on: bool) {
    PLAYER.store(on, Ordering::SeqCst);
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
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    use tauri::AppHandle;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetMonitorInfoW,
        GetTextExtentPoint32W, MonitorFromWindow, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER,
        ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS,
        DEFAULT_CHARSET, DIB_RGB_COLORS, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HDC, HFONT,
        HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, GetWindow, GetWindowRect,
        SetWindowPos, GW_HWNDPREV, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
        IsWindowVisible, LoadCursorW, PostQuitMessage, RegisterClassW, SetTimer, ShowWindow, TranslateMessage,
        UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_DESTROY,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    use crate::{media, panel};

    /// winuser.h; the windows crate only exports it with the Controls feature.
    const WM_MOUSELEAVE: u32 = 0x02A3;
    const CLASS: PCWSTR = w!("DockPanelTaskbarButton");
    const TIMER_MS: u32 = 250;
    const MEDIA_POLL: Duration = Duration::from_millis(1000);
    /// Album art is decoded at this size and filtered down to the DPI's size.
    const COVER_PX: u32 = 96;

    // Layout, in DIPs, matching the Windows 11 taskbar buttons.
    const LEFT_GAP: f64 = 6.0;
    const BUTTON_W: f64 = 44.0;
    const PLATE_H: f64 = 40.0;
    const RADIUS: f64 = 4.0;
    const ICON: f64 = 24.0;
    const PLAYER_GAP: f64 = 2.0;
    const INFO_PAD: f64 = 6.0;
    const COVER: f64 = 30.0;
    const COVER_RADIUS: f64 = 3.0;
    const TEXT_GAP: f64 = 8.0;
    const TEXT_MIN: f64 = 60.0;
    const TEXT_MAX: f64 = 190.0;
    const CONTROL_W: f64 = 34.0;
    const TITLE_PX: f64 = 12.0;
    const ARTIST_PX: f64 = 11.0;

    #[derive(Clone, Copy)]
    enum Glyph {
        Prev,
        Play,
        Pause,
        Next,
    }

    static APP: OnceLock<AppHandle> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);
    /// Asks the media poller to refresh now, e.g. right after a control click.
    static POKE: AtomicBool = AtomicBool::new(false);
    static MEDIA: Mutex<Option<Media>> = Mutex::new(None);

    /// Straight RGBA icon pixels.
    struct Icon {
        rgba: Vec<u8>,
        width: usize,
        height: usize,
    }

    /// What the player shows; `cover` changes identity only with a new image.
    #[derive(Clone, PartialEq)]
    struct Media {
        title: String,
        artist: String,
        playing: bool,
        can_prev: bool,
        can_next: bool,
        cover: Option<Arc<Vec<u8>>>,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Hit {
        Panel,
        Info,
        Prev,
        Toggle,
        Next,
    }

    /// Pixel rectangles of the clickable parts, relative to the window.
    #[derive(Clone, Copy, PartialEq, Default)]
    struct Zone {
        x: i32,
        w: i32,
    }

    #[derive(Clone, PartialEq, Default)]
    struct Layout {
        panel: Zone,
        info: Option<Zone>,
        prev: Option<Zone>,
        toggle: Option<Zone>,
        next: Option<Zone>,
        text_w: i32,
    }

    impl Layout {
        fn hit(&self, x: i32) -> Option<Hit> {
            let inside = |z: &Zone| x >= z.x && x < z.x + z.w;
            [
                (Some(self.panel), Hit::Panel),
                (self.info, Hit::Info),
                (self.prev, Hit::Prev),
                (self.toggle, Hit::Toggle),
                (self.next, Hit::Next),
            ]
            .into_iter()
            .find_map(|(z, hit)| z.filter(inside).map(|_| hit))
        }

        fn width(&self) -> i32 {
            [Some(self.panel), self.info, self.prev, self.toggle, self.next]
                .into_iter()
                .flatten()
                .map(|z| z.x + z.w)
                .max()
                .unwrap_or(0)
        }
    }

    #[derive(Clone, PartialEq)]
    struct Look {
        x: i32,
        y: i32,
        h: i32,
        scale: f64,
        light: bool,
        hover: Option<Hit>,
        pressed: Option<Hit>,
        media: Option<Media>,
        layout: Layout,
    }

    #[derive(Default)]
    struct State {
        taskbar: HWND,
        icon: Option<Icon>,
        shown: bool,
        drawn: Option<Look>,
        layout: Layout,
        hover: Option<Hit>,
        pressed: Option<Hit>,
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
        std::thread::Builder::new().name("taskbar-button".into()).spawn(move || run(icon)).ok();
        std::thread::Builder::new().name("taskbar-media".into()).spawn(poll_media).ok();
    }

    // ---- media ------------------------------------------------------------------

    fn poll_media() {
        let mut key = (String::new(), String::new());
        let mut cover: Option<Arc<Vec<u8>>> = None;
        let mut cover_tries = 0;
        loop {
            let wanted = super::enabled() && super::player_enabled();
            let now = if wanted { media::win::now_playing().ok().flatten() } else { None };
            let next = now.filter(|n| !n.title.is_empty()).map(|n| {
                let new_key = (n.title.clone(), n.artist.clone());
                if new_key != key {
                    key = new_key;
                    cover = None;
                    cover_tries = 0;
                }
                // Players often publish the art a moment after the title.
                if cover.is_none() && cover_tries < 3 {
                    cover_tries += 1;
                    cover = media::win::cover_pixels(COVER_PX).ok().flatten().map(Arc::new);
                }
                Media {
                    title: n.title,
                    artist: n.artist,
                    playing: n.playing,
                    can_prev: n.can_prev,
                    can_next: n.can_next,
                    cover: cover.clone(),
                }
            });
            *MEDIA.lock().unwrap_or_else(|e| e.into_inner()) = next;

            for _ in 0..(MEDIA_POLL.as_millis() / 50) {
                std::thread::sleep(Duration::from_millis(50));
                if POKE.swap(false, Ordering::SeqCst) {
                    break;
                }
            }
        }
    }

    fn media_now() -> Option<Media> {
        MEDIA.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn control(action: &'static str) {
        if action == "toggle" {
            if let Some(m) = MEDIA.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                m.playing = !m.playing;
            }
        }
        std::thread::spawn(move || {
            let _ = media::win::control(action);
            std::thread::sleep(Duration::from_millis(300));
            POKE.store(true, Ordering::SeqCst);
        });
    }

    // ---- window -----------------------------------------------------------------

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
                s.hover = None;
                s.pressed = None;
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

    fn mouse_x(lparam: LPARAM) -> i32 {
        (lparam.0 & 0xFFFF) as i16 as i32
    }

    fn mouse_y(lparam: LPARAM) -> i32 {
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_TIMER => update(hwnd),
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                let (entered, changed) = STATE.with_borrow_mut(|s| {
                    let hit = s.layout.hit(mouse_x(lparam));
                    let entered = s.hover.is_none();
                    (entered, std::mem::replace(&mut s.hover, hit) != hit)
                });
                if entered {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = unsafe { TrackMouseEvent(&mut track) };
                }
                if changed {
                    update(hwnd);
                }
            }
            WM_MOUSELEAVE => {
                STATE.with_borrow_mut(|s| s.hover = None);
                update(hwnd);
            }
            WM_LBUTTONDOWN => {
                STATE.with_borrow_mut(|s| s.pressed = s.layout.hit(mouse_x(lparam)));
                unsafe { SetCapture(hwnd) };
                update(hwnd);
            }
            WM_LBUTTONUP => {
                let _ = unsafe { ReleaseCapture() };
                let (pressed, released_on) = STATE.with_borrow_mut(|s| {
                    let mut r = RECT::default();
                    let _ = unsafe { GetWindowRect(hwnd, &mut r) };
                    let y = mouse_y(lparam);
                    let on = if y >= 0 && y < r.bottom - r.top { s.layout.hit(mouse_x(lparam)) } else { None };
                    (s.pressed.take(), on)
                });
                update(hwnd);
                if pressed.is_some() && pressed == released_on {
                    match pressed {
                        Some(Hit::Panel | Hit::Info) => {
                            if let Some(app) = APP.get() {
                                panel::toggle(app);
                            }
                        }
                        Some(Hit::Prev) => control("prev"),
                        Some(Hit::Toggle) => control("toggle"),
                        Some(Hit::Next) => control("next"),
                        None => {}
                    }
                    update(hwnd);
                }
            }
            WM_DESTROY => unsafe { PostQuitMessage(0) },
            _ => {}
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
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

    /// Top-left corner, height and DPI scale on the taskbar, or `None` when the
    /// button should be hidden.
    fn placement(taskbar: HWND) -> Option<(i32, i32, i32, f64)> {
        if !super::enabled() || !unsafe { IsWindowVisible(taskbar) }.as_bool() {
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

        let scale = unsafe { GetDpiForWindow(taskbar) }.max(96) as f64 / 96.0;
        let x = bar.left.max(info.rcMonitor.left) + (LEFT_GAP * scale).round() as i32;
        Some((x, top, bottom - top, scale))
    }

    fn px(dip: f64, scale: f64) -> i32 {
        (dip * scale).round() as i32
    }

    fn layout(scale: f64, media: Option<&Media>) -> Layout {
        let mut l = Layout { panel: Zone { x: 0, w: px(BUTTON_W, scale) }, ..Default::default() };
        let Some(m) = media else { return l };

        let title = text_width(&m.title, px(TITLE_PX, scale), 600, w!("Segoe UI"));
        let artist = text_width(&m.artist, px(ARTIST_PX, scale), 400, w!("Segoe UI"));
        l.text_w = title.max(artist).clamp(px(TEXT_MIN, scale), px(TEXT_MAX, scale));

        let mut x = l.panel.w + px(PLAYER_GAP, scale);
        let info_w = px(INFO_PAD + COVER + TEXT_GAP + INFO_PAD, scale) + l.text_w;
        l.info = Some(Zone { x, w: info_w });
        x += info_w;
        let cw = px(CONTROL_W, scale);
        if m.can_prev {
            l.prev = Some(Zone { x, w: cw });
            x += cw;
        }
        l.toggle = Some(Zone { x, w: cw });
        x += cw;
        if m.can_next {
            l.next = Some(Zone { x, w: cw });
        }
        l
    }

    fn update(hwnd: HWND) {
        STATE.with_borrow_mut(|s| {
            let Some((x, y, h, scale)) = placement(s.taskbar) else {
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
            let media = if super::player_enabled() { media_now() } else { None };
            // Text is measured only when the track changes.
            let same_track = s.drawn.as_ref().is_some_and(|d| {
                d.scale == scale
                    && d.media.as_ref().map(|m| (&m.title, &m.artist, m.can_prev, m.can_next))
                        == media.as_ref().map(|m| (&m.title, &m.artist, m.can_prev, m.can_next))
            });
            let layout = match &s.drawn {
                Some(d) if same_track => d.layout.clone(),
                _ => layout(scale, media.as_ref()),
            };
            s.layout = layout.clone();
            let look = Look { x, y, h, scale, light, hover: s.hover, pressed: s.pressed, media, layout };
            if s.drawn.as_ref() != Some(&look) && draw(hwnd, &look, s.icon.as_ref()).is_some() {
                s.drawn = Some(look);
            }
            if !s.shown {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                s.shown = true;
            }
            keep_above(hwnd, s.taskbar);
        });
    }

    /// Owned windows are meant to stay above their owner, but across processes
    /// Windows doesn't always keep that: after a fullscreen overlay (the
    /// Snipping Tool, for one) the taskbar can come back on top of the button.
    /// Puts the button right above the taskbar again — not at the very top, so
    /// it doesn't float over fullscreen apps the taskbar is below.
    fn keep_above(hwnd: HWND, taskbar: HWND) {
        unsafe {
            // Walk up from the button: meeting the taskbar means it covers us.
            let mut above = GetWindow(hwnd, GW_HWNDPREV).ok();
            while let Some(w) = above {
                if w == taskbar {
                    let insert_after = GetWindow(taskbar, GW_HWNDPREV).ok().unwrap_or(HWND_TOPMOST);
                    let _ = SetWindowPos(
                        hwnd,
                        Some(insert_after),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                    );
                    return;
                }
                above = GetWindow(w, GW_HWNDPREV).ok();
            }
        }
    }

    // ---- drawing ----------------------------------------------------------------

    /// Premultiplied BGRA pixels, top-down.
    struct Canvas {
        w: usize,
        h: usize,
        px: Vec<u8>,
    }

    impl Canvas {
        fn new(w: i32, h: i32) -> Self {
            let (w, h) = (w.max(1) as usize, h.max(1) as usize);
            // Alpha 1 everywhere: fully transparent pixels of a layered window
            // don't receive the mouse.
            Canvas { w, h, px: [0u8, 0, 0, 1].repeat(w * h) }
        }

        /// "Over" blend of a premultiplied colour at (x, y).
        fn blend(&mut self, x: usize, y: usize, (b, g, r, a): (f64, f64, f64, f64)) {
            if x >= self.w || y >= self.h || a <= 0.0 {
                return;
            }
            let d = &mut self.px[(y * self.w + x) * 4..][..4];
            let keep = 1.0 - a / 255.0;
            for (c, v) in d.iter_mut().zip([b, g, r, a]) {
                *c = (v + *c as f64 * keep).round().clamp(0.0, 255.0) as u8;
            }
        }

        /// Coverage of a rounded rectangle at a pixel, antialiased over one pixel.
        fn round_cover(px: f64, py: f64, (x, y, w, h): (f64, f64, f64, f64), r: f64) -> f64 {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let dx = ((px - cx).abs() - (w / 2.0 - r)).max(0.0);
            let dy = ((py - cy).abs() - (h / 2.0 - r)).max(0.0);
            (0.5 - ((dx * dx + dy * dy).sqrt() - r)).clamp(0.0, 1.0)
        }

        fn round_rect(&mut self, rect: (f64, f64, f64, f64), r: f64, tone: f64, alpha: f64) {
            let (x0, y0) = (rect.0.floor().max(0.0) as usize, rect.1.floor().max(0.0) as usize);
            let (x1, y1) = ((rect.0 + rect.2).ceil() as usize, (rect.1 + rect.3).ceil() as usize);
            for y in y0..y1.min(self.h) {
                for x in x0..x1.min(self.w) {
                    let c = Self::round_cover(x as f64 + 0.5, y as f64 + 0.5, rect, r) * alpha;
                    if c > 0.0 {
                        self.blend(x, y, (tone * c, tone * c, tone * c, 255.0 * c));
                    }
                }
            }
        }

        /// Box-filters `src` (premultiplied BGRA from `pixel`) into a `size` square at (ox, oy),
        /// clipped to a rounded square of radius `r`.
        fn image(
            &mut self,
            (ox, oy, size): (usize, usize, usize),
            (sw, sh): (usize, usize),
            r: f64,
            pixel: impl Fn(usize, usize) -> [f64; 4],
        ) {
            let rect = (ox as f64, oy as f64, size as f64, size as f64);
            for ty in 0..size {
                let (y0, y1) = (ty * sh / size, ((ty + 1) * sh / size).max(ty * sh / size + 1).min(sh));
                for tx in 0..size {
                    let (x0, x1) = (tx * sw / size, ((tx + 1) * sw / size).max(tx * sw / size + 1).min(sw));
                    let mut acc = [0.0; 4];
                    let mut n = 0.0;
                    for sy in y0..y1 {
                        for sx in x0..x1 {
                            for (a, v) in acc.iter_mut().zip(pixel(sx, sy)) {
                                *a += v;
                            }
                            n += 1.0;
                        }
                    }
                    if n == 0.0 {
                        continue;
                    }
                    let (x, y) = (ox + tx, oy + ty);
                    let clip = if r > 0.0 { Self::round_cover(x as f64 + 0.5, y as f64 + 0.5, rect, r) } else { 1.0 };
                    let k = clip / n;
                    self.blend(x, y, (acc[0] * k, acc[1] * k, acc[2] * k, acc[3] * k));
                }
            }
        }

        /// Filled triangle, antialiased by 4×4 supersampling.
        fn triangle(&mut self, p: [(f64, f64); 3], tone: f64, alpha: f64) {
            let edge = |a: (f64, f64), b: (f64, f64), x: f64, y: f64| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
            let area = edge(p[0], p[1], p[2].0, p[2].1);
            let inside = |x: f64, y: f64| {
                let e = [edge(p[0], p[1], x, y), edge(p[1], p[2], x, y), edge(p[2], p[0], x, y)];
                e.iter().all(|&v| v * area >= 0.0)
            };
            let xs = p.map(|q| q.0);
            let ys = p.map(|q| q.1);
            let (x0, x1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
            let (y0, y1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
            for y in (y0.floor().max(0.0) as usize)..(y1.ceil() as usize).min(self.h) {
                for x in (x0.floor().max(0.0) as usize)..(x1.ceil() as usize).min(self.w) {
                    let mut hits = 0;
                    for sy in 0..4 {
                        for sx in 0..4 {
                            if inside(x as f64 + (sx as f64 + 0.5) / 4.0, y as f64 + (sy as f64 + 0.5) / 4.0) {
                                hits += 1;
                            }
                        }
                    }
                    let c = hits as f64 / 16.0 * alpha;
                    if c > 0.0 {
                        self.blend(x, y, (tone * c, tone * c, tone * c, 255.0 * c));
                    }
                }
            }
        }

        /// Tints an alpha mask (e.g. rendered text) into the canvas at (ox, oy).
        fn mask(&mut self, mask: &[u8], (ox, oy, mw, mh): (usize, usize, usize, usize), tone: f64, alpha: f64) {
            for y in 0..mh {
                for x in 0..mw {
                    let c = mask[y * mw + x] as f64 / 255.0 * alpha;
                    if c > 0.0 {
                        self.blend(ox + x, oy + y, (tone * c, tone * c, tone * c, 255.0 * c));
                    }
                }
            }
        }
    }

    struct Font(HFONT);

    impl Font {
        fn new(px: i32, weight: i32, face: PCWSTR) -> Self {
            Font(unsafe {
                CreateFontW(
                    -px,
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    ANTIALIASED_QUALITY,
                    0,
                    face,
                )
            })
        }
    }

    impl Drop for Font {
        fn drop(&mut self) {
            let _ = unsafe { DeleteObject(HGDIOBJ(self.0 .0)) };
        }
    }

    /// A memory DC with a 32-bit top-down DIB selected; frees everything on drop.
    struct Surface {
        dc: HDC,
        bitmap: HGDIOBJ,
        old: HGDIOBJ,
        bits: *mut u8,
        len: usize,
    }

    impl Surface {
        fn new(w: i32, h: i32) -> Option<Self> {
            unsafe {
                let dc = CreateCompatibleDC(None);
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
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
                let bitmap = HGDIOBJ(bitmap.0);
                let old = SelectObject(dc, bitmap);
                Some(Surface { dc, bitmap, old, bits: bits as *mut u8, len: (w * h * 4) as usize })
            }
        }

        fn bytes(&mut self) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.bits, self.len) }
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.dc, self.old);
                let _ = DeleteObject(self.bitmap);
                let _ = DeleteDC(self.dc);
            }
        }
    }

    fn text_width(text: &str, px: i32, weight: i32, face: PCWSTR) -> i32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let font = Font::new(px, weight, face);
        let Some(s) = Surface::new(1, 1) else { return 0 };
        let mut size = SIZE::default();
        unsafe {
            let old = SelectObject(s.dc, HGDIOBJ(font.0 .0));
            let _ = GetTextExtentPoint32W(s.dc, &wide, &mut size);
            SelectObject(s.dc, old);
        }
        size.cx
    }

    /// Grayscale-antialiased text as an alpha mask, cut with "…" to fit `w`.
    fn text_mask(text: &str, px: i32, weight: i32, face: PCWSTR, w: i32, h: i32) -> Vec<u8> {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let font = Font::new(px, weight, face);
        let Some(mut s) = Surface::new(w, h) else { return vec![0; (w * h).max(0) as usize] };
        unsafe {
            let old = SelectObject(s.dc, HGDIOBJ(font.0 .0));
            SetTextColor(s.dc, COLORREF(0x00FF_FFFF));
            SetBkMode(s.dc, TRANSPARENT);
            let mut rect = RECT { left: 0, top: 0, right: w, bottom: h };
            DrawTextW(s.dc, &mut wide, &mut rect, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
            SelectObject(s.dc, old);
        }
        // White on black: any channel is the coverage.
        s.bytes().chunks_exact(4).map(|p| p[1]).collect()
    }

    fn draw(hwnd: HWND, look: &Look, icon: Option<&Icon>) -> Option<()> {
        let s = look.scale;
        let l = &look.layout;
        let h = look.h;
        let mut c = Canvas::new(l.width(), h);
        let fg = if look.light { 0.0 } else { 255.0 };
        let mid = h as f64 / 2.0;

        // Hover / pressed plates, like the taskbar's own buttons.
        let plate = |hit: Hit| -> f64 {
            match (look.pressed == Some(hit), look.hover == Some(hit), look.light) {
                (true, _, false) => 0.045,
                (false, true, false) => 0.075,
                (true, _, true) => 0.035,
                (false, true, true) => 0.06,
                _ => 0.0,
            }
        };
        let plate_h = (PLATE_H * s).min(h as f64);
        let zones = [(Some(l.panel), Hit::Panel), (l.info, Hit::Info), (l.prev, Hit::Prev), (l.toggle, Hit::Toggle), (l.next, Hit::Next)];
        for (zone, hit) in zones {
            if let Some(z) = zone {
                let a = plate(hit);
                if a > 0.0 {
                    let inset = 2.0 * s;
                    let rect = (z.x as f64 + inset, mid - plate_h / 2.0, z.w as f64 - 2.0 * inset, plate_h);
                    c.round_rect(rect, RADIUS * s, fg * a, a);
                }
            }
        }

        // App icon.
        if let Some(icon) = icon {
            let size = (px(ICON, s) as usize).min(l.panel.w as usize).min(h as usize).max(1);
            let pressed = usize::from(look.pressed == Some(Hit::Panel));
            let origin = ((l.panel.w as usize - size) / 2, (h as usize - size) / 2 + pressed, size);
            c.image(origin, (icon.width, icon.height), 0.0, |x, y| {
                let p = &icon.rgba[(y * icon.width + x) * 4..][..4];
                let a = p[3] as f64 / 255.0;
                [p[2] as f64 * a, p[1] as f64 * a, p[0] as f64 * a, p[3] as f64]
            });
        }

        // Player: cover, title and artist, controls.
        if let (Some(m), Some(info)) = (&look.media, l.info) {
            let cover = px(COVER, s);
            let cx = (info.x + px(INFO_PAD, s)) as usize;
            let cy = ((h - cover) / 2) as usize;
            match &m.cover {
                Some(pixels) => {
                    let n = COVER_PX as usize;
                    c.image((cx, cy, cover as usize), (n, n), COVER_RADIUS * s, |x, y| {
                        let p = &pixels[(y * n + x) * 4..][..4];
                        [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64]
                    });
                }
                None => c.round_rect((cx as f64, cy as f64, cover as f64, cover as f64), COVER_RADIUS * s, fg * 0.08, 0.08),
            }

            let tx = cx + (cover + px(TEXT_GAP, s)) as usize;
            let line = px(16.0, s);
            let (tw, top) = (l.text_w, (h / 2 - line) as usize);
            let title = text_mask(&m.title, px(TITLE_PX, s), 600, w!("Segoe UI"), tw, line);
            c.mask(&title, (tx, top, tw as usize, line as usize), fg, 0.92);
            if !m.artist.is_empty() {
                let artist = text_mask(&m.artist, px(ARTIST_PX, s), 400, w!("Segoe UI"), tw, line);
                c.mask(&artist, (tx, top + line as usize, tw as usize, line as usize), fg, 0.62);
            }

            // Control glyphs, drawn as shapes in DIPs around the zone's centre.
            let glyph = |c: &mut Canvas, zone: Option<Zone>, shape: Glyph, hit: Hit| {
                let Some(z) = zone else { return };
                let (cx, cy) = ((z.x + z.w / 2) as f64, mid + if look.pressed == Some(hit) { 1.0 } else { 0.0 });
                let at = |x: f64, y: f64| (cx + x * s, cy + y * s);
                let bar = |c: &mut Canvas, x0: f64, x1: f64, half: f64| {
                    let (ax, ay) = at(x0, -half);
                    c.round_rect((ax, ay, (x1 - x0) * s, 2.0 * half * s), 1.0 * s, fg * 0.9, 0.9);
                };
                match shape {
                    Glyph::Play => c.triangle([at(-4.5, -6.5), at(-4.5, 6.5), at(6.5, 0.0)], fg, 0.9),
                    Glyph::Pause => {
                        bar(c, -5.0, -1.5, 6.5);
                        bar(c, 1.5, 5.0, 6.5);
                    }
                    Glyph::Prev => {
                        bar(c, -6.0, -4.0, 5.5);
                        c.triangle([at(5.5, -5.5), at(5.5, 5.5), at(-3.5, 0.0)], fg, 0.9);
                    }
                    Glyph::Next => {
                        bar(c, 4.0, 6.0, 5.5);
                        c.triangle([at(-5.5, -5.5), at(-5.5, 5.5), at(3.5, 0.0)], fg, 0.9);
                    }
                }
            };
            glyph(&mut c, l.prev, Glyph::Prev, Hit::Prev);
            glyph(&mut c, l.toggle, if m.playing { Glyph::Pause } else { Glyph::Play }, Hit::Toggle);
            glyph(&mut c, l.next, Glyph::Next, Hit::Next);
        }

        present(hwnd, look, &c)
    }

    fn present(hwnd: HWND, look: &Look, c: &Canvas) -> Option<()> {
        let (w, h) = (c.w as i32, c.h as i32);
        let mut surface = Surface::new(w, h)?;
        surface.bytes().copy_from_slice(&c.px);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&POINT { x: look.x, y: look.y }),
                Some(&SIZE { cx: w, cy: h }),
                Some(surface.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
        .ok()
    }
}
