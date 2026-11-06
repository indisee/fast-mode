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
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl,
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
        }
    }
}

/// Same shape as the Swift v1 history.json, so history carries over.
#[derive(Serialize, Deserialize, Clone)]
struct HistoryItem {
    id: String,
    date: String,
    request: String,
    response: String,
}

struct AppState {
    dir: PathBuf,
    settings: Mutex<Settings>,
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
        let prompt = dir.join("system-prompt.md");
        if !prompt.exists() {
            let _ = std::fs::write(&prompt, DEFAULT_PROMPT);
        }
        let settings = read_json(&dir.join("settings.json")).unwrap_or_default();
        let history = read_json(&dir.join("history.json")).unwrap_or_default();
        Self {
            dir,
            settings: Mutex::new(settings),
            history: Mutex::new(history),
            child_pid: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            last_set_pos: Mutex::new(None),
            claude: Mutex::new(None),
        }
    }

    fn prompt_path(&self) -> PathBuf {
        self.dir.join("system-prompt.md")
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
    let exe = {
        let mut cached = state.claude.lock().unwrap();
        if cached.is_none() {
            *cached = find_claude();
        }
        cached.clone().ok_or("claude CLI not found")?
    };
    let model = state.settings.lock().unwrap().model.clone();

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("-p")
        .arg("--system-prompt-file")
        .arg(state.prompt_path())
        .args(["--tools", "", "--output-format", "text"]);
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
    if !out.status.success() || stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err([stderr, stdout]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_else(|| format!("claude exited with {}", out.status)));
    }

    let response = strip_outer_fence(&stdout);
    app.clipboard()
        .write_text(response.clone())
        .map_err(|e| e.to_string())?;

    let history = {
        let mut h = state.history.lock().unwrap();
        h.insert(
            0,
            HistoryItem {
                id: uuid::Uuid::new_v4().to_string().to_uppercase(),
                date: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                request: text,
                response: response.clone(),
            },
        );
        h.truncate(HISTORY_LIMIT);
        h.clone()
    };
    state.save_history();
    Ok(Answer { response, history })
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
    history: Vec<HistoryItem>,
    compact: f64,
    platform: &'static str,
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> Snapshot {
    Snapshot {
        settings: state.settings.lock().unwrap().clone(),
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

#[tauri::command]
fn edit_prompt(state: State<'_, AppState>) {
    let path = state.prompt_path();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-e").arg(&path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("notepad").arg(&path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        return w.set_focus().map_err(|e| e.to_string());
    }
    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Fast Mode Settings")
        .inner_size(440.0, 280.0)
        .resizable(false)
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
    win.set_size(LogicalSize::new(width, height)).map_err(|e| e.to_string())?;
    win.set_position(phys).map_err(|e| e.to_string())?;
    Ok(())
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

// ───────────────────────── global shortcut ─────────────────────────

/// Send the clipboard to Claude via the main window, without stealing focus.
fn on_hotkey(app: &AppHandle) {
    let text = app.clipboard().read_text().unwrap_or_default();
    if text.trim().is_empty() {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
    }
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
            edit_prompt,
            open_settings,
            resize,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running Fast Mode");
}
