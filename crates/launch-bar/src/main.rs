//! Launch Bar - Context-aware command launcher with icon buttons
//!
//! Features:
//! - Auto-detects project type (Rust, Node, etc.)
//! - Color-coded by project context
//! - Remembers window position per directory
//! - Supports $clipboard variable in commands
//!
//! Usage:
//!   launch-bar [--preset <name>]

use std::path::PathBuf;

use eframe::egui;

mod app;
mod config;
mod platform;
mod script;
mod ui;

use app::LaunchBarApp;
use config::{detect_preset_idx, Config};
use platform::open_file_with_default_app;
use script::ScriptConfig;
use ui::{available_icons, parse_hex_color};

fn main() -> eframe::Result<()> {
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Config paths
    let global_config_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("launch-bar")
        .join("config.toml");
    let local_config_path = working_dir.join("launch-bar.toml");

    // Parse CLI arguments
    let args: Vec<String> = std::env::args().collect();
    let mut explicit_preset: Option<String> = None;

    // Handle 'config' subcommand
    if args.len() >= 2 && args[1] == "config" {
        let sub_args: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();

        match sub_args.first().copied() {
            Some("open") => {
                let target_path = if sub_args.contains(&"--global") || sub_args.contains(&"-g") {
                    if !global_config_path.exists() {
                        eprintln!("Global config not found. Run 'launch-bar --init-global' first.");
                        std::process::exit(1);
                    }
                    global_config_path.clone()
                } else if sub_args.contains(&"--local") || sub_args.contains(&"-l") {
                    if !local_config_path.exists() {
                        eprintln!("Local config not found. Run 'launch-bar --init' first.");
                        std::process::exit(1);
                    }
                    local_config_path.clone()
                } else {
                    // Default: local > global
                    if local_config_path.exists() {
                        local_config_path.clone()
                    } else if global_config_path.exists() {
                        global_config_path.clone()
                    } else {
                        eprintln!("No config file found. Run 'launch-bar --init' or '--init-global' first.");
                        std::process::exit(1);
                    }
                };
                println!("Opening: {}", target_path.display());
                if let Err(e) = open_file_with_default_app(&target_path) {
                    eprintln!("Failed to open config: {}", e);
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
            Some("path") => {
                if sub_args.contains(&"--global") || sub_args.contains(&"-g") {
                    println!("{}", global_config_path.display());
                } else if sub_args.contains(&"--local") || sub_args.contains(&"-l") {
                    println!("{}", local_config_path.display());
                } else {
                    println!("Global: {}", global_config_path.display());
                    println!("Local:  {}", local_config_path.display());
                    if local_config_path.exists() {
                        println!("Active: {} (local)", local_config_path.display());
                    } else if global_config_path.exists() {
                        println!("Active: {} (global)", global_config_path.display());
                    } else {
                        println!("Active: (none)");
                    }
                }
                std::process::exit(0);
            }
            Some(cmd) => {
                eprintln!("Unknown config subcommand: {}", cmd);
                eprintln!("Available: open, path");
                std::process::exit(1);
            }
            None => {
                println!("Usage: launch-bar config <COMMAND>");
                println!();
                println!("Commands:");
                println!("  open [--global|-g] [--local|-l]  Open config in default editor");
                println!("  path [--global|-g] [--local|-l]  Show config file path(s)");
                std::process::exit(0);
            }
        }
    }

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--preset" | "-p" => {
                if i + 1 < args.len() {
                    explicit_preset = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --preset requires a value");
                    std::process::exit(1);
                }
            }
            "--init" => {
                if local_config_path.exists() {
                    eprintln!(
                        "Local config already exists: {}",
                        local_config_path.display()
                    );
                    std::process::exit(1);
                }
                let example = generate_example_config();
                std::fs::write(&local_config_path, &example).expect("Failed to write config");
                println!("Created local config: {}", local_config_path.display());
                std::process::exit(0);
            }
            "--init-global" => {
                if let Some(parent) = global_config_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let example = generate_example_config();
                std::fs::write(&global_config_path, &example).expect("Failed to write config");
                println!("Created global config: {}", global_config_path.display());
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("Usage: launch-bar [OPTIONS] [COMMAND]");
                println!();
                println!("Commands:");
                println!("  config               Manage configuration files");
                println!();
                println!("Options:");
                println!("  -p, --preset <NAME>  Use specific preset");
                println!("      --init           Create local config (./launch-bar.toml)");
                println!("      --init-global    Create/reset global config");
                println!("  -h, --help           Show this help");
                println!();
                println!("Run 'launch-bar config' for config subcommand help");
                std::process::exit(0);
            }
            _ => i += 1,
        }
    }

    // Load and merge configs
    let (config, config_path) = load_config(&global_config_path, &local_config_path);

    // Find preset: explicit > auto-detect
    let detected_preset_idx: Option<usize> = if let Some(ref name) = explicit_preset {
        config
            .presets
            .iter()
            .position(|p| p.name.eq_ignore_ascii_case(name))
    } else {
        detect_preset_idx(&working_dir, &config.presets)
    };
    let detected_preset = detected_preset_idx.and_then(|i| config.presets.get(i));

    let (commands, base_color, preset_name, preset_default_script) =
        if let Some(preset) = detected_preset {
            let color = preset
                .base_color
                .as_ref()
                .and_then(|c| parse_hex_color(c))
                .unwrap_or(egui::Color32::from_rgb(26, 26, 30));
            (
                preset.commands.clone(),
                color,
                Some(preset.name.clone()),
                preset.default_script,
            )
        } else if !config.commands.is_empty() {
            let color = config
                .window
                .background_color
                .as_ref()
                .and_then(|c| parse_hex_color(c))
                .unwrap_or(egui::Color32::from_rgb(26, 26, 30));
            (config.commands.clone(), color, None, None)
        } else {
            eprintln!("No preset matched and no fallback commands defined");
            (vec![], egui::Color32::from_rgb(26, 26, 30), None, None)
        };

    let all_presets = config.presets.clone();
    let script_config = ScriptConfig {
        global_default: config.window.default_script,
        preset_default: preset_default_script,
    };

    let commands: Vec<_> = commands.into_iter().take(config.window.max_icons).collect();

    let num_commands = commands.len().max(1);
    let width = (num_commands as f32 * 56.0) + 48.0;
    let height = 100.0;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top(),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Launch Bar",
        options,
        Box::new(move |cc| {
            Ok(Box::new(LaunchBarApp::new(
                cc,
                commands,
                config.window,
                base_color,
                working_dir,
                preset_name,
                config_path,
                script_config,
                all_presets,
                detected_preset_idx,
            )))
        }),
    )
}

/// Load and merge configuration files
fn load_config(global_config_path: &PathBuf, local_config_path: &PathBuf) -> (Config, PathBuf) {
    let global_config: Option<Config> = if global_config_path.exists() {
        std::fs::read_to_string(global_config_path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
    } else {
        None
    };

    let local_config: Option<Config> = if local_config_path.exists() {
        std::fs::read_to_string(local_config_path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
    } else {
        None
    };

    match (local_config, global_config) {
        (Some(mut local), Some(global)) => {
            // Merge: local presets + global presets (skip duplicates)
            let local_names: std::collections::HashSet<_> = local
                .presets
                .iter()
                .map(|p| p.name.to_lowercase())
                .collect();
            for preset in global.presets {
                if !local_names.contains(&preset.name.to_lowercase()) {
                    local.presets.push(preset);
                }
            }
            // Merge fallback commands if local has none
            if local.commands.is_empty() {
                local.commands = global.commands;
            }
            (local, local_config_path.clone())
        }
        (Some(local), None) => (local, local_config_path.clone()),
        (None, Some(global)) => (global, global_config_path.clone()),
        (None, None) => {
            // Create example config
            let example = generate_example_config();
            if let Some(parent) = global_config_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(global_config_path, &example).ok();
            eprintln!(
                "Created example config at: {}",
                global_config_path.display()
            );
            (
                toml::from_str(&example).unwrap(),
                global_config_path.clone(),
            )
        }
    }
}

fn generate_example_config() -> String {
    let icons_list = available_icons().join(", ");
    format!(
        r##"# Launch Bar Configuration
# Global config: ~/.config/launch-bar/config.toml
# Local override: ./launch-bar.toml (in project directory)

[window]
max_icons = 5              # Maximum icons to display
opacity = 0.8              # Background opacity (0.0 - 1.0)
border = "auto"            # "auto", "show", "hide"
title_bar = "auto"         # "auto" (hover), "show", "hide"
accent_line = "auto"       # "auto" (highlight on change/hover), "show", "hide"
# default_script = "rhai"  # Global default: "rhai" or "lua"

# ============================================================================
# Scripting
# ============================================================================
# Scripts can be written in Rhai or Lua. Script type is resolved in this order:
# 1. Explicit script_type on command
# 2. File extension for @path references (.rhai or .lua)
# 3. Preset's default_script
# 4. Window's default_script (global)
# 5. Fallback: rhai
#
# Available functions: clipboard(), clipboard_set(text), shell(cmd),
#   shell_spawn(cmd), claude(prompt), notify(msg), open(path),
#   env(name), read_file(path), write_file(path, content)

# ============================================================================
# Presets - Auto-detected by file or path pattern
# ============================================================================

[[presets]]
name = "RustDev"
detect_file = "Cargo.toml"
base_color = "#FF7043"     # Deep Orange
# default_script = "rhai"  # Preset default script type
commands = [
    {{ name = "Run", cmd = "cargo run", icon = "play" }},
    {{ name = "Test", cmd = "cargo test", icon = "check" }},
    {{ name = "Build", cmd = "cargo build --release", icon = "wrench" }},
    {{ name = "Clean", cmd = "cargo clean", icon = "broom" }},
    {{ name = "Fmt", cmd = "cargo fmt", icon = "edit" }},
]

[[presets]]
name = "NodeDev"
detect_file = "package.json"
base_color = "#66BB6A"     # Green
commands = [
    {{ name = "Start", cmd = "npm start", icon = "play" }},
    {{ name = "Test", cmd = "npm test", icon = "check" }},
    {{ name = "Build", cmd = "npm run build", icon = "wrench" }},
    {{ name = "Lint", cmd = "npm run lint", icon = "eye" }},
    {{ name = "Install", cmd = "npm install", icon = "download" }},
]

[[presets]]
name = "Python"
detect_file = "pyproject.toml"
base_color = "#42A5F5"     # Blue
default_script = "lua"     # Use Lua for this preset
commands = [
    {{ name = "Run", cmd = "python main.py", icon = "play" }},
    {{ name = "Test", cmd = "pytest", icon = "check" }},
    {{ name = "Lint", cmd = "ruff check .", icon = "eye" }},
    {{ name = "Fmt", cmd = "ruff format .", icon = "edit" }},
    # Script example (uses preset's default_script = "lua")
    {{ name = "Info", run = "notify('Python project')", icon = "info" }},
]

# ============================================================================
# Fallback commands (when no preset matches)
# ============================================================================

[[commands]]
name = "Terminal"
cmd = "open -a Terminal ."
icon = "terminal"

[[commands]]
name = "Finder"
cmd = "open ."
icon = "folder"

# Script with explicit type
# {{ name = "Greet", run = "notify('Hello!')", script_type = "rhai", icon = "info" }}

# Script from file (type detected by extension)
# {{ name = "Custom", run = "@scripts/custom.lua", icon = "code" }}

# Available icons: {icons}
"##,
        icons = icons_list
    )
}
