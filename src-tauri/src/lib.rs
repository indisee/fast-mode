//! Fast Mode: a floating always-on-top panel that sends text to `claude -p`
//! with a system prompt and puts the reply on the clipboard.
//!
//! - `state`    settings, projects, history on disk
//! - `claude`   running the CLI and parsing its reply
//! - `account`  sign-in status, login, usage page
//! - `commands` settings / projects / history commands for the frontend
//! - `shortcut` the global "send clipboard" hotkey
//! - `window`   the panel's size, position and opacity
//! - `platform` OS-specific code (only macOS is tested)

mod account;
mod claude;
mod commands;
mod platform;
mod shortcut;
mod state;
mod window;

use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::{state::AppState, window::COMPACT};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        shortcut::on_pressed(app);
                    }
                })
                .build(),
        )
        .manage(AppState::load())
        .setup(|app| {
            platform::setup(app);

            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            {
                let mut s = state.settings.lock().unwrap();
                if s.home.is_none() {
                    s.home = Some(window::default_home(&handle));
                }
            }
            let accel = state.settings.lock().unwrap().shortcut.clone();
            if let Err(e) = shortcut::register(&handle, &accel) {
                eprintln!("{e}");
            }

            window::build_main(app)?;

            if let Some(win) = app.get_webview_window("main") {
                let _ = window::resize(handle.clone(), state, COMPACT, COMPACT);
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(())
        })
        .on_window_event(window::on_event)
        .invoke_handler(tauri::generate_handler![
            claude::ask,
            claude::cancel,
            account::auth_status,
            account::login,
            account::open_usage,
            commands::get_state,
            commands::copy_text,
            commands::delete_history,
            commands::clear_history,
            commands::set_model,
            commands::set_opacity,
            commands::save_projects,
            commands::set_active_project,
            commands::new_project_id,
            commands::open_settings,
            commands::quit,
            shortcut::set_shortcut,
            shortcut::pause_shortcut,
            shortcut::resume_shortcut,
            window::resize,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fast Mode");
}
