//! macOS: native window calls through objc2, `open` for URLs and Terminal.

use std::path::{Path, PathBuf};

use tauri::{Manager, WebviewWindowBuilder, Wry};

pub const CLAUDE_NAMES: &[&str] = &["claude"];

pub fn claude_bin_dirs() -> Vec<PathBuf> {
    let mut v = super::common_bin_dirs();
    v.push("/opt/homebrew/bin".into());
    v.push("/usr/local/bin".into());
    v
}

pub fn which_claude() -> Option<String> {
    super::which_in_login_shell()
}

/// Menu-bar style app: no Dock icon, no app menu.
pub fn setup(app: &mut tauri::App) {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// Turn off Writing Tools ("Write with Siri") and inline predictions in the panel's WebKit.
pub fn configure_main_window<'a, M: Manager<Wry>>(
    builder: WebviewWindowBuilder<'a, Wry, M>,
) -> WebviewWindowBuilder<'a, Wry, M> {
    builder.with_webview_configuration(webkit_config())
}

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

/// One `setFrame:display:` call. Tauri's set_size + set_position are two async
/// steps (and setContentSize keeps the bottom edge), so the panel visibly jumps.
pub fn set_frame(win: &tauri::WebviewWindow, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
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

/// NSWindow.alphaValue: changing CSS opacity made WebKit rebuild its layers
/// and the panel blinked on focus.
pub fn set_alpha(win: &tauri::WebviewWindow, alpha: f64) {
    use objc2_app_kit::NSWindow;
    let Ok(ptr) = win.ns_window() else { return };
    let ptr = ptr as usize;
    let _ = win.run_on_main_thread(move || unsafe {
        (&*(ptr as *const NSWindow)).setAlphaValue(alpha);
    });
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Run `claude auth login` in Terminal via a `.command` script.
pub fn open_login_terminal(exe: &str, dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("login.command");
    let body = format!(
        "#!/bin/bash\nclear\n'{exe}' auth login && '{exe}' auth status\necho\necho 'You can close this window.'\n",
        exe = exe.replace('\'', "'\\''")
    );
    std::fs::write(&script, body).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&script)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

pub fn hide_console(_cmd: &mut tokio::process::Command) {}
