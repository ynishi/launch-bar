# launch-bar

Context-aware command launcher with icon buttons for developers.

[![Crates.io](https://img.shields.io/crates/v/launch-bar.svg)](https://crates.io/crates/launch-bar)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![GitHub](https://img.shields.io/github/stars/ynishi/launch-bar?style=social)](https://github.com/ynishi/launch-bar)

## Features

- **Auto-detection**: Detects project type by file presence (Cargo.toml, package.json, etc.)
- **Preset system**: Configurable command sets per project type with custom colors
- **Preset switching**: Hot-switch presets from title bar without restart
- **Scripting**: Rhai and Lua scripting support for advanced automation
- **Visual feedback**: Process status indicators (running/success/failed)
- **File watcher**: Highlights when files in the working directory change
- **Position memory**: Remembers window position per directory
- **Clipboard support**: Use `$clipboard` variable in commands
- **Transparent UI**: Semi-transparent window with customizable opacity

## Installation

```bash
cargo install launch-bar
```

## Usage

```bash
# Run in current directory (auto-detects project type)
launch-bar

# Use specific preset
launch-bar --preset RustDev

# Create local config in current directory
launch-bar --init

# Create/reset global config
launch-bar --init-global
```

## Configuration

Configuration files are loaded in this order:
1. `./launch-bar.toml` (local, highest priority)
2. `~/.config/launch-bar/config.toml` (global)

### Example config

```toml
[window]
max_icons = 5              # Maximum icons to display
opacity = 0.8              # Background opacity (0.0 - 1.0)
border = "auto"            # "auto", "show", "hide"
title_bar = "auto"         # "auto" (hover), "show", "hide"
accent_line = "auto"       # "auto" (highlight on change/hover), "show", "hide"
# default_script = "rhai"  # Global default script type: "rhai" or "lua"

[[presets]]
name = "RustDev"
detect_file = "Cargo.toml"
base_color = "#FF7043"
commands = [
    { name = "Run", cmd = "cargo run", icon = "play" },
    { name = "Test", cmd = "cargo test", icon = "check" },
    { name = "Build", cmd = "cargo build --release", icon = "wrench" },
    { name = "Clean", cmd = "cargo clean", icon = "broom" },
    { name = "Fmt", cmd = "cargo fmt", icon = "edit" },
]

[[presets]]
name = "NodeDev"
detect_file = "package.json"
base_color = "#66BB6A"
commands = [
    { name = "Start", cmd = "npm start", icon = "play" },
    { name = "Test", cmd = "npm test", icon = "check" },
    { name = "Build", cmd = "npm run build", icon = "wrench" },
]
```

### Preset options

| Field | Description |
|-------|-------------|
| `name` | Preset identifier |
| `detect_file` | Auto-detect by file presence |
| `cwd_pattern` | Auto-detect by path pattern (supports `*` suffix) |
| `base_color` | Hex color for accent line |
| `default_script` | Default script type for this preset (`rhai` or `lua`) |
| `commands` | List of command configurations |

### Command options

| Field | Description |
|-------|-------------|
| `name` | Display name |
| `cmd` | Shell command to execute (supports `$clipboard`) |
| `run` | Script to execute (Rhai or Lua) |
| `script_type` | Force script type: `rhai` or `lua` |
| `icon` | Icon name (see available icons below) |
| `cwd` | Working directory override |

## Scripting

Commands can use `run` instead of `cmd` to execute scripts. Both Rhai and Lua are supported.

### Script type resolution

Script type is determined in this order:
1. Explicit `script_type` on the command
2. File extension for `@path` references (`.rhai` or `.lua`)
3. Preset's `default_script`
4. Window's `default_script` (global)
5. Fallback: `rhai`

### Example

```toml
[window]
default_script = "lua"  # Global default

[[presets]]
name = "Scripts"
default_script = "rhai"  # Preset default
commands = [
    # Inline script (uses preset default = rhai)
    { name = "Greet", run = "notify(\"Hello!\")", icon = "info" },

    # Script from file (type detected by .lua extension)
    { name = "Build", run = "@scripts/build.lua", icon = "wrench" },

    # Explicit script type
    { name = "Test", run = "shell('cargo test')", script_type = "lua", icon = "check" },
]
```

### Available functions

| Function | Description |
|----------|-------------|
| `clipboard()` | Get clipboard text |
| `clipboard_set(text)` | Set clipboard text |
| `shell(cmd)` | Run command and return output |
| `shell_spawn(cmd)` | Run command in background |
| `claude(prompt)` | Call Claude CLI with prompt |
| `notify(message)` | Show system notification |
| `open(path)` | Open file/URL with system default |
| `env(name)` | Get environment variable |
| `read_file(path)` | Read file contents |
| `write_file(path, content)` | Write to file |

### Available icons

`play`, `check`, `wrench`, `broom`, `edit`, `trash`, `gear`, `bug`, `refresh`, `folder`, `file`, `plus`, `minus`, `x`, `search`, `copy`, `download`, `upload`, `eye`, `fire`, `lock`, `unlock`, `info`, `warning`, `stop`, `pause`, `home`, `user`, `terminal`, `code`, `package`

## Window controls

Hover over the top area to reveal the title bar:

- **Preset switch** (🔄 icon): Cycle through available presets
- **Settings** (gear icon): Open config file
- **Corner** (corners icon): Move to bottom-right corner / Return to original position
- **Minimize** (minus icon): Minimize window
- **Close** (x icon): Close application

## License

MIT
