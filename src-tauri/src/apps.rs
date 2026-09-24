//! Installed applications: enumeration of the shell AppsFolder (the same
//! source the Start menu uses: Win32 apps and Store apps alike), icon
//! extraction and launching.
//!
//! Icons are served lazily through the `appicon` URI scheme so the list
//! itself stays small; a single STA worker renders them and caches PNGs on disk.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::OnceLock;

use percent_encoding::percent_decode_str;
use serde::Serialize;
use tauri::http::{header, Request, Response};
use tauri::UriSchemeResponder;

const ICON_SIZE: i32 = 64;

#[derive(Serialize, Clone)]
pub struct AppEntry {
    /// AppsFolder parsing name: an AUMID for Store apps, a known-folder path otherwise.
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub async fn list_apps() -> Result<Vec<AppEntry>, String> {
    let raw = on_sta(win::enumerate).map_err(|e| e.to_string())?;

    let mut apps: Vec<AppEntry> = raw
        .into_iter()
        .filter(|(id, name)| is_launchable(id, name))
        .map(|(id, name)| AppEntry { id, name })
        .collect();
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.id == b.id);
    Ok(apps)
}

#[tauri::command]
pub async fn launch_app(id: String) -> Result<(), String> {
    if on_sta(move || win::launch(&id)) {
        Ok(())
    } else {
        Err("не удалось запустить приложение".into())
    }
}

/// AppsFolder also lists uninstallers, readmes and web links next to real apps.
fn is_launchable(id: &str, name: &str) -> bool {
    const NOISE: [&str; 6] = ["uninstall", "удалить", "удаление", "деинсталл", "readme", "release notes"];
    const DOC_EXT: [&str; 9] = [".url", ".chm", ".txt", ".htm", ".html", ".pdf", ".rtf", ".md", ".ini"];

    let name = name.to_lowercase();
    let id = id.to_lowercase();
    !NOISE.iter().any(|w| name.contains(w))
        && !id.starts_with("http:")
        && !id.starts_with("https:")
        && !DOC_EXT.iter().any(|e| id.ends_with(e))
}

/// Shell objects want a single-threaded apartment; run `f` on a fresh STA thread.
fn on_sta<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::spawn(move || {
        let _com = win::ComGuard::new();
        f()
    })
    .join()
    .expect("STA worker panicked")
}

// ---- icons -----------------------------------------------------------------

type IconJob = (String, UriSchemeResponder);
static ICON_QUEUE: OnceLock<mpsc::Sender<IconJob>> = OnceLock::new();

pub fn start_icon_worker(cache_dir: PathBuf) {
    let (tx, rx) = mpsc::channel::<IconJob>();
    if ICON_QUEUE.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        let _com = win::ComGuard::new();
        let _ = std::fs::create_dir_all(&cache_dir);

        for (id, responder) in rx {
            let file = cache_dir.join(format!("{:016x}-{ICON_SIZE}.png", fnv1a(&id)));
            let png = std::fs::read(&file).ok().or_else(|| {
                let png = win::icon_png(&id, ICON_SIZE)?;
                let _ = std::fs::write(&file, &png);
                Some(png)
            });
            responder.respond(icon_response(png));
        }
    });
}

/// Handler for `appicon://<percent-encoded app id>`.
pub fn handle_icon_request(request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let raw = request.uri().path().trim_start_matches('/');
    let id = percent_decode_str(raw).decode_utf8_lossy().into_owned();
    match ICON_QUEUE.get() {
        Some(queue) => {
            if let Err(mpsc::SendError((_, responder))) = queue.send((id, responder)) {
                responder.respond(icon_response(None));
            }
        }
        None => responder.respond(icon_response(None)),
    }
}

fn icon_response(png: Option<Vec<u8>>) -> Response<Vec<u8>> {
    let builder = Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    match png {
        Some(bytes) => builder
            .status(200)
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "max-age=604800")
            .body(bytes),
        None => builder.status(404).body(Vec::new()),
    }
    .expect("static response parts are valid")
}

/// Stable across builds (unlike `DefaultHasher`), so the disk cache survives upgrades.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Premultiplied BGRA (what GDI hands back) to straight RGBA, in place.
fn bgra_premul_to_rgba(px: &mut [u8]) {
    // Legacy 24-bit icons come back with an all-zero alpha channel.
    let opaque = px.chunks_exact(4).all(|p| p[3] == 0);
    for p in px.chunks_exact_mut(4) {
        let (b, g, r) = (p[0] as u32, p[1] as u32, p[2] as u32);
        let a = if opaque { 255 } else { p[3] as u32 };
        let un = |c: u32| if a == 0 || a == 255 { c } else { ((c * 255 + a / 2) / a).min(255) };
        p[0] = un(r) as u8;
        p[1] = un(g) as u8;
        p[2] = un(b) as u8;
        p[3] = a as u8;
    }
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

#[cfg(windows)]
mod win {
    use std::mem::size_of;

    use windows::core::{w, Result, HSTRING, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, IEnumShellItems, IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName,
        ShellExecuteW, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    pub struct ComGuard;

    impl ComGuard {
        pub fn new() -> Self {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            }
            ComGuard
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }

    unsafe fn display_name(item: &IShellItem, kind: SIGDN) -> Result<String> {
        let p = item.GetDisplayName(kind)?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        Ok(s)
    }

    /// `(parsing name, display name)` for every entry in `shell:AppsFolder`.
    pub fn enumerate() -> Result<Vec<(String, String)>> {
        unsafe {
            let folder: IShellItem = SHCreateItemFromParsingName(w!("shell:AppsFolder"), None)?;
            let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;

            let mut out = Vec::new();
            loop {
                let mut batch = [None];
                let mut fetched = 0u32;
                if items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                let Some(item) = batch[0].take() else { break };
                if let (Ok(id), Ok(name)) = (
                    display_name(&item, SIGDN_PARENTRELATIVEPARSING),
                    display_name(&item, SIGDN_NORMALDISPLAY),
                ) {
                    out.push((id, name));
                }
            }
            Ok(out)
        }
    }

    pub fn launch(id: &str) -> bool {
        let target = HSTRING::from(format!("shell:AppsFolder\\{id}"));
        let result = unsafe {
            ShellExecuteW(None, w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
        };
        // ShellExecute reports success as a value greater than 32.
        result.0 as isize > 32
    }

    pub fn icon_png(id: &str, size: i32) -> Option<Vec<u8>> {
        unsafe {
            let path = HSTRING::from(format!("shell:AppsFolder\\{id}"));
            let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&path, None).ok()?;
            let hbmp = factory
                .GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
                .ok()?;
            let pixels = read_bitmap(hbmp);
            let _ = DeleteObject(HGDIOBJ(hbmp.0));

            let (width, height, mut px) = pixels?;
            super::bgra_premul_to_rgba(&mut px);
            super::encode_png(width, height, &px)
        }
    }

    /// Top-down 32-bit BGRA pixels of a GDI bitmap.
    unsafe fn read_bitmap(hbmp: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hbmp.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as _)) == 0 {
            return None;
        }
        let (width, height) = (bm.bmWidth, bm.bmHeight.abs());

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative: top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (width * height * 4) as usize];

        let hdc = GetDC(None);
        let lines = GetDIBits(hdc, hbmp, 0, height as u32, Some(buf.as_mut_ptr() as _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, hdc);

        (lines == height).then_some((width as u32, height as u32, buf))
    }
}
