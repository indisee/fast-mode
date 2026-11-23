//! Fast Mode — floating always-on-top panel that sends text to `claude -p`
//! with a system prompt and puts the reply on the clipboard.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio::io::AsyncWriteExt;

const DEFAULT_PROMPT: &str = include_str!("../default-prompt.md");
const HISTORY_LIMIT: usize = 300;
const COMPACT: f64 = 70.0;

// ───────────────────────── data ─────────────────────────

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Settings {
    #[serde(default)]
    model: String,
    #[serde(default = "default_opacity")]
    inactive_opacity: f64,
    /// Accelerator like "Control+Alt+KeyV"; empty = disabled.
    #[serde(default = "default_shortcut")]
    shortcut: String,
    /// Logical top-left the user dragged the panel to.
    #[serde(default)]
    home: Option<(f64, f64)>,
    /// Id of the project whose prompt is used.
    #[serde(default)]
    active_project: String,
}

fn default_opacity() -> f64 {
    0.7
}
fn default_shortcut() -> String {
    "Control+Alt+KeyV".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: String::new(),
            inactive_opacity: default_opacity(),
            shortcut: default_shortcut(),
            home: None,
            active_project: String::new(),
        }
    }
}

/// A named system prompt with a frame color. Unknown fields are kept,
/// so newer versions can add things without older ones dropping them.
#[derive(Serialize, Deserialize, Clone)]
struct Project {
    id: String,
    name: String,
    #[serde(default)]
    prompt: String,
    /// CSS color ("#ff9f0a"); empty = neutral frame.
    #[serde(default)]
    color: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}

/// Same shape as the Swift v1 history.json, so history carries over.
#[derive(Serialize, Deserialize, Clone)]
struct HistoryItem {
    id: String,
    date: String,
    request: String,
    response: String,
    /// Full `claude -p --output-format json` result (cost, usage, session, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw: Option<serde_json::Value>,
    /// Project name/color at the time of the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

struct AppState {
    dir: PathBuf,
    settings: Mutex<Settings>,
    projects: Mutex<Vec<Project>>,
    history: Mutex<Vec<HistoryItem>>,
    child_pid: Mutex<Option<u32>>,
    cancelled: AtomicBool,
    /// Last position we set ourselves, so our own moves don't overwrite `home`.
    last_set_pos: Mutex<Option<(i32, i32)>>,
    claude: Mutex<Option<String>>,
}

impl AppState {
    fn load() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("FastMode");
        let _ = std::fs::create_dir_all(&dir);
        let mut settings: Settings = read_json(&dir.join("settings.json")).unwrap_or_default();
        let mut projects: Vec<Project> = read_json(&dir.join("projects.json")).unwrap_or_default();
        if projects.is_empty() {
            // First run of the projects version: adopt the old single prompt.
            let prompt = std::fs::read_to_string(dir.join("system-prompt.md"))
                .unwrap_or_else(|_| DEFAULT_PROMPT.to_string());
            projects.push(Project {
                id: new_id(),
                name: "Default".into(),
                prompt,
                color: String::new(),
                extra: Default::default(),
            });
            write_json(&dir.join("projects.json"), &projects);
        }
        if !projects.iter().any(|p| p.id == settings.active_project) {
            settings.active_project = projects[0].id.clone();
        }
        let history = read_json(&dir.join("history.json")).unwrap_or_default();
        Self {
            dir,
            settings: Mutex::new(settings),
            projects: Mutex::new(projects),
            history: Mutex::new(history),
            child_pid: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            last_set_pos: Mutex::new(None),
            claude: Mutex::new(None),
        }
    }

    fn active_project(&self) -> Project {
        let id = self.settings.lock().unwrap().active_project.clone();
        let projects = self.projects.lock().unwrap();
        projects.iter().find(|p| p.id == id).unwrap_or(&projects[0]).clone()
    }

    fn save_projects(&self) {
        write_json(&self.dir.join("projects.json"), &*self.projects.lock().unwrap());
    }

    fn save_settings(&self) {
        write_json(&self.dir.join("settings.json"), &*self.settings.lock().unwrap());
    }

    fn save_history(&self) {
        write_json(&self.dir.join("history.json"), &*self.history.lock().unwrap());
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Ok(data) = serde_json::to_vec_pretty(value) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

// ───────────────────────── claude CLI ─────────────────────────

fn extra_paths() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![home.join(".local/bin"), home.join(".claude/local")];
    if cfg!(windows) {
        if let Some(appdata) = dirs::data_dir() {
            v.push(appdata.join("npm"));
        }
    } else {
        v.push("/opt/homebrew/bin".into());
        v.push("/usr/local/bin".into());
    }
    v
}

fn augmented_path() -> std::ffi::OsString {
    let mut parts = extra_paths();
    if let Some(p) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(parts).unwrap_or_default()
}

/// GUI apps don't inherit the shell PATH: check the usual spots, then ask the shell.
fn find_claude() -> Option<String> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    for dir in extra_paths() {
        for n in names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    let out = if cfg!(windows) {
        std::process::Command::new("where").arg("claude").output().ok()?
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        std::process::Command::new(shell)
            .args(["-lc", "command -v claude"])
            .output()
            .ok()?
    };
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().map(|l| l.trim().to_string()).filter(|l| !l.is_empty())
}

/// The prompt wraps markdown in ``` for chat UIs; the clipboard wants the raw text.
fn strip_outer_fence(s: &str) -> String {
    let t = s.trim();
    if t.len() > 6 && t.starts_with("```") && t.ends_with("```") {
        let lines: Vec<&str> = t.lines().collect();
        if lines.len() >= 2 {
            return lines[1..lines.len() - 1].join("\n").trim().to_string();
        }
    }
    t.to_string()
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[derive(Serialize)]
struct Answer {
    response: String,
    history: Vec<HistoryItem>,
}

#[tauri::command]
async fn ask(app: AppHandle, state: State<'_, AppState>, text: String) -> Result<Answer, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Nothing to send".into());
    }
    let exe = claude_exe(&state)?;
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
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    state.cancelled.store(false, Ordering::SeqCst);
    let mut child = cmd.spawn().map_err(|e| format!("Failed to launch claude: {e}"))?;
    *state.child_pid.lock().unwrap() = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes()).await;
    }
    let out = child.wait_with_output().await;
    *state.child_pid.lock().unwrap() = None;
    let out = out.map_err(|e| e.to_string())?;

    if state.cancelled.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // claude exits non-zero on API/auth errors but still prints the JSON envelope.
    let raw: Option<serde_json::Value> = serde_json::from_str(&stdout).ok();
    if raw.is_none() && (!out.status.success() || stdout.is_empty()) {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err([stderr, stdout]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| format!("claude exited with {}", out.status)));
    }

    // JSON envelope: {"type":"result","is_error":…,"result":"…","usage":…,"total_cost_usd":…}
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
        None => stdout.clone(),
    };
    if text_out.trim().is_empty() {
        return Err(format!("Empty response from claude: {stdout}"));
    }
    let response = strip_outer_fence(&text_out);
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

fn claude_exe(state: &AppState) -> Result<String, String> {
    let mut cached = state.claude.lock().unwrap();
    if cached.is_none() {
        *cached = find_claude();
    }
    cached.clone().ok_or_else(|| "claude CLI not found".into())
}

// ───────────────────────── account ─────────────────────────

/// `claude auth status` → {loggedIn, email?, authMethod, …}
#[tauri::command]
async fn auth_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let exe = claude_exe(&state)?;
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["auth", "status"])
        .current_dir(&state.dir)
        .env("PATH", augmented_path())
        .stdin(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).map_err(|_| stdout.trim().to_string())
}

/// `claude auth login` needs a browser round-trip and may ask questions,
/// so run it in a real terminal window.
#[tauri::command]
fn login(state: State<'_, AppState>) -> Result<(), String> {
    let exe = claude_exe(&state)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = state.dir.join("login.command");
        let body = format!(
            "#!/bin/bash\nclear\n'{exe}' auth login && '{exe}' auth status\necho\necho 'You can close this window.'\n",
            exe = exe.replace('\'', "'\\''")
        );
        std::fs::write(&script, body).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        std::process::Command::new("open").arg(&script).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    std::process::Command::new("cmd")
        .args(["/c", "start", "", "cmd", "/k"])
        .arg(&exe)
        .args(["auth", "login"])
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(all(unix, not(target_os = "macos")))]
    std::process::Command::new("x-terminal-emulator")
        .args(["-e", &exe, "auth", "login"])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_usage() {
    let url = "https://claude.ai/settings/usage";
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    if let Some(pid) = *state.child_pid.lock().unwrap() {
        state.cancelled.store(true, Ordering::SeqCst);
        kill_pid(pid);
    }
}

// ───────────────────────── state / settings ─────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    settings: Settings,
    projects: Vec<Project>,
    history: Vec<HistoryItem>,
    compact: f64,
    platform: &'static str,
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> Snapshot {
    Snapshot {
        settings: state.settings.lock().unwrap().clone(),
        projects: state.projects.lock().unwrap().clone(),
        history: state.history.lock().unwrap().clone(),
        compact: COMPACT,
        platform: std::env::consts::OS,
    }
}

#[tauri::command]
fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_history(state: State<'_, AppState>, id: String) -> Vec<HistoryItem> {
    let h = {
        let mut h = state.history.lock().unwrap();
        h.retain(|i| i.id != id);
        h.clone()
    };
    state.save_history();
    h
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) {
    state.history.lock().unwrap().clear();
    state.save_history();
}

fn broadcast_settings(app: &AppHandle, state: &AppState) {
    state.save_settings();
    let _ = app.emit("settings", state.settings.lock().unwrap().clone());
}

#[tauri::command]
fn set_model(app: AppHandle, state: State<'_, AppState>, model: String) {
    state.settings.lock().unwrap().model = model;
    broadcast_settings(&app, &state);
}

#[tauri::command]
fn set_opacity(app: AppHandle, state: State<'_, AppState>, value: f64) {
    state.settings.lock().unwrap().inactive_opacity = value.clamp(0.2, 1.0);
    broadcast_settings(&app, &state);
    if let Some(win) = app.get_webview_window("main") {
        let focused = win.is_focused().unwrap_or(true);
        apply_alpha(&app, focused);
    }
}

fn register_shortcut(app: &AppHandle, accel: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if accel.is_empty() {
        return Ok(());
    }
    gs.register(accel)
        .map_err(|e| format!("Couldn't register {accel}: {e}"))
}

/// Save + register a new global shortcut ("" disables it).
#[tauri::command]
fn set_shortcut(app: AppHandle, state: State<'_, AppState>, accel: String) -> Result<(), String> {
    let result = register_shortcut(&app, &accel);
    state.settings.lock().unwrap().shortcut = accel;
    broadcast_settings(&app, &state);
    result
}

/// Unregister while the settings window records a new combination.
#[tauri::command]
fn pause_shortcut(app: AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}

#[tauri::command]
fn resume_shortcut(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let accel = state.settings.lock().unwrap().shortcut.clone();
    register_shortcut(&app, &accel)
}

/// Replace the whole project list (Settings window edits).
#[tauri::command]
fn save_projects(app: AppHandle, state: State<'_, AppState>, projects: Vec<Project>) -> Result<(), String> {
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
fn set_active_project(app: AppHandle, state: State<'_, AppState>, id: String) {
    if state.projects.lock().unwrap().iter().any(|p| p.id == id) {
        state.settings.lock().unwrap().active_project = id;
        broadcast_settings(&app, &state);
    }
}

#[tauri::command]
fn new_project_id() -> String {
    new_id()
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
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
fn quit(app: AppHandle) {
    app.exit(0);
}

// ───────────────────────── window geometry ─────────────────────────

/// Resize the panel, keeping the user's top-left (`home`) unless that would push it
/// off screen — then it grows upward / leftward instead. Shrinking returns it home.
#[tauri::command]
fn resize(app: AppHandle, state: State<'_, AppState>, width: f64, height: f64) -> Result<(), String> {
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
    set_frame(&win, x, y, width, height)
}

/// macOS: one `setFrame:display:` call. Tauri's set_size + set_position are two async
/// steps (and setContentSize keeps the bottom edge), so the panel visibly jumps.
#[cfg(target_os = "macos")]
fn set_frame(win: &tauri::WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

    let ptr = win.ns_window().map_err(|e| e.to_string())? as usize;
    win.run_on_main_thread(move || unsafe {
        let mtm = MainThreadMarker::new_unchecked();
        let ns_window = &*(ptr as *const NSWindow);
        // Tauri uses top-left origin on the primary screen; Cocoa uses bottom-left.
        let primary_h = NSScreen::screens(mtm)
            .firstObject()
            .map(|s| s.frame().size.height)
            .unwrap_or(0.0);
        let rect = NSRect::new(NSPoint::new(x, primary_h - y - h), NSSize::new(w, h));
        ns_window.setFrame_display(rect, true);
    })
    .map_err(|e| e.to_string())
}

/// Inactive opacity via NSWindow.alphaValue: changing CSS opacity made WebKit rebuild
/// its layers and the panel blinked on focus.
#[cfg(target_os = "macos")]
fn set_alpha(win: &tauri::WebviewWindow, alpha: f64) {
    use objc2_app_kit::NSWindow;
    let Ok(ptr) = win.ns_window() else { return };
    let ptr = ptr as usize;
    let _ = win.run_on_main_thread(move || unsafe {
        (&*(ptr as *const NSWindow)).setAlphaValue(alpha);
    });
}

#[cfg(not(target_os = "macos"))]
fn set_alpha(_win: &tauri::WebviewWindow, _alpha: f64) {}

fn apply_alpha(app: &AppHandle, focused: bool) {
    if let Some(win) = app.get_webview_window("main") {
        let inactive = app.state::<AppState>().settings.lock().unwrap().inactive_opacity;
        set_alpha(&win, if focused { 1.0 } else { inactive });
    }
}

#[cfg(not(target_os = "macos"))]
fn set_frame(win: &tauri::WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    win.set_size(tauri::LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    win.set_position(LogicalPosition::new(x, y)).map_err(|e| e.to_string())
}

fn default_home(app: &AppHandle) -> (f64, f64) {
    if let Ok(Some(m)) = app.primary_monitor() {
        let wa = m.work_area();
        let s = m.scale_factor();
        let x = wa.position.x as f64 / s + (wa.size.width as f64 / s - COMPACT) / 2.0;
        let y = wa.position.y as f64 / s + 40.0;
        return (x, y);
    }
    (100.0, 100.0)
}

/// The panel is built in code (not tauri.conf.json) so macOS gets a WebKit config
/// with Writing Tools ("Write with Siri") and inline predictions turned off.
fn build_main_window(app: &tauri::App) -> tauri::Result<()> {
    #[allow(unused_mut)]
    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
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
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_webview_configuration(webkit_config());
    }
    builder.build().map(|_| ())
}

#[cfg(target_os = "macos")]
fn webkit_config() -> objc2::rc::Retained<objc2_web_kit::WKWebViewConfiguration> {
    use objc2::{msg_send, runtime::NSObjectProtocol, sel};
    let mtm = objc2_foundation::MainThreadMarker::new().expect("setup runs on the main thread");
    #[allow(unused_unsafe)]
    let config = unsafe { objc2_web_kit::WKWebViewConfiguration::new(mtm) };
    unsafe {
        // NSWritingToolsBehaviorNone = -1 (macOS 15+)
        if config.respondsToSelector(sel!(setWritingToolsBehavior:)) {
            let _: () = msg_send![&*config, setWritingToolsBehavior: -1isize];
        }
        // macOS 14+
        if config.respondsToSelector(sel!(setAllowsInlinePredictions:)) {
            let _: () = msg_send![&*config, setAllowsInlinePredictions: false];
        }
    }
    config
}

// ───────────────────────── global shortcut ─────────────────────────

/// Send the clipboard to Claude via the main window, without stealing focus.
fn on_hotkey(app: &AppHandle) {
    let text = app.clipboard().read_text().unwrap_or_default();
    if text.trim().is_empty() {
        return;
    }
    // No show()/focus here: the panel is always visible, and making it key while
    // another app is active hid it until the request finished.
    let _ = app.emit_to("main", "hotkey-send", text);
}

// ───────────────────────── entry ─────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        on_hotkey(app);
                    }
                })
                .build(),
        )
        .manage(AppState::load())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            {
                let mut s = state.settings.lock().unwrap();
                if s.home.is_none() {
                    s.home = Some(default_home(&handle));
                }
            }
            let accel = state.settings.lock().unwrap().shortcut.clone();
            if let Err(e) = register_shortcut(&handle, &accel) {
                eprintln!("{e}");
            }

            build_main_window(app)?;

            if let Some(win) = app.get_webview_window("main") {
                let _ = resize(handle.clone(), state, COMPACT, COMPACT);
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
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
                    state.save_settings();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ask,
            cancel,
            get_state,
            copy_text,
            delete_history,
            clear_history,
            set_model,
            set_opacity,
            set_shortcut,
            pause_shortcut,
            resume_shortcut,
            save_projects,
            set_active_project,
            new_project_id,
            open_settings,
            resize,
            auth_status,
            login,
            open_usage,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fast Mode");
}
