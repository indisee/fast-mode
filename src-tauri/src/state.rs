//! Persistent data (settings, projects, history) and the shared app state.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Mutex,
    },
};

use serde::{Deserialize, Serialize};

const DEFAULT_PROMPT: &str = include_str!("../default-prompt.md");
pub const HISTORY_LIMIT: usize = 300;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_opacity")]
    pub inactive_opacity: f64,
    /// Accelerator like "Control+Alt+KeyV"; empty = disabled.
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
    /// Logical top-left the user dragged the panel to.
    #[serde(default)]
    pub home: Option<(f64, f64)>,
    /// Id of the project whose prompt is used.
    #[serde(default)]
    pub active_project: String,
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
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    /// CSS color ("#ff9f0a"); empty = neutral frame.
    #[serde(default)]
    pub color: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}

/// Same shape as the Swift v1 history.json, so history carries over.
#[derive(Serialize, Deserialize, Clone)]
pub struct HistoryItem {
    pub id: String,
    pub date: String,
    pub request: String,
    pub response: String,
    /// Full `claude -p --output-format json` result (cost, usage, session, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    /// Project name/color at the time of the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

pub struct AppState {
    pub dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub projects: Mutex<Vec<Project>>,
    pub history: Mutex<Vec<HistoryItem>>,
    pub child_pid: Mutex<Option<u32>>,
    pub cancelled: AtomicBool,
    /// Last position we set ourselves, so our own moves don't overwrite `home`.
    pub last_set_pos: Mutex<Option<(i32, i32)>>,
    /// Cached path to the `claude` executable.
    pub claude: Mutex<Option<String>>,
    /// Bumped on every drag move; only the last move's delayed save writes.
    pub home_save_gen: AtomicU64,
}

impl AppState {
    pub fn load() -> Self {
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
            home_save_gen: AtomicU64::new(0),
        }
    }

    pub fn active_project(&self) -> Project {
        let id = self.settings.lock().unwrap().active_project.clone();
        let projects = self.projects.lock().unwrap();
        projects.iter().find(|p| p.id == id).unwrap_or(&projects[0]).clone()
    }

    pub fn save_projects(&self) {
        write_json(&self.dir.join("projects.json"), &*self.projects.lock().unwrap());
    }

    pub fn save_settings(&self) {
        write_json(&self.dir.join("settings.json"), &*self.settings.lock().unwrap());
    }

    pub fn save_history(&self) {
        write_json(&self.dir.join("history.json"), &*self.history.lock().unwrap());
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Write via a temp file + rename, so a crash never leaves half a JSON file.
fn write_json<T: Serialize>(path: &Path, value: &T) {
    if let Ok(data) = serde_json::to_vec_pretty(value) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}
