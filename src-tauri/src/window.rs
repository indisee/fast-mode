//! The floating panel: creation, size/position, inactive opacity.

use tauri::{AppHandle, LogicalPosition, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use std::sync::atomic::Ordering;

use crate::{platform, state::AppState};

/// Side of the idle (compact) panel, in logical pixels.
pub const COMPACT: f64 = 70.0;

/// Resize the panel, keeping the user's top-left (`home`) unless that would push it
/// off screen — then it grows upward / leftward instead. Shrinking returns it home.
#[tauri::command]
pub fn resize(app: AppHandle, state: State<'_, AppState>, width: f64, height: f64) -> Result<(), String> {
    let win = app.get_webview_window("main").ok_or("no main window")?;
    let scale = win.scale_factor().map_err(|e| e.to_string())?;
    let (mut x, mut y) = state.settings.lock().unwrap().home.unwrap_or((100.0, 100.0));

    let monitor = win
        .monitor_from_point(x, y)
        .ok()
        .flatten()
        .or_else(|| win.current_monitor().ok().flatten());
    if let Some(m) = monitor {
        let wa = m.work_area();
        let ms = m.scale_factor();
        let (left, top) = (wa.position.x as f64 / ms, wa.position.y as f64 / ms);
        let (right, bottom) = (left + wa.size.width as f64 / ms, top + wa.size.height as f64 / ms);
        if y + height > bottom {
            y = (bottom - height).max(top);
        }
        if x + width > right {
            x = (right - width).max(left);
        }
    }

    let phys = LogicalPosition::new(x, y).to_physical::<i32>(scale);
    *state.last_set_pos.lock().unwrap() = Some((phys.x, phys.y));
    platform::set_frame(&win, x, y, width, height)
}

/// Full opacity when focused, the user's setting otherwise.
pub fn apply_alpha(app: &AppHandle, focused: bool) {
    if let Some(win) = app.get_webview_window("main") {
        let inactive = app.state::<AppState>().settings.lock().unwrap().inactive_opacity;
        platform::set_alpha(&win, if focused { 1.0 } else { inactive });
    }
}

/// First launch: top center of the primary screen.
pub fn default_home(app: &AppHandle) -> (f64, f64) {
    if let Ok(Some(m)) = app.primary_monitor() {
        let wa = m.work_area();
        let s = m.scale_factor();
        let x = wa.position.x as f64 / s + (wa.size.width as f64 / s - COMPACT) / 2.0;
        let y = wa.position.y as f64 / s + 40.0;
        return (x, y);
    }
    (100.0, 100.0)
}

/// Built in code (not tauri.conf.json) so the platform layer can adjust the webview.
pub fn build_main(app: &tauri::App) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Fast Mode")
        .inner_size(COMPACT, COMPACT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .accept_first_mouse(true)
        .focused(true)
        .visible(false);
    platform::configure_main_window(builder).build().map(|_| ())
}

/// A drag sends many Moved events; write settings.json once it settles.
fn save_home_soon(app: AppHandle) {
    let gen = app.state::<AppState>().home_save_gen.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let state = app.state::<AppState>();
        if state.home_save_gen.load(Ordering::SeqCst) == gen {
            state.save_settings();
        }
    });
}

/// Main-window events: opacity on focus change, remember where the user dragged it.
pub fn on_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::Focused(focused) = event {
        apply_alpha(window.app_handle(), *focused);
    }
    if let WindowEvent::Moved(pos) = event {
        let state = window.state::<AppState>();
        let ours = *state.last_set_pos.lock().unwrap() == Some((pos.x, pos.y));
        if !ours {
            let scale = window.scale_factor().unwrap_or(1.0);
            let l = pos.to_logical::<f64>(scale);
            state.settings.lock().unwrap().home = Some((l.x, l.y));
            save_home_soon(window.app_handle().clone());
        }
    }
}
