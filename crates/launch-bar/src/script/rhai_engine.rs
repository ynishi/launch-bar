//! Rhai script engine implementation

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use arboard::Clipboard;
use rhai::{Engine, Scope};

use super::ScriptResult;

/// Create a Rhai engine with registered functions
fn create_engine(cwd: Arc<PathBuf>) -> Engine {
    let mut engine = Engine::new();

    // clipboard() -> String
    engine.register_fn("clipboard", || -> String {
        Clipboard::new()
            .and_then(|mut cb| cb.get_text())
            .unwrap_or_else(|_| "[ERROR:clipboard]".to_string())
    });

    // clipboard_set(text) -> bool
    engine.register_fn("clipboard_set", |text: String| -> bool {
        Clipboard::new()
            .and_then(|mut cb| cb.set_text(text))
            .is_ok()
    });

    // shell(cmd) -> String
    let cwd_for_shell = Arc::clone(&cwd);
    engine.register_fn("shell", move |cmd: String| -> String {
        let output = Command::new("sh")
            .args(["-c", &cmd])
            .current_dir(cwd_for_shell.as_ref())
            .output();
        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
            Err(e) => format!("[ERROR:shell] {}", e),
        }
    });

    // shell_spawn(cmd) -> bool
    let cwd_for_spawn = Arc::clone(&cwd);
    engine.register_fn("shell_spawn", move |cmd: String| -> bool {
        Command::new("sh")
            .args(["-c", &cmd])
            .current_dir(cwd_for_spawn.as_ref())
            .spawn()
            .is_ok()
    });

    // claude(prompt) -> String
    let cwd_for_claude = Arc::clone(&cwd);
    engine.register_fn("claude", move |prompt: String| -> String {
        let output = Command::new("claude")
            .args(["-p", &prompt])
            .current_dir(cwd_for_claude.as_ref())
            .output();
        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
            Err(e) => format!("[ERROR:claude] {}", e),
        }
    });

    // notify(message)
    #[cfg(target_os = "macos")]
    engine.register_fn("notify", |msg: String| {
        let escaped = msg
            .replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("\n", " ")
            .replace("\r", "");
        let script = format!(r#"display alert "Launch Bar" message "{}""#, escaped);
        let _ = Command::new("osascript").args(["-e", &script]).spawn();
    });

    #[cfg(not(target_os = "macos"))]
    engine.register_fn("notify", |msg: String| {
        eprintln!("[notify] {}", msg);
    });

    // open(path)
    engine.register_fn("open", |path: String| {
        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg(&path).spawn();
        #[cfg(target_os = "linux")]
        let _ = Command::new("xdg-open").arg(&path).spawn();
        #[cfg(target_os = "windows")]
        let _ = Command::new("cmd").args(["/C", "start", &path]).spawn();
    });

    // env(name) -> String
    engine.register_fn("env", |name: String| -> String {
        std::env::var(&name).unwrap_or_default()
    });

    // read_file(path) -> String
    let cwd_for_read = Arc::clone(&cwd);
    engine.register_fn("read_file", move |path: String| -> String {
        let full_path = if path.starts_with('/') {
            PathBuf::from(&path)
        } else {
            cwd_for_read.join(&path)
        };
        std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| format!("[ERROR:read_file] {}: {}", path, e))
    });

    // write_file(path, content) -> bool
    engine.register_fn("write_file", move |path: String, content: String| -> bool {
        let full_path = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            cwd.join(path)
        };
        std::fs::write(full_path, content).is_ok()
    });

    engine
}

/// Execute a Rhai script
pub fn run(script: &str, cwd: Arc<PathBuf>) -> ScriptResult {
    let engine = create_engine(cwd);
    let mut scope = Scope::new();

    match engine.run_with_scope(&mut scope, script) {
        Ok(_) => ScriptResult {
            success: true,
            message: "Script completed".to_string(),
        },
        Err(e) => ScriptResult {
            success: false,
            message: format!("Script error: {}", e),
        },
    }
}
