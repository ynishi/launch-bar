//! Configuration types for Launch Bar

use serde::Deserialize;

use crate::script::ScriptType;

/// Reserved name for top-level commands converted to preset
pub const GLOBAL_PRESET_NAME: &str = "[Global]";

/// Main configuration structure
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub window: WindowSettings,
    #[serde(default)]
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub commands: Vec<CommandConfig>,
}

impl Config {
    /// Convert top-level commands to a [Global] preset
    ///
    /// Returns None if no commands are defined.
    pub fn commands_as_preset(&self) -> Option<Preset> {
        if self.commands.is_empty() {
            return None;
        }
        Some(Preset {
            name: GLOBAL_PRESET_NAME.to_string(),
            detect_file: None,
            cwd_pattern: None,
            base_color: self.window.background_color.clone(),
            default_script: self.window.default_script,
            commands: self.commands.clone(),
        })
    }
}

/// Preset configuration for project-specific commands
#[derive(Debug, Deserialize, Clone)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub detect_file: Option<String>,
    #[serde(default)]
    pub cwd_pattern: Option<String>,
    #[serde(default)]
    pub base_color: Option<String>,
    #[serde(default)]
    pub default_script: Option<ScriptType>,
    #[serde(default)]
    pub commands: Vec<CommandConfig>,
}

impl Preset {
    /// Returns true if this preset has no detection rules (i.e., a global/fallback preset)
    pub fn is_global(&self) -> bool {
        self.detect_file.is_none() && self.cwd_pattern.is_none()
    }
}

/// Command configuration
#[derive(Debug, Deserialize, Clone)]
pub struct CommandConfig {
    pub name: String,
    #[serde(default)]
    pub cmd: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub script_type: Option<ScriptType>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Window settings
#[derive(Debug, Deserialize, Clone)]
pub struct WindowSettings {
    #[serde(default = "default_max_icons")]
    pub max_icons: usize,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub background_color: Option<String>,
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_title_bar")]
    pub title_bar: String,
    #[serde(default = "default_auto")]
    pub accent_line: String,
    #[serde(default)]
    pub default_script: Option<ScriptType>,
}

fn default_max_icons() -> usize {
    5
}

fn default_opacity() -> f32 {
    0.8
}

fn default_border() -> String {
    "auto".to_string()
}

fn default_title_bar() -> String {
    "auto".to_string()
}

fn default_auto() -> String {
    "auto".to_string()
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            max_icons: default_max_icons(),
            opacity: default_opacity(),
            background_color: None,
            border: default_border(),
            title_bar: default_title_bar(),
            accent_line: default_auto(),
            default_script: None,
        }
    }
}
