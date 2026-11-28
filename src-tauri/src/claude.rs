//! Running `claude -p`: finding the CLI, sending a request, parsing the reply.

use std::{process::Stdio, sync::atomic::Ordering};

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::io::AsyncWriteExt;

use crate::{
    platform,
    state::{new_id, AppState, HistoryItem, HISTORY_LIMIT},
};

/// PATH with the usual install locations first: GUI apps don't inherit the shell PATH.
pub fn augmented_path() -> std::ffi::OsString {
    let mut parts = platform::claude_bin_dirs();
    if let Some(p) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(parts).unwrap_or_default()
}

/// Check the usual spots, then ask the system.
fn find_claude() -> Option<String> {
    for dir in platform::claude_bin_dirs() {
        for n in platform::CLAUDE_NAMES {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    platform::which_claude()
}

/// Path to `claude`, cached. Looked up again if the cached file is gone (the CLI
/// moved after an update). The lookup may run a login shell, so it runs on a
/// blocking thread with no lock held.
pub async fn claude_exe(state: &AppState) -> Result<String, String> {
    if let Some(p) = state.claude.lock().unwrap().clone() {
        if std::path::Path::new(&p).is_file() {
            return Ok(p);
        }
    }
    let found = tauri::async_runtime::spawn_blocking(find_claude)
        .await
        .ok()
        .flatten();
    *state.claude.lock().unwrap() = found.clone();
    found.ok_or_else(|| "claude CLI not found".into())
}

/// The prompt wraps markdown in ``` for chat UIs; the clipboard wants the raw text.
/// Only a single block that wraps the whole reply is unwrapped: a reply with
/// several blocks (or nothing inside the fence) is kept as is.
fn strip_outer_fence(s: &str) -> String {
    let t = s.trim();
    let lines: Vec<&str> = t.lines().collect();
    if lines.len() >= 3
        && lines[0].starts_with("```")
        && lines[lines.len() - 1].trim() == "```"
        && !lines[1..lines.len() - 1].iter().any(|l| l.trim_start().starts_with("```"))
    {
        let inner = lines[1..lines.len() - 1].join("\n").trim().to_string();
        if !inner.is_empty() {
            return inner;
        }
    }
    t.to_string()
}

/// With `verbose` on (CLI flag or `"verbose": true` in ~/.claude/settings.json),
/// `--output-format json` prints an array of every event instead of the single
/// result envelope. Pick the last `"type":"result"` element out of it.
fn result_envelope(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Array(items) => {
            let found = items
                .iter()
                .rev()
                .find(|e| e.get("type").and_then(|t| t.as_str()) == Some("result"))
                .cloned();
            found.unwrap_or(serde_json::Value::Array(items))
        }
        other => other,
    }
}

/// Keep error messages short: the first `max` chars, then an ellipsis.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Turn the CLI's output into (reply text, raw JSON envelope) or an error message.
///
/// The envelope looks like `{"type":"result","is_error":…,"result":"…","usage":…,"total_cost_usd":…}`.
/// claude exits non-zero on API/auth errors but still prints it.
fn parse_output(
    stdout: &str,
    stderr: &str,
    success: bool,
    status: &str,
) -> Result<(String, Option<serde_json::Value>), String> {
    let stdout = stdout.trim();
    let raw: Option<serde_json::Value> = serde_json::from_str(stdout).ok().map(result_envelope);
    if raw.is_none() && (!success || stdout.is_empty()) {
        return Err([stderr.trim(), stdout]
            .into_iter()
            .find(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("claude exited with {status}")));
    }

    let text_out = match &raw {
        Some(v) => {
            let result = v.get("result").and_then(|r| r.as_str()).unwrap_or_default();
            if v.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false) {
                return Err(if !result.is_empty() {
                    result.to_string()
                } else if let Some(e) = v.get("errors").and_then(|e| e.as_array()).and_then(|a| a.first()) {
                    e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
                } else {
                    format!("claude error ({})", v.get("subtype").and_then(|s| s.as_str()).unwrap_or("unknown"))
                });
            }
            result.to_string()
        }
        None => stdout.to_string(),
    };
    if text_out.trim().is_empty() {
        return Err(format!("Empty response from claude: {}", clip(stdout, 300)));
    }
    Ok((strip_outer_fence(&text_out), raw))
}

#[derive(Serialize)]
pub struct Answer {
    response: String,
    history: Vec<HistoryItem>,
}

/// Send `text` with the active project's prompt, copy the reply, add it to history.
#[tauri::command]
pub async fn ask(app: AppHandle, state: State<'_, AppState>, text: String) -> Result<Answer, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Nothing to send".into());
    }
    // Reset before the (possibly slow) lookup so a cancel during it still counts.
    state.cancelled.store(false, Ordering::SeqCst);
    let exe = claude_exe(&state).await?;
    let model = state.settings.lock().unwrap().model.clone();
    let project = state.active_project();
    // A file, not an argument: prompts can exceed Windows' command-line limit.
    let prompt_file = state.dir.join("active-prompt.md");
    std::fs::write(&prompt_file, &project.prompt).map_err(|e| e.to_string())?;

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("-p")
        .arg("--system-prompt-file")
        .arg(&prompt_file)
        .args(["--tools", "", "--output-format", "json"]);
    if !model.is_empty() {
        cmd.args(["--model", &model]);
    }
    // Neutral cwd so no project CLAUDE.md gets pulled in.
    cmd.current_dir(&state.dir)
        .env("PATH", augmented_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    platform::hide_console(&mut cmd);

    if state.cancelled.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let mut child = cmd.spawn().map_err(|e| format!("Failed to launch claude: {e}"))?;
    *state.child_pid.lock().unwrap() = child.id();
    // A cancel that came in before the pid was recorded couldn't kill it.
    if state.cancelled.load(Ordering::SeqCst) {
        let _ = child.start_kill();
    }
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes()).await;
    }
    let out = child.wait_with_output().await;
    *state.child_pid.lock().unwrap() = None;
    let out = out.map_err(|e| e.to_string())?;

    if state.cancelled.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let (response, raw) = parse_output(
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
        out.status.success(),
        &out.status.to_string(),
    )?;
    app.clipboard()
        .write_text(response.clone())
        .map_err(|e| e.to_string())?;

    let history = {
        let mut h = state.history.lock().unwrap();
        h.insert(
            0,
            HistoryItem {
                id: new_id(),
                date: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                request: text,
                response: response.clone(),
                raw,
                project: Some(project.name.clone()),
                color: Some(project.color.clone()).filter(|c| !c.is_empty()),
            },
        );
        h.truncate(HISTORY_LIMIT);
        h.clone()
    };
    state.save_history();
    Ok(Answer { response, history })
}

#[tauri::command]
pub fn cancel(state: State<'_, AppState>) {
    // Always flag it: `ask` checks the flag before spawning and right after.
    state.cancelled.store(true, Ordering::SeqCst);
    if let Some(pid) = *state.child_pid.lock().unwrap() {
        platform::kill_pid(pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_is_stripped() {
        assert_eq!(strip_outer_fence("```\n# Title\n- item\n```"), "# Title\n- item");
        assert_eq!(strip_outer_fence("```markdown\nhi\n```"), "hi");
    }

    #[test]
    fn plain_text_is_kept() {
        assert_eq!(strip_outer_fence("  Hello there  "), "Hello there");
        assert_eq!(strip_outer_fence("use ``` inline"), "use ``` inline");
    }

    #[test]
    fn several_blocks_are_kept() {
        let s = "```bash\na\n```\nThen run:\n```bash\nb\n```";
        assert_eq!(strip_outer_fence(s), s);
    }

    #[test]
    fn empty_fence_is_kept() {
        assert_eq!(strip_outer_fence("```\n```"), "```\n```");
        assert_eq!(strip_outer_fence("```\n\n```"), "```\n\n```");
    }

    #[test]
    fn envelope_result_is_returned() {
        let out = r#"{"type":"result","is_error":false,"result":"Hi!","total_cost_usd":0.01}"#;
        let (text, raw) = parse_output(out, "", true, "exit status: 0").unwrap();
        assert_eq!(text, "Hi!");
        assert_eq!(raw.unwrap()["total_cost_usd"], 0.01);
    }

    #[test]
    fn envelope_error_wins_even_on_nonzero_exit() {
        let out = r#"{"type":"result","is_error":true,"result":"Invalid API key · Please run /login"}"#;
        let err = parse_output(out, "", false, "exit status: 1").unwrap_err();
        assert_eq!(err, "Invalid API key · Please run /login");
    }

    #[test]
    fn envelope_error_falls_back_to_errors_then_subtype() {
        let out = r#"{"is_error":true,"result":"","errors":["rate limited"]}"#;
        assert_eq!(parse_output(out, "", true, "").unwrap_err(), "rate limited");
        let out = r#"{"is_error":true,"subtype":"error_max_turns"}"#;
        assert_eq!(parse_output(out, "", true, "").unwrap_err(), "claude error (error_max_turns)");
    }

    #[test]
    fn non_json_failure_reports_stderr_then_status() {
        assert_eq!(parse_output("", "boom\n", false, "exit status: 2").unwrap_err(), "boom");
        assert_eq!(parse_output("", "", false, "exit status: 2").unwrap_err(), "claude exited with exit status: 2");
    }

    #[test]
    fn non_json_success_is_used_as_text() {
        let (text, raw) = parse_output("plain reply\n", "", true, "").unwrap();
        assert_eq!(text, "plain reply");
        assert!(raw.is_none());
    }

    #[test]
    fn verbose_array_uses_result_event() {
        let out = r#"[{"type":"system","subtype":"init","cwd":"/x"},{"type":"assistant","message":{}},{"type":"result","is_error":false,"result":"Hi!","total_cost_usd":0.01}]"#;
        let (text, raw) = parse_output(out, "", true, "exit status: 0").unwrap();
        assert_eq!(text, "Hi!");
        assert_eq!(raw.unwrap()["type"], "result");
    }

    #[test]
    fn verbose_array_error_is_reported() {
        let out = r#"[{"type":"system","subtype":"init"},{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}]"#;
        assert_eq!(parse_output(out, "", false, "exit status: 1").unwrap_err(), "Not logged in · Please run /login");
    }

    #[test]
    fn empty_response_error_is_clipped() {
        let out = format!(r#"[{{"type":"system","cwd":"{}"}}]"#, "a".repeat(1000));
        let err = parse_output(&out, "", true, "").unwrap_err();
        assert!(err.starts_with("Empty response from claude"));
        assert!(err.chars().count() < 400);
    }

    #[test]
    fn empty_result_is_an_error() {
        let err = parse_output(r#"{"is_error":false,"result":"  "}"#, "", true, "").unwrap_err();
        assert!(err.starts_with("Empty response from claude"));
    }
}
