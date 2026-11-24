//! Linux (untested): plain Tauri window calls, xdg-open, x-terminal-emulator.

use std::path::{Path, PathBuf};

use tauri::{LogicalPosition, LogicalSize, Manager, WebviewWindowBuilder, Wry};

pub const CLAUDE_NAMES: &[&str] = &["claude"];

pub fn claude_bin_dirs() -> Vec<PathBuf> {
    let mut v = super::common_bin_dirs();
    v.push("/usr/local/bin".into());
    v
}

pub fn which_claude() -> Option<String> {
    super::which_in_login_shell()
}

pub fn setup(_app: &mut tauri::App) {}

pub fn configure_main_window<'a, M: Manager<Wry>>(
    builder: WebviewWindowBuilder<'a, Wry, M>,
) -> WebviewWindowBuilder<'a, Wry, M> {
    builder
}

pub fn set_frame(win: &tauri::WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    win.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    win.set_position(LogicalPosition::new(x, y)).map_err(|e| e.to_string())
}

/// No native window alpha here; the frontend fades the page with CSS instead.
pub fn set_alpha(_win: &tauri::WebviewWindow, _alpha: f64) {}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn open_login_terminal(exe: &str, _dir: &Path) -> Result<(), String> {
    std::process::Command::new("x-terminal-emulator")
        .args(["-e", exe, "auth", "login"])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

pub fn hide_console(_cmd: &mut tokio::process::Command) {}
