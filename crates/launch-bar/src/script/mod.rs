//! Script engine abstraction for multi-language support
//!
//! Supports Rhai and Lua scripting with configurable defaults.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

#[cfg(feature = "lua-script")]
mod lua_engine;
#[cfg(feature = "rhai-script")]
mod rhai_engine;

/// Script language type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptType {
    #[default]
    Rhai,
    Lua,
}

impl ScriptType {
    /// Detect from file extension
    pub fn from_extension(path: &str) -> Option<Self> {
        if path.ends_with(".rhai") {
            Some(Self::Rhai)
        } else if path.ends_with(".lua") {
            Some(Self::Lua)
        } else {
            None
        }
    }
}

/// Configuration for script defaults
#[derive(Debug, Clone, Default)]
pub struct ScriptConfig {
    pub global_default: Option<ScriptType>,
    pub preset_default: Option<ScriptType>,
}

/// Resolve script type with priority:
/// 1. Explicit script_type on command
/// 2. File extension (for @path references)
/// 3. Preset default
/// 4. Global default
/// 5. Fallback to Rhai
pub fn resolve_script_type(
    explicit: Option<ScriptType>,
    script: &str,
    config: &ScriptConfig,
) -> ScriptType {
    // 1. Explicit type takes priority
    if let Some(t) = explicit {
        return t;
    }

    // 2. File reference with extension
    if let Some(path) = script.strip_prefix('@') {
        if let Some(t) = ScriptType::from_extension(path) {
            return t;
        }
    }

    // 3. Preset default
    if let Some(t) = config.preset_default {
        return t;
    }

    // 4. Global default
    if let Some(t) = config.global_default {
        return t;
    }

    // 5. Fallback
    ScriptType::Rhai
}

/// Script execution result
pub struct ScriptResult {
    pub success: bool,
    pub message: String,
}

/// Execute a script with the specified type
pub fn run_script(script: &str, script_type: ScriptType, cwd: Arc<PathBuf>) -> ScriptResult {
    // Handle file reference (@path)
    let (actual_script, actual_cwd) = if let Some(path) = script.strip_prefix('@') {
        let full_path = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            cwd.join(path)
        };

        match std::fs::read_to_string(&full_path) {
            Ok(content) => {
                let script_dir = full_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| cwd.as_ref().clone());
                (content, Arc::new(script_dir))
            }
            Err(e) => {
                return ScriptResult {
                    success: false,
                    message: format!("Failed to read script file: {}", e),
                };
            }
        }
    } else {
        (script.to_string(), cwd)
    };

    match script_type {
        #[cfg(feature = "rhai-script")]
        ScriptType::Rhai => rhai_engine::run(&actual_script, actual_cwd),

        #[cfg(not(feature = "rhai-script"))]
        ScriptType::Rhai => ScriptResult {
            success: false,
            message: "Rhai support not compiled in".to_string(),
        },

        #[cfg(feature = "lua-script")]
        ScriptType::Lua => lua_engine::run(&actual_script, actual_cwd),

        #[cfg(not(feature = "lua-script"))]
        ScriptType::Lua => ScriptResult {
            success: false,
            message: "Lua support not compiled in".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_type_from_extension() {
        assert_eq!(
            ScriptType::from_extension("test.rhai"),
            Some(ScriptType::Rhai)
        );
        assert_eq!(
            ScriptType::from_extension("test.lua"),
            Some(ScriptType::Lua)
        );
        assert_eq!(ScriptType::from_extension("test.py"), None);
    }

    #[test]
    fn test_resolve_script_type_explicit() {
        let config = ScriptConfig::default();
        assert_eq!(
            resolve_script_type(Some(ScriptType::Lua), "anything", &config),
            ScriptType::Lua
        );
    }

    #[test]
    fn test_resolve_script_type_extension() {
        let config = ScriptConfig::default();
        assert_eq!(
            resolve_script_type(None, "@scripts/test.lua", &config),
            ScriptType::Lua
        );
    }

    #[test]
    fn test_resolve_script_type_preset_default() {
        let config = ScriptConfig {
            global_default: None,
            preset_default: Some(ScriptType::Lua),
        };
        assert_eq!(
            resolve_script_type(None, "inline code", &config),
            ScriptType::Lua
        );
    }

    #[test]
    fn test_resolve_script_type_global_default() {
        let config = ScriptConfig {
            global_default: Some(ScriptType::Lua),
            preset_default: None,
        };
        assert_eq!(
            resolve_script_type(None, "inline code", &config),
            ScriptType::Lua
        );
    }

    #[test]
    fn test_resolve_script_type_fallback() {
        let config = ScriptConfig::default();
        assert_eq!(
            resolve_script_type(None, "inline code", &config),
            ScriptType::Rhai
        );
    }
}
