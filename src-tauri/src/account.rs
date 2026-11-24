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
    let exe = claude_exe(&state).await?;
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["auth", "status"])
        .current_dir(&state.dir)
        .env("PATH", augmented_path())
        .stdin(Stdio::null());
    platform::hide_console(&mut cmd);
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stdout = stdout.trim();
    serde_json::from_str(stdout).map_err(|_| {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !stdout.is_empty() {
            stdout.to_string()
        } else if !stderr.is_empty() {
            stderr
        } else {
            format!("claude auth status exited with {}", out.status)
        }
    })
}

/// `claude auth login` needs a browser round-trip and may ask questions,
/// so run it in a real terminal window.
#[tauri::command]
pub async fn login(state: State<'_, AppState>) -> Result<(), String> {
    let exe = claude_exe(&state).await?;
    platform::open_login_terminal(&exe, &state.dir)
}

#[tauri::command]
pub fn open_usage() {
    platform::open_url("https://claude.ai/settings/usage");
}
