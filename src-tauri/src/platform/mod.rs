//! Everything OS-specific lives here. Each OS module exposes the same functions:
//!
//! - `setup`: app-level tweaks at startup
//! - `configure_main_window`: adjust the panel's builder before it is built
//! - `set_frame` / `set_alpha`: move+resize and fade the panel
//! - `open_url`, `open_login_terminal`: hand off to the OS
//! - `kill_pid`, `hide_console`: child-process handling
//! - `CLAUDE_NAMES`, `claude_bin_dirs`, `which_claude`: finding the `claude` CLI
//!
//! Only macOS is tested. Windows and Linux compile in principle but are untested.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(all(unix, not(target_os = "macos")))]
pub use linux::*;

/// Directories `claude` is usually installed to, besides PATH.
fn common_bin_dirs() -> Vec<std::path::PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![home.join(".local/bin"), home.join(".claude/local")]
}

/// `command -v claude` in the user's login shell (GUI apps don't get the shell PATH).
#[cfg(unix)]
fn which_in_login_shell() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = std::process::Command::new(shell)
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    first_existing_file(&out.stdout)
}

/// First output line that is an existing file. Login shells can print banners
/// or notices before the path, so the first line alone isn't trusted.
fn first_existing_file(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && std::path::Path::new(l).is_file())
        .map(str::to_string)
}
