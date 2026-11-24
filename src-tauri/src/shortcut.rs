//! Global "send clipboard to Claude" shortcut.

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::{commands::broadcast_settings, state::AppState};

/// Replace the registered shortcut with `accel` ("" = none).
pub fn register(app: &AppHandle, accel: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if accel.is_empty() {
        return Ok(());
    }
    gs.register(accel)
        .map_err(|e| format!("Couldn't register {accel}: {e}"))
}

/// Send the clipboard to Claude via the main window, without stealing focus.
pub fn on_pressed(app: &AppHandle) {
    let text = app.clipboard().read_text().unwrap_or_default();
    if text.trim().is_empty() {
        return;
    }
    // No show()/focus here: the panel is always visible, and making it key while
    // another app is active hid it until the request finished.
    let _ = app.emit_to("main", "hotkey-send", text);
}

/// Save + register a new global shortcut ("" disables it).
#[tauri::command]
pub fn set_shortcut(app: AppHandle, state: State<'_, AppState>, accel: String) -> Result<(), String> {
    let result = register(&app, &accel);
    state.settings.lock().unwrap().shortcut = accel;
    broadcast_settings(&app, &state);
    result
}

/// Unregister while the settings window records a new combination.
#[tauri::command]
pub fn pause_shortcut(app: AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}

#[tauri::command]
pub fn resume_shortcut(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let accel = state.settings.lock().unwrap().shortcut.clone();
    register(&app, &accel)
}
