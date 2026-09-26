//! "Мониторы" widget: brightness, contrast, volume, input and power of external
//! monitors over DDC/CI (the monitor's own settings, like its OSD buttons).
//!
//! DDC/CI is a slow I²C bus: reading one value takes ~40 ms and the
//! capabilities string up to a couple of seconds, so capabilities are cached
//! per monitor and every bus operation is serialised behind one lock.

use serde::Serialize;

/// A continuous setting: current value and the monitor's maximum.
#[derive(Serialize, Clone, Copy)]
pub struct Level {
    value: u32,
    max: u32,
}

/// A setting with a fixed list of values (input source, power mode).
#[derive(Serialize, Clone)]
pub struct Choice {
    current: u32,
    options: Vec<u32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    id: String,
    name: String,
    primary: bool,
    brightness: Option<Level>,
    contrast: Option<Level>,
    volume: Option<Level>,
    input: Option<Choice>,
    power: Option<Choice>,
}

const BRIGHTNESS: u8 = 0x10;
const CONTRAST: u8 = 0x12;
const VOLUME: u8 = 0x62;
const INPUT: u8 = 0x60;
const POWER: u8 = 0xD6;

fn feature_code(feature: &str) -> Option<u8> {
    Some(match feature {
        "brightness" => BRIGHTNESS,
        "contrast" => CONTRAST,
        "volume" => VOLUME,
        "input" => INPUT,
        "power" => POWER,
        _ => return None,
    })
}

/// VCP codes from a capabilities string, with the values listed for those
/// that have them: `vcp(10 12 60(11 12 0F) D6(01 04))`.
fn parse_vcp(caps: &str) -> Vec<(u8, Vec<u32>)> {
    let lower = caps.to_ascii_lowercase();
    let Some(start) = lower.find("vcp(") else { return Vec::new() };
    let body = &caps[start + 4..];
    let mut out: Vec<(u8, Vec<u32>)> = Vec::new();
    let mut depth = 0;
    let mut token = String::new();
    let flush = |token: &mut String, depth: i32, out: &mut Vec<(u8, Vec<u32>)>| {
        if let Ok(v) = u32::from_str_radix(token, 16) {
            match depth {
                0 => out.push((v as u8, Vec::new())),
                _ => {
                    if let Some(last) = out.last_mut() {
                        last.1.push(v);
                    }
                }
            }
        }
        token.clear();
    };
    for c in body.chars() {
        match c {
            '(' => {
                flush(&mut token, depth, &mut out);
                depth += 1;
            }
            ')' => {
                flush(&mut token, depth, &mut out);
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            c if c.is_ascii_hexdigit() => token.push(c),
            _ => flush(&mut token, depth, &mut out),
        }
    }
    out
}

/// Reads capabilities in the background at startup, so the widget's first
/// load doesn't wait seconds for them.
pub fn warm_up() {
    std::thread::spawn(|| {
        let _ = win::list();
    });
}

#[tauri::command]
pub async fn monitors_list() -> Result<Vec<Monitor>, String> {
    tauri::async_runtime::spawn_blocking(win::list).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn monitor_set(id: String, feature: String, value: u32) -> Result<(), String> {
    let code = feature_code(&feature).ok_or("неизвестная настройка")?;
    tauri::async_runtime::spawn_blocking(move || win::set(&id, code, value)).await.map_err(|e| e.to_string())?
}

/// Blacks out every screen and turns monitors' brightness to the minimum.
///
/// By default the panel hides and every monitor dims; a click or any key
/// brings it all back. With `around_panel` the panel stays open on top of the
/// black screens, its own monitor keeps its brightness, and a click on the
/// black area (or `monitors_blackout_end`) ends it. The widget hears about
/// both ends through the `monitors:blackout` event.
#[tauri::command]
pub fn monitors_blackout(app: tauri::AppHandle, around_panel: Option<bool>) {
    let around_panel = around_panel.unwrap_or(false);
    if !around_panel {
        crate::panel::request_hide(&app);
    }
    #[cfg(windows)]
    blackout::start(app, around_panel);
}

#[tauri::command]
pub fn monitors_blackout_end() {
    #[cfg(windows)]
    blackout::end();
}

#[tauri::command]
pub fn monitors_blackout_active() -> bool {
    #[cfg(windows)]
    return blackout::active();
    #[cfg(not(windows))]
    false
}

/// Black topmost windows over every screen, with the brightness to restore.
///
/// The windows go up first, so the screens go dark at once; DDC/CI then dims
/// the backlight, which takes a second or two. Mouse movement is ignored so a
/// nudge doesn't wake them, and input in the first moments is ignored too: the
/// click that started it must not end it.
#[cfg(windows)]
mod blackout {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, BLACK_BRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW, PostQuitMessage,
        PostThreadMessageW, RegisterClassW, SetCursor, SetForegroundWindow, SetWindowPos, TranslateMessage,
        HWND_TOPMOST, IDC_ARROW, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SETCURSOR, WM_SYSKEYDOWN, WNDCLASSW, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    use super::{win, BRIGHTNESS};

    /// Input this soon after the start is the click that started it.
    const GRACE: Duration = Duration::from_millis(700);

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    /// The blackout thread, so `end` can stop its message loop.
    static THREAD: AtomicU32 = AtomicU32::new(0);
    static STARTED_AT: Mutex<Option<Instant>> = Mutex::new(None);
    /// Around the panel the cursor stays: the panel is still in use.
    static HIDE_CURSOR: AtomicBool = AtomicBool::new(true);

    pub fn active() -> bool {
        ACTIVE.load(Ordering::SeqCst)
    }

    pub fn end() {
        let thread = THREAD.load(Ordering::SeqCst);
        if thread != 0 {
            let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
    }

    pub fn start(app: AppHandle, around_panel: bool) {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = app.emit("monitors:blackout", true);
        std::thread::spawn(move || {
            THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
            HIDE_CURSOR.store(!around_panel, Ordering::SeqCst);
            let panel = if around_panel {
                crate::panel::hold_open(&app, true);
                crate::panel::hwnd(&app).map(|h| HWND(h as _))
            } else {
                // Let the panel's hide animation finish before the screens go black.
                std::thread::sleep(Duration::from_millis(250));
                None
            };
            // The panel's own monitor keeps its brightness, or the panel would be unreadable.
            let keep = panel.and_then(win::device_of).map(|device| format!("{device}#"));

            let windows = cover(panel.is_none());
            if let Some(panel) = panel {
                // The black windows came later, so they're above the panel: put it back on top.
                let _ = unsafe { SetWindowPos(panel, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
                let _ = unsafe { SetForegroundWindow(panel) };
            }
            *STARTED_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());

            // Remember each monitor's brightness, then dim it, off this thread's message loop.
            let dimmer = std::thread::spawn(move || {
                let saved: Vec<(String, u32)> = win::list()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| keep.as_ref().is_none_or(|k| !m.id.starts_with(k.as_str())))
                    .filter_map(|m| Some((m.id, m.brightness?.value)))
                    .collect();
                for (id, _) in &saved {
                    let _ = win::set(id, BRIGHTNESS, 0);
                }
                saved
            });

            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            for hwnd in windows {
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            if around_panel {
                crate::panel::hold_open(&app, false);
            }
            THREAD.store(0, Ordering::SeqCst);
            // Waits for the dimming if the screens were woken before it finished.
            for (id, value) in dimmer.join().unwrap_or_default() {
                let _ = win::set(&id, BRIGHTNESS, value);
            }
            ACTIVE.store(false, Ordering::SeqCst);
            let _ = app.emit("monitors:blackout", false);
        });
    }

    /// One black window per screen, on this thread; `focus` takes the keyboard
    /// so any key can end the blackout.
    fn cover(focus: bool) -> Vec<HWND> {
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else { return Vec::new() };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: w!("DockPanelBlackout"),
            hbrBackground: HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0),
            ..Default::default()
        };
        // Fails harmlessly when already registered by an earlier blackout.
        unsafe { RegisterClassW(&class) };
        let windows: Vec<HWND> = win::screen_rects()
            .into_iter()
            .filter_map(|r| unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                    w!("DockPanelBlackout"),
                    w!("Dock Panel"),
                    WS_POPUP | WS_VISIBLE,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    None,
                    None,
                    Some(module.into()),
                    None,
                )
                .ok()
            })
            .collect();
        if focus {
            if let Some(&first) = windows.first() {
                let _ = unsafe { SetForegroundWindow(first) };
            }
        }
        windows
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_SETCURSOR => {
                let cursor = if HIDE_CURSOR.load(Ordering::SeqCst) { None } else { unsafe { LoadCursorW(None, IDC_ARROW) }.ok() };
                unsafe { SetCursor(cursor) };
                return LRESULT(1);
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_KEYDOWN | WM_SYSKEYDOWN => {
                let started = *STARTED_AT.lock().unwrap_or_else(|e| e.into_inner());
                if started.is_some_and(|t| t.elapsed() >= GRACE) {
                    unsafe { PostQuitMessage(0) };
                }
                return LRESULT(0);
            }
            _ => {}
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    use windows::Win32::Devices::Display::{
        CapabilitiesRequestAndCapabilitiesReply, DestroyPhysicalMonitors, DisplayConfigGetDeviceInfo,
        GetCapabilitiesStringLength, GetDisplayConfigBufferSizes, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, QueryDisplayConfig, SetVCPFeature,
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
        DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
        DISPLAYCONFIG_TARGET_DEVICE_NAME, PHYSICAL_MONITOR, QDC_ONLY_ACTIVE_PATHS,
    };
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };

    use super::{parse_vcp, Choice, Level, Monitor, BRIGHTNESS, CONTRAST, INPUT, POWER, VOLUME};

    /// One DDC/CI conversation at a time.
    static BUS: Mutex<()> = Mutex::new(());
    /// Supported VCP codes per monitor id, from its capabilities string.
    static CAPS: Mutex<Option<HashMap<String, Vec<(u8, Vec<u32>)>>>> = Mutex::new(None);

    const MONITORINFOF_PRIMARY: u32 = 1;

    fn wide(s: &[u16]) -> String {
        String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
    }

    /// GDI device name (`\\.\DISPLAY1`) → EDID friendly name ("LS27B61x").
    fn friendly_names() -> HashMap<String, String> {
        let mut names = HashMap::new();
        unsafe {
            let (mut np, mut nm) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
                return names;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
            if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None)
                .is_err()
            {
                return names;
            }
            for path in &paths[..np as usize] {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                source.header.size = std::mem::size_of_val(&source) as u32;
                source.header.adapterId = path.sourceInfo.adapterId;
                source.header.id = path.sourceInfo.id;
                let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
                target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
                target.header.size = std::mem::size_of_val(&target) as u32;
                target.header.adapterId = path.targetInfo.adapterId;
                target.header.id = path.targetInfo.id;
                if DisplayConfigGetDeviceInfo(&mut source.header) == 0 && DisplayConfigGetDeviceInfo(&mut target.header) == 0 {
                    let name = wide(&target.monitorFriendlyDeviceName);
                    if !name.is_empty() {
                        names.insert(wide(&source.viewGdiDeviceName), name);
                    }
                }
            }
        }
        names
    }

    struct Screen {
        handle: HMONITOR,
        device: String,
        /// Desktop coordinates of the whole screen.
        rect: RECT,
        primary: bool,
    }

    fn screens() -> Vec<Screen> {
        unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
            let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
            list.push(m);
            true.into()
        }
        let mut handles: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut handles as *mut _ as isize));
        }
        let mut out: Vec<Screen> = handles
            .into_iter()
            .filter_map(|handle| {
                let mut info = MONITORINFOEXW::default();
                info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
                unsafe { GetMonitorInfoW(handle, &mut info.monitorInfo) }.ok().ok()?;
                Some(Screen {
                    handle,
                    device: wide(&info.szDevice),
                    rect: info.monitorInfo.rcMonitor,
                    primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                })
            })
            .collect();
        out.sort_by_key(|s| (s.rect.left, s.rect.top));
        out
    }

    /// Desktop rectangles of every screen, for covering them all.
    pub fn screen_rects() -> Vec<RECT> {
        screens().into_iter().map(|s| s.rect).collect()
    }

    /// GDI device name (`\\.\DISPLAY1`) of the screen showing most of `window`,
    /// the prefix of its monitors' ids.
    pub fn device_of(window: windows::Win32::Foundation::HWND) -> Option<String> {
        let screen = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        unsafe { GetMonitorInfoW(screen, &mut info.monitorInfo) }.ok().ok()?;
        Some(wide(&info.szDevice))
    }

    /// A monitor handle to read from a worker thread; the bus lock keeps it exclusive.
    #[derive(Clone, Copy)]
    struct Shared(PHYSICAL_MONITOR);
    unsafe impl Send for Shared {}
    unsafe impl Sync for Shared {}

    /// Physical monitors behind one screen; destroyed on drop.
    struct Physical(Vec<PHYSICAL_MONITOR>);

    impl Physical {
        fn of(screen: HMONITOR) -> Self {
            let mut n = 0u32;
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(screen, &mut n) }.is_err() || n == 0 {
                return Physical(Vec::new());
            }
            let mut list = vec![PHYSICAL_MONITOR::default(); n as usize];
            if unsafe { GetPhysicalMonitorsFromHMONITOR(screen, &mut list) }.is_err() {
                return Physical(Vec::new());
            }
            Physical(list)
        }
    }

    impl Drop for Physical {
        fn drop(&mut self) {
            if !self.0.is_empty() {
                let _ = unsafe { DestroyPhysicalMonitors(&self.0) };
            }
        }
    }

    fn capabilities(m: &PHYSICAL_MONITOR) -> Option<String> {
        for _ in 0..2 {
            let mut len = 0u32;
            if unsafe { GetCapabilitiesStringLength(m.hPhysicalMonitor, &mut len) } != 0 && len > 0 {
                let mut buf = vec![0u8; len as usize];
                if unsafe { CapabilitiesRequestAndCapabilitiesReply(m.hPhysicalMonitor, &mut buf) } != 0 {
                    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    return Some(String::from_utf8_lossy(&buf[..end]).into_owned());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }

    /// Current value and maximum, retried: monitors sometimes drop a reply.
    fn read(m: &PHYSICAL_MONITOR, code: u8) -> Option<(u32, u32)> {
        for _ in 0..3 {
            let (mut cur, mut max) = (0u32, 0u32);
            if unsafe { GetVCPFeatureAndVCPFeatureReply(m.hPhysicalMonitor, code, None, &mut cur, Some(&mut max)) } != 0 {
                return Some((cur, max));
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        None
    }

    fn monitor(id: String, name: String, primary: bool, m: &PHYSICAL_MONITOR, vcp: &[(u8, Vec<u32>)]) -> Monitor {
        let has = |code: u8| vcp.iter().find(|(c, _)| *c == code);
        let level = |code: u8| {
            has(code)?;
            let (value, max) = read(m, code)?;
            (max > 0).then_some(Level { value: value.min(max), max })
        };
        let choice = |code: u8| {
            let options = has(code)?.1.clone();
            let (current, _) = read(m, code)?;
            (!options.is_empty()).then_some(Choice { current, options })
        };
        Monitor {
            id,
            name,
            primary,
            brightness: level(BRIGHTNESS),
            contrast: level(CONTRAST),
            volume: level(VOLUME),
            input: choice(INPUT),
            power: choice(POWER),
        }
    }

    /// Calls `f` with the physical monitor for `id` (`<GDI device>#<index>`).
    fn each(mut f: impl FnMut(String, &Screen, &PHYSICAL_MONITOR)) {
        for screen in screens() {
            let physical = Physical::of(screen.handle);
            for (i, m) in physical.0.iter().enumerate() {
                f(format!("{}#{i}", screen.device), &screen, m);
            }
        }
    }

    pub fn list() -> Result<Vec<Monitor>, String> {
        let _bus = BUS.lock().unwrap_or_else(|e| e.into_inner());
        let names = friendly_names();
        let mut found: Vec<(String, String, bool, Shared)> = Vec::new();
        let mut keep = Vec::new();
        for screen in screens() {
            let physical = Physical::of(screen.handle);
            for (i, m) in physical.0.iter().enumerate() {
                let name = names.get(&screen.device).cloned().unwrap_or_else(|| {
                    // Packed struct: copy the field before borrowing it.
                    let description = m.szPhysicalMonitorDescription;
                    wide(&description)
                });
                found.push((format!("{}#{i}", screen.device), name, screen.primary, Shared(*m)));
            }
            keep.push(physical);
        }

        // Each monitor has its own I²C bus, so they can be read in parallel.
        let monitors = std::thread::scope(|s| {
            let jobs: Vec<_> = found
                .iter()
                .map(|(id, name, primary, m)| {
                    s.spawn(move || {
                        let key = format!("{id}|{name}");
                        let cached = CAPS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&key).cloned());
                        let vcp = match cached {
                            Some(v) => v,
                            None => {
                                let v = capabilities(&m.0).map(|c| parse_vcp(&c)).unwrap_or_default();
                                if !v.is_empty() {
                                    CAPS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(key, v.clone());
                                }
                                v
                            }
                        };
                        monitor(id.clone(), name.clone(), *primary, &m.0, &vcp)
                    })
                })
                .collect();
            jobs.into_iter().filter_map(|j| j.join().ok()).collect::<Vec<_>>()
        });
        drop(keep);
        Ok(monitors)
    }

    pub fn set(id: &str, code: u8, value: u32) -> Result<(), String> {
        let _bus = BUS.lock().unwrap_or_else(|e| e.into_inner());
        let mut result = Err("монитор не найден".to_string());
        each(|this, _, m| {
            if this == id {
                result = if unsafe { SetVCPFeature(m.hPhysicalMonitor, code, value) } != 0 {
                    Ok(())
                } else {
                    Err("монитор не принял команду".to_string())
                };
            }
        });
        result
    }
}

#[cfg(not(windows))]
mod win {
    use super::Monitor;

    pub fn list() -> Result<Vec<Monitor>, String> {
        Ok(Vec::new())
    }

    pub fn set(_id: &str, _code: u8, _value: u32) -> Result<(), String> {
        Err("только Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_capabilities_of_real_monitors() {
        let mi = "(prot(monitor)type(lcd)MStarcmds(01 02 03 07 0C E3 F3)vcp(02 04 05 08 10 12 14(05 08 0B 0C) 16 18 1A 52 60( 11 12 0F 10) AA(01 02) AC AE B2 B6 C6 C8 C9 D6(01 04 05) DC(00 02 03 05 ) DF FD)mccs_ver(2.1)mswhql(1))";
        let vcp = parse_vcp(mi);
        let get = |code: u8| vcp.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
        assert_eq!(get(0x10), Some(vec![]));
        assert_eq!(get(0x60), Some(vec![0x11, 0x12, 0x0F, 0x10]));
        assert_eq!(get(0xD6), Some(vec![1, 4, 5]));
        assert_eq!(get(0x62), None);

        let samsung = "(prot(monitor)type(lcd)model(SAMSUNG)cmds(01 02 03 07 0C 4E F3 E3)vcp(02 04 05 08 0B 0C 10 12 14(01 04 05 06 08 0B) 16 18 1A 6C 6E 70 AC AE B6 C0 C6 C8 C9 CA CC(00 02 03 04 05 06 07 08 09 0A 0B 0C 0D 1A 1E 24) D6(01 04) DF 60(0F 11 12 )62 8D FF 39 32 35)mswhql(1)mccs_ver(2.0)asset_eep(32)mpu_ver(01))";
        let vcp = parse_vcp(samsung);
        let get = |code: u8| vcp.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone());
        assert_eq!(get(0x60), Some(vec![0x0F, 0x11, 0x12]));
        assert_eq!(get(0x62), Some(vec![]));
        assert_eq!(get(0xD6), Some(vec![1, 4]));
    }

    /// Reads the monitors attached to this machine: `cargo test -- --ignored real_monitors --nocapture`.
    #[test]
    #[ignore]
    fn real_monitors() {
        for pass in 0..2 {
            let start = std::time::Instant::now();
            let list = win::list().unwrap();
            println!("pass {pass}: {:?}", start.elapsed());
            println!("{}", serde_json::to_string_pretty(&list).unwrap());
        }
    }

    /// Writes each monitor's current brightness back to it: exercises the
    /// write path without changing anything on screen.
    #[test]
    #[ignore]
    fn real_monitors_accept_writes() {
        for m in win::list().unwrap() {
            if let Some(b) = m.brightness {
                println!("{} ({}): {:?}", m.name, b.value, win::set(&m.id, BRIGHTNESS, b.value));
            }
        }
    }

    #[test]
    fn knows_only_whitelisted_features() {
        assert_eq!(feature_code("brightness"), Some(0x10));
        assert_eq!(feature_code("factory_reset"), None);
    }
}
