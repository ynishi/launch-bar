//! Platform-specific utilities

use std::path::{Path, PathBuf};
use std::process::Command;

/// Execute a shell command on the current platform
pub fn spawn_shell_command(cmd: &str, cwd: &PathBuf) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", cmd])
            .current_dir(cwd)
            .spawn()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .args(["-c", cmd])
            .current_dir(cwd)
            .spawn()
    }
}

/// Open a file with the default system application
pub fn open_file(path: &PathBuf) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("notepad").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

/// Open a file with the default application (blocking version for CLI)
pub fn open_file_with_default_app(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).status()?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(path).status()?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .status()?;
    }
    Ok(())
}
