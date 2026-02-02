//! Preset resolution with unified priority ordering
//!
//! Priority order (later overrides earlier):
//! 1. Global config (~/.config/launch-bar/config.toml)
//! 2. Project config (./launch-bar.toml)
//! 3. CLI argument (--preset <name>)
//! 4. Environment variable (LAUNCH_BAR_PRESET)

use super::detect::detect_preset_idx;
use super::types::{Config, Preset, WindowSettings};
use std::path::Path;

#[cfg(test)]
use super::types::GLOBAL_PRESET_NAME;

/// Configuration source with priority ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigSource {
    Global = 0,
    Project = 1,
    Arg = 2,
    Env = 3,
}

impl ConfigSource {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigSource::Global => "global",
            ConfigSource::Project => "project",
            ConfigSource::Arg => "arg",
            ConfigSource::Env => "env",
        }
    }
}

/// Preset with source tracking
#[derive(Debug, Clone)]
pub struct ResolvedPreset {
    pub preset: Preset,
    pub source: ConfigSource,
}

/// Unified preset resolver
///
/// Handles preset resolution from multiple sources with consistent priority.
pub struct PresetResolver {
    /// All presets collected from sources (not yet deduplicated)
    presets: Vec<ResolvedPreset>,
    /// Merged window settings (later sources override)
    window: WindowSettings,
    /// Explicitly selected preset name (from arg or env)
    explicit_preset: Option<(String, ConfigSource)>,
}

impl PresetResolver {
    pub fn new() -> Self {
        Self {
            presets: Vec::new(),
            window: WindowSettings::default(),
            explicit_preset: None,
        }
    }

    /// Add presets from global config
    pub fn add_global(&mut self, config: Config) {
        self.add_config(config, ConfigSource::Global);
    }

    /// Add presets from project config
    pub fn add_project(&mut self, config: Config) {
        self.add_config(config, ConfigSource::Project);
    }

    /// Set explicit preset from CLI argument
    pub fn set_arg_preset(&mut self, name: String) {
        self.explicit_preset = Some((name, ConfigSource::Arg));
    }

    /// Set explicit preset from environment variable
    pub fn set_env_preset(&mut self, name: String) {
        // Only set if not already set by Arg (Arg has higher priority)
        if self.explicit_preset.as_ref().map(|(_, s)| *s) != Some(ConfigSource::Arg) {
            self.explicit_preset = Some((name, ConfigSource::Env));
        }
    }

    /// Add config from a specific source
    fn add_config(&mut self, config: Config, source: ConfigSource) {
        // Merge window settings (later overrides)
        self.merge_window(&config.window, source);

        // Convert top-level commands to [Global] preset
        if let Some(global_preset) = config.commands_as_preset() {
            self.presets.push(ResolvedPreset {
                preset: global_preset,
                source,
            });
        }

        // Add all presets
        for preset in config.presets {
            self.presets.push(ResolvedPreset { preset, source });
        }
    }

    /// Merge window settings (only override non-default values)
    fn merge_window(&mut self, new_window: &WindowSettings, _source: ConfigSource) {
        // Later source values override earlier ones
        self.window.max_icons = new_window.max_icons;
        self.window.opacity = new_window.opacity;
        if new_window.background_color.is_some() {
            self.window.background_color = new_window.background_color.clone();
        }
        self.window.border = new_window.border.clone();
        self.window.title_bar = new_window.title_bar.clone();
        self.window.accent_line = new_window.accent_line.clone();
        if new_window.default_script.is_some() {
            self.window.default_script = new_window.default_script;
        }
    }

    /// Resolve presets (deduplicate by name, later source wins)
    pub fn resolve(&self) -> ResolvedConfig {
        use std::collections::HashMap;

        // Group by name, keeping track of source priority
        let mut by_name: HashMap<String, ResolvedPreset> = HashMap::new();

        for resolved in &self.presets {
            let key = resolved.preset.name.to_lowercase();
            match by_name.get(&key) {
                Some(existing) if existing.source >= resolved.source => {
                    // Existing has higher or equal priority, skip
                }
                _ => {
                    by_name.insert(key, resolved.clone());
                }
            }
        }

        // Collect and sort: Global presets first, then others
        let mut global_presets: Vec<ResolvedPreset> = Vec::new();
        let mut other_presets: Vec<ResolvedPreset> = Vec::new();

        for resolved in by_name.into_values() {
            if resolved.preset.is_global() {
                global_presets.push(resolved);
            } else {
                other_presets.push(resolved);
            }
        }

        // Sort by source within each group (Global source first)
        global_presets.sort_by_key(|r| r.source);
        other_presets.sort_by_key(|r| r.source);

        // Build final list: global presets, then detection-based presets
        let mut presets = global_presets;
        presets.extend(other_presets);

        ResolvedConfig {
            presets,
            window: self.window.clone(),
            explicit_preset: self.explicit_preset.clone(),
        }
    }

    /// Get merged window settings
    #[allow(dead_code)]
    pub fn window(&self) -> &WindowSettings {
        &self.window
    }
}

impl Default for PresetResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolved configuration ready for use
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub presets: Vec<ResolvedPreset>,
    pub window: WindowSettings,
    pub explicit_preset: Option<(String, ConfigSource)>,
}

impl ResolvedConfig {
    /// Get just the presets (without source info)
    pub fn presets(&self) -> Vec<Preset> {
        self.presets.iter().map(|r| r.preset.clone()).collect()
    }

    /// Find preset by name (case-insensitive)
    pub fn find_preset(&self, name: &str) -> Option<usize> {
        self.presets
            .iter()
            .position(|r| r.preset.name.eq_ignore_ascii_case(name))
    }

    /// Detect or select initial preset
    pub fn detect_preset(&self, working_dir: &Path) -> Option<usize> {
        // 1. Explicit preset (Env or Arg) has highest priority
        if let Some((ref name, _)) = self.explicit_preset {
            if let Some(idx) = self.find_preset(name) {
                return Some(idx);
            }
            eprintln!("[warn] Specified preset '{}' not found", name);
        }

        // 2. Auto-detect by file/path pattern
        let presets: Vec<_> = self.presets.iter().map(|r| r.preset.clone()).collect();
        detect_preset_idx(working_dir, &presets)
    }

    /// Build switch order: Global presets -> Project presets -> cycle
    ///
    /// Order: detected -> global group -> other group -> back to detected
    #[allow(dead_code)]
    pub fn build_switch_order(&self, detected_idx: Option<usize>) -> Vec<usize> {
        let mut order = Vec::new();

        // 1. Detected preset first (if any)
        if let Some(idx) = detected_idx {
            order.push(idx);
        }

        // 2. Global presets (is_global() == true), excluding detected
        for (i, resolved) in self.presets.iter().enumerate() {
            if resolved.preset.is_global() && Some(i) != detected_idx {
                order.push(i);
            }
        }

        // 3. Other presets (has detection rules), excluding detected
        for (i, resolved) in self.presets.iter().enumerate() {
            if !resolved.preset.is_global() && Some(i) != detected_idx {
                order.push(i);
            }
        }

        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommandConfig;

    fn make_preset(name: &str, detect_file: Option<&str>) -> Preset {
        Preset {
            name: name.to_string(),
            detect_file: detect_file.map(|s| s.to_string()),
            cwd_pattern: None,
            base_color: None,
            default_script: None,
            commands: vec![],
        }
    }

    fn make_config(presets: Vec<Preset>, commands: Vec<CommandConfig>) -> Config {
        Config {
            window: WindowSettings::default(),
            presets,
            commands,
        }
    }

    #[test]
    fn test_priority_project_overrides_global() {
        let mut resolver = PresetResolver::new();

        // Global has "Rust" preset
        let global_config = make_config(vec![make_preset("Rust", Some("Cargo.toml"))], vec![]);
        resolver.add_global(global_config);

        // Project also has "Rust" preset (different detect_file)
        let project_config = make_config(vec![make_preset("Rust", Some("Cargo.lock"))], vec![]);
        resolver.add_project(project_config);

        let resolved = resolver.resolve();
        assert_eq!(resolved.presets.len(), 1);
        assert_eq!(resolved.presets[0].preset.name, "Rust");
        assert_eq!(
            resolved.presets[0].preset.detect_file,
            Some("Cargo.lock".to_string())
        );
        assert_eq!(resolved.presets[0].source, ConfigSource::Project);
    }

    #[test]
    fn test_global_commands_become_preset() {
        let mut resolver = PresetResolver::new();

        let commands = vec![CommandConfig {
            name: "Terminal".to_string(),
            cmd: Some("open -a Terminal .".to_string()),
            run: None,
            script_type: None,
            icon: Some("terminal".to_string()),
            cwd: None,
        }];
        let config = make_config(vec![], commands);
        resolver.add_global(config);

        let resolved = resolver.resolve();
        assert_eq!(resolved.presets.len(), 1);
        assert_eq!(resolved.presets[0].preset.name, GLOBAL_PRESET_NAME);
        assert!(resolved.presets[0].preset.is_global());
    }

    #[test]
    fn test_env_preset_selection() {
        let mut resolver = PresetResolver::new();

        let config = make_config(
            vec![make_preset("Dev", None), make_preset("Prod", None)],
            vec![],
        );
        resolver.add_global(config);
        resolver.set_env_preset("Prod".to_string());

        let resolved = resolver.resolve();
        let detected = resolved.detect_preset(Path::new("."));
        // Verify the selected preset is "Prod" (by name, not index)
        assert!(detected.is_some());
        assert_eq!(resolved.presets[detected.unwrap()].preset.name, "Prod");
    }

    #[test]
    fn test_arg_overrides_env() {
        let mut resolver = PresetResolver::new();

        let config = make_config(
            vec![make_preset("Dev", None), make_preset("Prod", None)],
            vec![],
        );
        resolver.add_global(config);
        resolver.set_env_preset("Prod".to_string());
        resolver.set_arg_preset("Dev".to_string());

        let resolved = resolver.resolve();
        // Arg should win
        assert_eq!(
            resolved.explicit_preset,
            Some(("Dev".to_string(), ConfigSource::Arg))
        );
    }
}
