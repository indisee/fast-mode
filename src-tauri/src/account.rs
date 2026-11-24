//! Claude account: sign-in status, login, usage page.

use std::process::Stdio;

use tauri::State;

use crate::{
    claude::{augmented_path, claude_exe},
    platform,
    state::AppState,
};

/// `claude auth status` → {loggedIn, email?, authMethod, …}
#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let exe = claude_exe(&state)?;
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["auth", "status"])
        .current_dir(&state.dir)
        .env("PATH", augmented_path())
        .stdin(Stdio::null());
    platform::hide_console(&mut cmd);
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).map_err(|_| stdout.trim().to_string())
}

/// `claude auth login` needs a browser round-trip and may ask questions,
/// so run it in a real terminal window.
#[tauri::command]
pub fn login(state: State<'_, AppState>) -> Result<(), String> {
    let exe = claude_exe(&state)?;
    platform::open_login_terminal(&exe, &state.dir)
}

#[tauri::command]
pub fn open_usage() {
    platform::open_url("https://claude.ai/settings/usage");
}
