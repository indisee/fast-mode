//! Commands for settings, projects, history and app windows.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    state::{new_id, AppState, HistoryItem, Project, Settings},
    window::{apply_alpha, COMPACT},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    settings: Settings,
    projects: Vec<Project>,
    history: Vec<HistoryItem>,
    compact: f64,
    platform: &'static str,
}

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Snapshot {
    Snapshot {
        settings: state.settings.lock().unwrap().clone(),
        projects: state.projects.lock().unwrap().clone(),
        history: state.history.lock().unwrap().clone(),
        compact: COMPACT,
        platform: std::env::consts::OS,
    }
}

/// Save settings and tell every window.
pub fn broadcast_settings(app: &AppHandle, state: &AppState) {
    state.save_settings();
    let _ = app.emit("settings", state.settings.lock().unwrap().clone());
}

// ── history ──

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_history(state: State<'_, AppState>, id: String) -> Vec<HistoryItem> {
    let h = {
        let mut h = state.history.lock().unwrap();
        h.retain(|i| i.id != id);
        h.clone()
    };
    state.save_history();
    h
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) {
    state.history.lock().unwrap().clear();
    state.save_history();
}

// ── settings ──

#[tauri::command]
pub fn set_model(app: AppHandle, state: State<'_, AppState>, model: String) {
    state.settings.lock().unwrap().model = model;
    broadcast_settings(&app, &state);
}

#[tauri::command]
pub fn set_opacity(app: AppHandle, state: State<'_, AppState>, value: f64) {
    state.settings.lock().unwrap().inactive_opacity = value.clamp(0.2, 1.0);
    broadcast_settings(&app, &state);
    if let Some(win) = app.get_webview_window("main") {
        let focused = win.is_focused().unwrap_or(true);
        apply_alpha(&app, focused);
    }
}

// ── projects ──

/// Replace the whole project list (Settings window edits).
#[tauri::command]
pub fn save_projects(app: AppHandle, state: State<'_, AppState>, projects: Vec<Project>) -> Result<(), String> {
    if projects.is_empty() {
        return Err("Keep at least one project".into());
    }
    let ids_ok = {
        let active = state.settings.lock().unwrap().active_project.clone();
        projects.iter().any(|p| p.id == active)
    };
    if !ids_ok {
        state.settings.lock().unwrap().active_project = projects[0].id.clone();
        broadcast_settings(&app, &state);
    }
    *state.projects.lock().unwrap() = projects.clone();
    state.save_projects();
    let _ = app.emit("projects", projects);
    Ok(())
}

#[tauri::command]
pub fn set_active_project(app: AppHandle, state: State<'_, AppState>, id: String) {
    if state.projects.lock().unwrap().iter().any(|p| p.id == id) {
        state.settings.lock().unwrap().active_project = id;
        broadcast_settings(&app, &state);
    }
}

#[tauri::command]
pub fn new_project_id() -> String {
    new_id()
}

// ── app ──

#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        return w.set_focus().map_err(|e| e.to_string());
    }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Fast Mode Settings")
        .inner_size(560.0, 640.0)
        .min_inner_size(460.0, 480.0)
        .resizable(true)
        .minimizable(false)
        .always_on_top(true)
        .center()
        .build()
        .map(|w| {
            let _ = w.set_focus();
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
