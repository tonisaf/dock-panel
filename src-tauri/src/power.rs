use std::os::windows::process::CommandExt;

#[tauri::command]
pub fn power_action(action: String) -> Result<(), String> {
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
