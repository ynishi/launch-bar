//! Lua script engine implementation

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use arboard::Clipboard;
use mlua::{Lua, Result as LuaResult};

use super::ScriptResult;

/// Create a Lua instance with registered functions
fn create_lua(cwd: Arc<PathBuf>) -> LuaResult<Lua> {
    let lua = Lua::new();

    // Register global functions
    let globals = lua.globals();

    // clipboard() -> string
    globals.set(
        "clipboard",
        lua.create_function(|_, ()| {
            Ok(Clipboard::new()
                .and_then(|mut cb| cb.get_text())
                .unwrap_or_else(|_| "[ERROR:clipboard]".to_string()))
        })?,
    )?;

    // clipboard_set(text) -> boolean
    globals.set(
        "clipboard_set",
        lua.create_function(|_, text: String| {
            Ok(Clipboard::new()
                .and_then(|mut cb| cb.set_text(text))
                .is_ok())
        })?,
    )?;

    // shell(cmd) -> string
    let cwd_for_shell = Arc::clone(&cwd);
    globals.set(
        "shell",
        lua.create_function(move |_, cmd: String| {
            let output = Command::new("sh")
                .args(["-c", &cmd])
                .current_dir(cwd_for_shell.as_ref())
                .output();
            Ok(match output {
                Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
                Err(e) => format!("[ERROR:shell] {}", e),
            })
        })?,
    )?;

    // shell_spawn(cmd) -> boolean
    let cwd_for_spawn = Arc::clone(&cwd);
    globals.set(
        "shell_spawn",
        lua.create_function(move |_, cmd: String| {
            Ok(Command::new("sh")
                .args(["-c", &cmd])
                .current_dir(cwd_for_spawn.as_ref())
                .spawn()
                .is_ok())
        })?,
    )?;

    // claude(prompt) -> string
    let cwd_for_claude = Arc::clone(&cwd);
    globals.set(
        "claude",
        lua.create_function(move |_, prompt: String| {
            let output = Command::new("claude")
                .args(["-p", &prompt])
                .current_dir(cwd_for_claude.as_ref())
                .output();
            Ok(match output {
                Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
                Err(e) => format!("[ERROR:claude] {}", e),
            })
        })?,
    )?;

    // notify(message)
    #[cfg(target_os = "macos")]
    globals.set(
        "notify",
        lua.create_function(|_, msg: String| {
            let escaped = msg
                .replace("\\", "\\\\")
                .replace("\"", "\\\"")
                .replace("\n", " ")
                .replace("\r", "");
            let script = format!(r#"display alert "Launch Bar" message "{}""#, escaped);
            let _ = Command::new("osascript").args(["-e", &script]).spawn();
            Ok(())
        })?,
    )?;

    #[cfg(not(target_os = "macos"))]
    globals.set(
        "notify",
        lua.create_function(|_, msg: String| {
            eprintln!("[notify] {}", msg);
            Ok(())
        })?,
    )?;

    // open(path)
    globals.set(
        "open",
        lua.create_function(|_, path: String| {
            #[cfg(target_os = "macos")]
            let _ = Command::new("open").arg(&path).spawn();
            #[cfg(target_os = "linux")]
            let _ = Command::new("xdg-open").arg(&path).spawn();
            #[cfg(target_os = "windows")]
            let _ = Command::new("cmd").args(["/C", "start", &path]).spawn();
            Ok(())
        })?,
    )?;

    // env(name) -> string
    globals.set(
        "env",
        lua.create_function(|_, name: String| Ok(std::env::var(&name).unwrap_or_default()))?,
    )?;

    // read_file(path) -> string
    let cwd_for_read = Arc::clone(&cwd);
    globals.set(
        "read_file",
        lua.create_function(move |_, path: String| {
            let full_path = if path.starts_with('/') {
                PathBuf::from(&path)
            } else {
                cwd_for_read.join(&path)
            };
            Ok(std::fs::read_to_string(&full_path)
                .unwrap_or_else(|e| format!("[ERROR:read_file] {}: {}", path, e)))
        })?,
    )?;

    // write_file(path, content) -> boolean
    let cwd_for_write = Arc::clone(&cwd);
    globals.set(
        "write_file",
        lua.create_function(move |_, (path, content): (String, String)| {
            let full_path = if path.starts_with('/') {
                PathBuf::from(&path)
            } else {
                cwd_for_write.join(&path)
            };
            Ok(std::fs::write(full_path, content).is_ok())
        })?,
    )?;

    Ok(lua)
}

/// Execute a Lua script
pub fn run(script: &str, cwd: Arc<PathBuf>) -> ScriptResult {
    match create_lua(cwd) {
        Ok(lua) => match lua.load(script).exec() {
            Ok(_) => ScriptResult {
                success: true,
                message: "Script completed".to_string(),
            },
            Err(e) => ScriptResult {
                success: false,
                message: format!("Script error: {}", e),
            },
        },
        Err(e) => ScriptResult {
            success: false,
            message: format!("Failed to initialize Lua: {}", e),
        },
    }
}
