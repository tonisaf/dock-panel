//! Status and on/off for the two VPN clients on this machine.
//!
//! - OpenVPN Connect: its CLI (`--connect-shortcut=<profile>` /
//!   `--disconnect-shortcut`). The profile's password isn't saved, so
//!   connecting brings up the app's own password prompt.
//! - AmneziaVPN: no CLI; the tunnel is the Windows service
//!   `AmneziaWGTunnel$AmneziaVPN`. Stopping it needs admin, so disconnecting
//!   runs `sc.exe` elevated (one UAC prompt). Amnezia deletes that service once
//!   the tunnel goes down, so connecting normally means bringing the Amnezia
//!   window forward for its own "Connect" button.
//!
//! Connection state comes from the tunnel network adapters, which needs no rights.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use serde::Serialize;

const OPENVPN_EXE: &str = r"OpenVPN Connect\OpenVPNConnect.exe";
const AMNEZIA_EXE: &str = r"AmneziaVPN\AmneziaVPN.exe";
const AMNEZIA_SERVICE: &str = "AmneziaWGTunnel$AmneziaVPN";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn program_files(rel: &str) -> PathBuf {
    let base = std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
    PathBuf::from(base).join(rel)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnInfo {
    id: &'static str,
    name: &'static str,
    installed: bool,
    connected: bool,
    /// Profile or tunnel name shown under the title.
    detail: Option<String>,
    /// For Amnezia: whether the tunnel service exists (it may be removed when
    /// disconnecting from the Amnezia app itself).
    can_toggle: bool,
}

#[derive(Clone)]
struct OpenVpnProfile {
    id: String,
    name: String,
}

static OPENVPN_PROFILE: Mutex<Option<OpenVpnProfile>> = Mutex::new(None);

/// First profile from `--list-profiles`, cached for the session.
fn openvpn_profile() -> Option<OpenVpnProfile> {
    if let Some(p) = OPENVPN_PROFILE.lock().ok()?.clone() {
        return Some(p);
    }
    let out = Command::new(program_files(OPENVPN_EXE))
        .arg("--list-profiles")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let first = list.as_array()?.first()?;
    let profile = OpenVpnProfile {
        id: first["id"].as_str()?.to_string(),
        name: first["name"].as_str().unwrap_or("OpenVPN").to_string(),
    };
    *OPENVPN_PROFILE.lock().ok()? = Some(profile.clone());
    Some(profile)
}

#[tauri::command]
pub async fn vpn_status() -> Vec<VpnInfo> {
    tauri::async_runtime::spawn_blocking(|| {
        let adapters = win::adapters_up();
        let up = |pred: &dyn Fn(&str, &str) -> bool| adapters.iter().any(|(name, desc)| pred(name, desc));

        let openvpn_installed = program_files(OPENVPN_EXE).exists();
        let profile = if openvpn_installed { openvpn_profile() } else { None };
        let amnezia_installed = program_files(AMNEZIA_EXE).exists();

        vec![
            VpnInfo {
                id: "openvpn",
                name: "OpenVPN",
                installed: openvpn_installed,
                connected: up(&|_, d| d.contains("OpenVPN Connect") || d.contains("OpenVPN Data Channel Offload")),
                // Profile names are often "user@host"; show only the user part.
                detail: profile.as_ref().map(|p| p.name.split('@').next().unwrap_or(&p.name).to_string()),
                can_toggle: profile.is_some(),
            },
            VpnInfo {
                id: "amnezia",
                name: "AmneziaVPN",
                installed: amnezia_installed,
                connected: up(&|n, _| n == "AmneziaVPN"),
                detail: None,
                can_toggle: win::service_exists(AMNEZIA_SERVICE),
            },
        ]
    })
    .await
    .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToggleOutcome {
    /// The command ran; status will follow shortly.
    Done,
    /// Handed to the VPN app (password prompt or no tunnel service).
    Opened,
}

#[tauri::command]
pub async fn vpn_toggle(id: String, connect: bool) -> Result<ToggleOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || match id.as_str() {
        "openvpn" => {
            let exe = program_files(OPENVPN_EXE);
            let arg = if connect {
                let profile = openvpn_profile().ok_or("В OpenVPN Connect нет профиля")?;
                format!("--connect-shortcut={}", profile.id)
            } else {
                "--disconnect-shortcut".to_string()
            };
            Command::new(exe).arg(arg).creation_flags(CREATE_NO_WINDOW).spawn().map_err(|e| e.to_string())?;
            Ok(if connect { ToggleOutcome::Opened } else { ToggleOutcome::Done })
        }
        "amnezia" => {
            if connect && !win::service_exists(AMNEZIA_SERVICE) {
                open_app("amnezia")?;
                return Ok(ToggleOutcome::Opened);
            }
            let verb = if connect { "start" } else { "stop" };
            win::run_elevated("sc.exe", &format!("{verb} \"{AMNEZIA_SERVICE}\""))?;
            Ok(ToggleOutcome::Done)
        }
        _ => Err(format!("unknown vpn {id}")),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Brings the running app's window forward (both apps hide in the tray, and a
/// second launch doesn't show them), or starts the app if it isn't running.
fn open_app(id: &str) -> Result<(), String> {
    let exe = match id {
        "openvpn" => program_files(OPENVPN_EXE),
        "amnezia" => program_files(AMNEZIA_EXE),
        _ => return Err(format!("unknown vpn {id}")),
    };
    if win::focus_app_window(&exe) {
        return Ok(());
    }
    Command::new(exe).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn vpn_open(id: String) -> Result<(), String> {
    open_app(&id)
}

mod win {
    use windows::core::{w, HSTRING, PCWSTR};
    use std::path::Path;

    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_BUFFER_OVERFLOW, ERROR_CANCELLED, HWND, LPARAM, WAIT_OBJECT_0};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, SetForegroundWindow, ShowWindow,
        GW_OWNER, SW_HIDE, SW_RESTORE, SW_SHOW,
    };

    fn process_image(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Shows and focuses the main window of the running `exe`, if any.
    pub fn focus_app_window(exe: &Path) -> bool {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let list = &mut *(lparam.0 as *mut Vec<HWND>);
            // Titled, unowned top-level windows are app main windows (hidden ones included).
            if GetWindow(hwnd, GW_OWNER).map_or(true, |o| o.is_invalid()) && GetWindowTextLengthW(hwnd) > 0 {
                list.push(hwnd);
            }
            true.into()
        }

        let target = exe.to_string_lossy().to_lowercase();
        let mut windows: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
        }
        let found = windows.into_iter().find(|&hwnd| {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            process_image(pid).is_some_and(|img| img.to_lowercase() == target)
        });
        let Some(hwnd) = found else { return false };
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        true
    }

    /// `(friendly name, description)` of every adapter that is up.
    pub fn adapters_up() -> Vec<(String, String)> {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size: u32 = 16 * 1024;
        let mut buf: Vec<u64> = Vec::new(); // u64 keeps the struct alignment
        for _ in 0..3 {
            buf.resize((size as usize).div_ceil(8), 0);
            let ret = unsafe {
                GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(buf.as_mut_ptr().cast()), &mut size)
            };
            if ret == ERROR_BUFFER_OVERFLOW.0 {
                continue;
            }
            if ret != 0 {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
            while !p.is_null() {
                let a = unsafe { &*p };
                if a.OperStatus == IfOperStatusUp {
                    let name = unsafe { a.FriendlyName.to_string() }.unwrap_or_default();
                    let desc = unsafe { a.Description.to_string() }.unwrap_or_default();
                    out.push((name, desc));
                }
                p = a.Next;
            }
            return out;
        }
        Vec::new()
    }

    pub fn service_exists(name: &str) -> bool {
        unsafe {
            let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) else {
                return false;
            };
            let exists = match OpenServiceW(scm, &HSTRING::from(name), SERVICE_QUERY_STATUS) {
                Ok(svc) => {
                    let _ = CloseServiceHandle(svc);
                    true
                }
                Err(_) => false,
            };
            let _ = CloseServiceHandle(scm);
            exists
        }
    }

    /// Runs `file args` elevated (UAC prompt) with no window and waits for it.
    pub fn run_elevated(file: &str, args: &str) -> Result<(), String> {
        let file = HSTRING::from(file);
        let args = HSTRING::from(args);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(args.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        unsafe {
            if let Err(e) = ShellExecuteExW(&mut info) {
                return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
                    "Отменено в окне UAC".into()
                } else {
                    e.message()
                });
            }
            if info.hProcess.is_invalid() {
                return Ok(());
            }
            let waited = WaitForSingleObject(info.hProcess, 30_000);
            let mut code = 0u32;
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            match (waited == WAIT_OBJECT_0, code) {
                (false, _) => Err("Служба не ответила за 30 секунд".into()),
                // 1056: already running, 1062: not started — the goal state already holds.
                (true, 0 | 1056 | 1062) => Ok(()),
                (true, code) => Err(format!("sc.exe завершился с кодом {code}")),
            }
        }
    }
}
