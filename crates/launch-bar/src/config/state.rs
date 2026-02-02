//! Application state persistence

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

/// Persistent application state (window positions per directory)
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppState {
    positions: HashMap<String, [f32; 2]>,
}

impl AppState {
    /// Load state from disk
    pub fn load() -> Self {
        let state_path = Self::state_path();
        if state_path.exists() {
            std::fs::read_to_string(&state_path)
                .ok()
                .and_then(|s| toml::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save state to disk
    pub fn save(&self) {
        let state_path = Self::state_path();
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(content) = toml::to_string_pretty(self) {
            std::fs::write(&state_path, content).ok();
        }
    }

    /// Get the state file path
    fn state_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("launch-bar")
            .join("state.toml")
    }

    /// Get saved position for a working directory
    pub fn get_position(&self, cwd: &str) -> Option<egui::Pos2> {
        self.positions.get(cwd).map(|p| egui::pos2(p[0], p[1]))
    }

    /// Set position for a working directory
    pub fn set_position(&mut self, cwd: &str, pos: egui::Pos2) {
        self.positions.insert(cwd.to_string(), [pos.x, pos.y]);
    }
}
