use std::os::windows::process::CommandExt;

#[tauri::command]
pub fn power_action(action: String) -> Result<(), String> {
    if action == "sleep" {
        return sleep();
    }
    let root = std::env::var_os("SystemRoot").ok_or("Не найден каталог Windows")?;
    let system = std::path::PathBuf::from(root).join("System32");
    let (program, args): (&str, &[&str]) = match action.as_str() {
        "shutdown" => ("shutdown.exe", &["/s", "/t", "0"]),
        "restart" => ("shutdown.exe", &["/r", "/t", "0"]),
        "lock" => ("rundll32.exe", &["user32.dll,LockWorkStation"]),
        _ => return Err("Неизвестное действие питания".into()),
    };
    let result = std::process::Command::new(system.join(program))
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Не удалось выполнить действие: {e}"))?;
    if result.status.success() {
        Ok(())
    } else {
        Err("Windows не смогла выполнить действие питания".into())
    }
}

fn sleep() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| e.to_string())?;
        let result = (|| {
            let mut luid = LUID::default();
            LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid)
                .map_err(|e| e.to_string())?;
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let mut previous = TOKEN_PRIVILEGES::default();
            AdjustTokenPrivileges(
                token,
                false,
                Some(&privileges),
                std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                Some(&mut previous),
                None,
            )
            .map_err(|e| e.to_string())?;
            if GetLastError() == ERROR_NOT_ALL_ASSIGNED {
                return Err("Windows не разрешила переход в спящий режим".into());
            }
            let suspended = SetSuspendState(false, false, false);
            let _ = AdjustTokenPrivileges(token, false, Some(&previous), 0, None, None);
            if suspended {
                Ok(())
            } else {
                Err("Не удалось перейти в спящий режим".into())
            }
        })();
        let _ = CloseHandle(token);
        result
    }
}
