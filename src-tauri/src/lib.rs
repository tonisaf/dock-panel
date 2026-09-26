mod agents;
mod ai_limits;
mod ask;
mod alerts;
mod apps;
mod calendar;
mod gcal;
mod home;
mod claude_web;
mod mail;
mod media;
mod monitors;
mod net;
mod notion;
mod oauth;
mod panel;
mod secrets;
mod spotify;
mod system;
mod taskbar;
mod tray;
mod updater;
mod vpn;
mod weather;
mod youtube;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch just opens the running panel.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            panel::show(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                // The panel toggle is the only global shortcut we register.
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        panel::toggle(app);
                    }
                })
                .build(),
        )
        .register_asynchronous_uri_scheme_protocol("appicon", |_ctx, request, responder| {
            apps::handle_icon_request(request, responder);
        })
        .setup(move |app| {
            let handle = app.handle();
            let shortcut = panel::init(handle);
            tray::init(handle, &shortcut)?;
            taskbar::start(handle);
            monitors::warm_up();
            claude_web::init(handle);
            alerts::init(handle);
            mail::init(handle);
            gcal::init();
            agents::init(handle);
            youtube::init(handle);
            updater::init(handle);
            apps::start_icon_worker(app.path().app_cache_dir()?.join("icons"));
            let registered = shortcut
                .parse::<Shortcut>()
                .map_err(|e| e.to_string())
                .and_then(|sc| app.global_shortcut().register(sc).map_err(|e| e.to_string()));
            if let Err(e) = registered {
                eprintln!("{shortcut} is unavailable: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            panel::hide_panel,
            panel::panel_settings,
            panel::panel_set_edge,
            panel::panel_set_shortcut,
            panel::panel_suspend_shortcut,
            panel::panel_set_width,
            panel::panel_set_extra_width,
            panel::panel_set_pinned,
            panel::panel_set_taskbar_button,
            panel::panel_set_taskbar_player,
            panel::panel_set_taskbar_mail,
            panel::panel_set_taskbar_tasks,
            panel::panel_set_taskbar_agents,
            apps::list_apps,
            apps::launch_app,
            apps::pick_files,
            media::media_now_playing,
            media::media_thumbnail,
            media::media_control,
            media::media_seek,
            system::system_stats,
            system::system_accent,
            ai_limits::ai_limits,
            ai_limits::claude_connect,
            ai_limits::claude_disconnect,
            claude_web::claude_web_login,
            claude_web::claude_web_refresh,
            claude_web::claude_web_logout,
            alerts::ai_alerts_set,
            notion::notion_status,
            notion::notion_set_token,
            notion::notion_disconnect,
            notion::notion_list_sources,
            notion::notion_tasks,
            notion::notion_complete,
            notion::notion_restore,
            notion::notion_create,
            gcal::gcal_login,
            gcal::gcal_status,
            gcal::gcal_logout,
            gcal::gcal_calendars,
            gcal::gcal_set_visible,
            gcal::gcal_events,
            gcal::gcal_create,
            gcal::gcal_update,
            gcal::gcal_delete,
            gcal::gcal_tasks,
            gcal::gcal_task_done,
            gcal::gcal_task_lists,
            gcal::gcal_task_create,
            calendar::calendar_list,
            calendar::calendar_add,
            calendar::calendar_remove,
            calendar::calendar_ics,
            spotify::spotify_login,
            spotify::spotify_status,
            spotify::spotify_logout,
            spotify::spotify_library,
            spotify::spotify_play,
            spotify::spotify_current,
            spotify::spotify_set_liked,
            spotify::spotify_devices,
            spotify::spotify_transfer,
            spotify::spotify_search,
            spotify::spotify_queue,
            spotify::spotify_player,
            spotify::spotify_seek,
            spotify::spotify_volume,
            spotify::spotify_shuffle,
            spotify::spotify_repeat,
            spotify::spotify_up_next,
            spotify::spotify_skip,
            spotify::spotify_add_to_playlist,
            spotify::spotify_image,
            weather::weather_forecast,
            weather::weather_geocode,
            home::home_state,
            home::home_lamp_set,
            home::home_speaker_control,
            home::home_rename,
            home::home_forget,
            monitors::monitors_list,
            monitors::monitor_set,
            monitors::monitors_blackout,
            agents::agents_list,
            ask::ask_start,
            ask::ask_cancel,
            ask::clipboard_text,
            agents::agents_dismiss,
            agents::agents_set_notify,
            agents::agents_focus,
            monitors::monitors_blackout_end,
            monitors::monitors_blackout_active,
            mail::mail_settings,
            mail::mail_add,
            mail::mail_remove,
            mail::mail_set_notify,
            mail::mail_list,
            mail::mail_open,
            mail::mail_action,
            mail::mail_unread,
            mail::mail_refresh,
            youtube::youtube_settings,
            youtube::youtube_add,
            youtube::youtube_import,
            youtube::youtube_remove,
            youtube::youtube_set_options,
            youtube::youtube_set_watched,
            youtube::youtube_feed,
            vpn::vpn_status,
            vpn::vpn_toggle,
            vpn::vpn_open,
            updater::update_status,
            updater::update_check,
            updater::update_install,
            updater::update_set_token,
            updater::update_clear_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
