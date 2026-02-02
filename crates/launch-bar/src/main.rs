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
//!
//! Environment:
//!   LAUNCH_BAR_PRESET - Override preset selection

use std::path::{Path, PathBuf};

use eframe::egui;

mod app;
mod config;
mod platform;
mod script;
mod ui;

use app::LaunchBarApp;
use config::{Config, PresetResolver, ResolvedConfig};
use platform::open_file_with_default_app;
use script::ScriptConfig;
use ui::{available_icons, parse_hex_color};

/// Environment variable for preset override
const ENV_PRESET: &str = "LAUNCH_BAR_PRESET";

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
    let mut arg_preset: Option<String> = None;

    // Handle 'config' subcommand
    if args.len() >= 2 && args[1] == "config" {
        handle_config_subcommand(&args, &global_config_path, &local_config_path);
    }

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--preset" | "-p" => {
                if i + 1 < args.len() {
                    arg_preset = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --preset requires a value");
                    std::process::exit(1);
                }
            }
            "--init" => {
                init_local_config(&local_config_path);
            }
            "--init-global" => {
                init_global_config(&global_config_path);
            }
            "--help" | "-h" => {
                print_help();
            }
            _ => i += 1,
        }
    }

    // Build resolved config using PresetResolver
    let (resolved_config, config_path) =
        resolve_config(&global_config_path, &local_config_path, arg_preset);

    // Detect or select initial preset
    let detected_preset_idx = resolved_config.detect_preset(&working_dir);
    let all_presets = resolved_config.presets();

    let (commands, base_color, preset_name, preset_default_script) =
        if let Some(idx) = detected_preset_idx {
            let preset = &resolved_config.presets[idx].preset;
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
        } else if !all_presets.is_empty() {
            // Use first available preset (usually [Global])
            let preset = &all_presets[0];
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
        } else {
            eprintln!("No presets defined");
            (vec![], egui::Color32::from_rgb(26, 26, 30), None, None)
        };

    let script_config = ScriptConfig {
        global_default: resolved_config.window.default_script,
        preset_default: preset_default_script,
    };

    let commands: Vec<_> = commands
        .into_iter()
        .take(resolved_config.window.max_icons)
        .collect();

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
                resolved_config.window,
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

/// Resolve configuration from all sources using PresetResolver
fn resolve_config(
    global_config_path: &Path,
    local_config_path: &Path,
    arg_preset: Option<String>,
) -> (ResolvedConfig, PathBuf) {
    let mut resolver = PresetResolver::new();

    // 1. Load global config (lowest priority)
    if global_config_path.exists() {
        if let Some(config) = load_config_file(global_config_path) {
            resolver.add_global(config);
        }
    }

    // 2. Load project config (overrides global)
    if local_config_path.exists() {
        if let Some(config) = load_config_file(local_config_path) {
            resolver.add_project(config);
        }
    }

    // 3. CLI argument preset (overrides project)
    if let Some(name) = arg_preset {
        resolver.set_arg_preset(name);
    }

    // 4. Environment variable (highest priority)
    if let Ok(env_preset) = std::env::var(ENV_PRESET) {
        if !env_preset.is_empty() {
            resolver.set_env_preset(env_preset);
        }
    }

    // Resolve and determine active config path
    let resolved = resolver.resolve();

    // If no presets resolved, create example config
    if resolved.presets.is_empty() {
        let example = generate_example_config();
        if let Some(parent) = global_config_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(global_config_path, &example).ok();
        eprintln!(
            "Created example config at: {}",
            global_config_path.display()
        );

        // Re-resolve with the new config
        let mut resolver = PresetResolver::new();
        if let Some(config) = load_config_file(global_config_path) {
            resolver.add_global(config);
        }
        return (resolver.resolve(), global_config_path.to_path_buf());
    }

    // Determine which config path to show (prefer local if exists)
    let config_path = if local_config_path.exists() {
        local_config_path.to_path_buf()
    } else {
        global_config_path.to_path_buf()
    };

    (resolved, config_path)
}

/// Load a config file, returning None on error
fn load_config_file(path: &Path) -> Option<Config> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
}

/// Handle 'config' subcommand
fn handle_config_subcommand(args: &[String], global_config_path: &Path, local_config_path: &Path) {
    let sub_args: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();

    match sub_args.first().copied() {
        Some("open") => {
            let target_path = if sub_args.contains(&"--global") || sub_args.contains(&"-g") {
                if !global_config_path.exists() {
                    eprintln!("Global config not found. Run 'launch-bar --init-global' first.");
                    std::process::exit(1);
                }
                global_config_path.to_path_buf()
            } else if sub_args.contains(&"--local") || sub_args.contains(&"-l") {
                if !local_config_path.exists() {
                    eprintln!("Local config not found. Run 'launch-bar --init' first.");
                    std::process::exit(1);
                }
                local_config_path.to_path_buf()
            } else {
                // Default: local > global
                if local_config_path.exists() {
                    local_config_path.to_path_buf()
                } else if global_config_path.exists() {
                    global_config_path.to_path_buf()
                } else {
                    eprintln!(
                        "No config file found. Run 'launch-bar --init' or '--init-global' first."
                    );
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

/// Initialize local config
fn init_local_config(local_config_path: &Path) {
    if local_config_path.exists() {
        eprintln!(
            "Local config already exists: {}",
            local_config_path.display()
        );
        std::process::exit(1);
    }
    let example = generate_example_config();
    std::fs::write(local_config_path, &example).expect("Failed to write config");
    println!("Created local config: {}", local_config_path.display());
    std::process::exit(0);
}

/// Initialize global config
fn init_global_config(global_config_path: &Path) {
    if let Some(parent) = global_config_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let example = generate_example_config();
    std::fs::write(global_config_path, &example).expect("Failed to write config");
    println!("Created global config: {}", global_config_path.display());
    std::process::exit(0);
}

/// Print help message
fn print_help() {
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
    println!("Environment:");
    println!("  LAUNCH_BAR_PRESET    Override preset selection (highest priority)");
    println!();
    println!("Priority order (later overrides earlier):");
    println!("  1. Global config (~/.config/launch-bar/config.toml)");
    println!("  2. Project config (./launch-bar.toml)");
    println!("  3. CLI argument (--preset)");
    println!("  4. Environment variable (LAUNCH_BAR_PRESET)");
    println!();
    println!("Run 'launch-bar config' for config subcommand help");
    std::process::exit(0);
}

fn generate_example_config() -> String {
    let icons_list = available_icons().join(", ");
    format!(
        r##"# Launch Bar Configuration
# Global config: ~/.config/launch-bar/config.toml
# Local override: ./launch-bar.toml (in project directory)
#
# Priority order (later overrides earlier):
# 1. Global config
# 2. Project config
# 3. CLI argument (--preset)
# 4. Environment variable (LAUNCH_BAR_PRESET)

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
# Fallback commands (becomes [Global] preset when no preset matches)
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
