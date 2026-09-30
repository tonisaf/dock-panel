//! CPU, memory, system drive and battery for the System widget.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Disks, System};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    /// Average across cores, 0–100. Needs two samples, so the first call reads 0.
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub battery: Option<Battery>,
}

/// CPU usage is a delta between refreshes, so the `System` must outlive calls.
static SYSTEM: Mutex<Option<System>> = Mutex::new(None);
/// The disk list, made once, and when its space was last read: disk use moves
/// slowly, and the widget asks every 2 s.
static DISKS: Mutex<Option<(Disks, Instant)>> = Mutex::new(None);
const DISK_EVERY: Duration = Duration::from_secs(30);

#[tauri::command]
pub async fn system_stats() -> Result<SystemStats, String> {
    tauri::async_runtime::spawn_blocking(collect)
        .await
        .map_err(|e| e.to_string())
}

fn collect() -> SystemStats {
    let (cpu, mem_used, mem_total) = {
        let mut guard = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
        let sys = guard.get_or_insert_with(System::new);
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        (
            sys.global_cpu_usage(),
            sys.used_memory(),
            sys.total_memory(),
        )
    };

    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let mut guard = DISKS.lock().unwrap_or_else(|e| e.into_inner());
    let (disks, read_at) =
        guard.get_or_insert_with(|| (Disks::new_with_refreshed_list(), Instant::now()));
    if read_at.elapsed() >= DISK_EVERY {
        disks.refresh(false);
        *read_at = Instant::now();
    }
    let (disk_used, disk_total) = disks
        .iter()
        .find(|d| d.mount_point().to_string_lossy().starts_with(&system_drive))
        .map(|d| (d.total_space() - d.available_space(), d.total_space()))
        .unwrap_or((0, 0));

    SystemStats {
        cpu,
        mem_used,
        mem_total,
        disk_used,
        disk_total,
        battery: battery(),
    }
}

#[cfg(windows)]
fn battery() -> Option<Battery> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    const NO_BATTERY: u8 = 128;
    const UNKNOWN: u8 = 255;

    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    if status.BatteryFlag & NO_BATTERY != 0 || status.BatteryLifePercent == UNKNOWN {
        return None;
    }
    Some(Battery {
        percent: status.BatteryLifePercent,
        charging: status.ACLineStatus == 1,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccentColors {
    /// For dark backgrounds.
    light: String,
    /// For light backgrounds.
    dark: String,
}

/// The Windows accent color (Settings → Personalization → Colors), in shades
/// readable on dark and on light backgrounds.
#[tauri::command]
pub fn system_accent() -> Result<AccentColors, String> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let settings = UISettings::new().map_err(|e| e.to_string())?;
    let hex = |kind: UIColorType| -> Result<String, String> {
        let c = settings.GetColorValue(kind).map_err(|e| e.to_string())?;
        Ok(format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
    };
    Ok(AccentColors {
        light: hex(UIColorType::AccentLight2)?,
        dark: hex(UIColorType::AccentDark1)?,
    })
}
