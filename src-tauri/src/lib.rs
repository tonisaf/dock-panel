mod agents;
mod ai_limits;
mod ask;
mod alerts;
mod apps;
mod calendar;
mod desktop;
mod discord;
mod files;
mod gcal;
mod home;
mod bionic;
mod listening;
mod lmctl;
mod lmstudio;
mod claude_web;
mod mail;
mod media;
mod monitors;
mod net;
mod notes;
mod notion;
mod oauth;
mod openclaw;
mod panel;
mod player;
mod rates;
mod pomodoro;
mod secrets;
mod spotify;
mod system;
mod taskbar;
mod tray;
mod trust;
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
                // The panel toggle, and the Discord microphone toggle if set.
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if discord::is_mute_shortcut(shortcut) {
                        discord::toggle_mute();
                    } else {
                        panel::toggle(app);
                    }
                })
                .build(),
        )
        .register_uri_scheme_protocol("noteimg", |_ctx, request| notes::image_response(&request))
        .register_asynchronous_uri_scheme_protocol("appicon", |_ctx, request, responder| {
            apps::handle_icon_request(request, responder);
        })
        .setup(move |app| {
            let handle = app.handle();
            if let Ok(dir) = app.path().app_data_dir() {
                trust::load_pins(&dir.join("prefs.json"));
            }
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
            discord::init(handle);
            pomodoro::init(handle);
            listening::init(handle);
            notes::init(handle);
            files::init();
            updater::init(handle);
            desktop::init(handle);
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
            panel::panel_set_fullscreen,
            panel::panel_fullscreen,
            panel::panel_set_extra_width,
            panel::panel_set_pinned,
            panel::panel_set_taskbar_button,
            panel::panel_set_taskbar_player,
            panel::panel_set_taskbar_mail,
            panel::panel_set_taskbar_tasks,
            panel::panel_set_taskbar_agents,
            panel::panel_set_taskbar_mic,
            panel::panel_set_taskbar_pomodoro,
            panel::panel_open,
            desktop::desktop_widgets,
            desktop::desktop_set,
            desktop::desktop_fit,
            desktop::desktop_drag,
            desktop::desktop_grid_prepare,
            desktop::desktop_backdrop,
            desktop::desktop_menu,
            notes::notes_state,
            notes::notes_set_source,
            notes::notes_sync,
            notes::notes_page,
            notes::notes_toggle,
            notes::notes_create,
            notes::notes_text,
            notes::notes_edit,
            notes::notes_set_props,
            notes::notes_search,
            pomodoro::pomodoro_state,
            pomodoro::pomodoro_start,
            pomodoro::pomodoro_pause,
            pomodoro::pomodoro_reset,
            pomodoro::pomodoro_skip,
            pomodoro::pomodoro_set_phase,
            pomodoro::pomodoro_set_settings,
            apps::list_apps,
            apps::launch_app,
            apps::app_info,
            apps::launch_app_admin,
            apps::open_recent,
            files::files_search,
            rates::currency_rates,
            apps::pick_files,
            media::media_now_playing,
            media::media_watch,
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
            ai_limits::ai_limits_refresh,
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
            notion::notion_task_schema,
            notion::notion_update,
            notion::notion_delete,
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
            spotify::spotify_recent,
            spotify::spotify_top,
            listening::listening_summary,
            weather::weather_forecast,
            weather::weather_geocode,
            home::home_state,
            home::home_lamp_set,
            home::home_speaker_control,
            home::home_rename,
            home::home_forget,
            home::yandex::yandex_login,
            home::yandex::yandex_status,
            home::yandex::yandex_logout,
            monitors::monitors_list,
            monitors::monitor_set,
            monitors::monitors_blackout,
            agents::agents_list,
            ask::ask_start,
            bionic::bionic_projects,
            bionic::bionic_session,
            openclaw::openclaw_status,
            openclaw::openclaw_set_token,
            openclaw::openclaw_disconnect,
            lmstudio::lmstudio_status,
            lmstudio::lmstudio_set_model,
            lmstudio::llm_run,
            lmctl::lmstudio_overview,
            lmctl::lmstudio_load,
            lmctl::lmstudio_unload,
            lmctl::lmstudio_server,
            player::player_play,
            player::player_available,
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
            discord::discord_state,
            discord::discord_login,
            discord::discord_logout,
            discord::discord_guilds,
            discord::discord_channels,
            discord::discord_set_watched,
            discord::discord_set_notify,
            discord::discord_voice,
            discord::discord_join,
            discord::discord_set_mute_shortcut,
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
