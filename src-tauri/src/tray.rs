use std::sync::OnceLock;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::panel;

static OPEN_ITEM: OnceLock<MenuItem<tauri::Wry>> = OnceLock::new();

fn open_label(shortcut: &str) -> String {
    format!("Открыть панель  ({shortcut})")
}

pub fn set_shortcut_label(shortcut: &str) {
    if let Some(item) = OPEN_ITEM.get() {
        let _ = item.set_text(open_label(shortcut));
    }
}

pub fn init(app: &AppHandle, shortcut: &str) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", open_label(shortcut), true, None::<&str>)?;
    let _ = OPEN_ITEM.set(open.clone());
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Dock Panel")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => panel::show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                panel::toggle(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
