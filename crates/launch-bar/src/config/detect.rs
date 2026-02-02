//! Preset detection based on project files and paths

use std::path::Path;

use super::Preset;

/// Detect matching preset for the working directory
#[allow(dead_code)]
pub fn detect_preset<'a>(working_dir: &Path, presets: &'a [Preset]) -> Option<&'a Preset> {
    detect_preset_idx(working_dir, presets).and_then(|i| presets.get(i))
}

/// Detect matching preset index for the working directory
pub fn detect_preset_idx(working_dir: &Path, presets: &[Preset]) -> Option<usize> {
    for (i, preset) in presets.iter().enumerate() {
        // Check detect_file
        if let Some(ref file) = preset.detect_file {
            if working_dir.join(file).exists() {
                return Some(i);
            }
        }

        // Check cwd_pattern (simple glob: supports * at end)
        if let Some(ref pattern) = preset.cwd_pattern {
            let expanded = shellexpand::tilde(pattern).to_string();
            let cwd_str = working_dir.to_string_lossy();

            if expanded.ends_with('*') {
                let prefix = &expanded[..expanded.len() - 1];
                if cwd_str.starts_with(prefix) {
                    return Some(i);
                }
            } else if cwd_str == expanded {
                return Some(i);
            }
        }
    }
    None
}
