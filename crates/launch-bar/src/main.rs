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

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use arboard::Clipboard;
use eframe::egui;
use egui_cha_ds::Theme;
use egui_cha_ds::icons;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

// ============================================================================
// Config
// ============================================================================

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    window: WindowSettings,
    #[serde(default)]
    presets: Vec<Preset>,
    #[serde(default)]
    commands: Vec<CommandConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct Preset {
    name: String,
    #[serde(default)]
    detect_file: Option<String>,
    #[serde(default)]
    cwd_pattern: Option<String>,
    #[serde(default)]
    base_color: Option<String>,
    #[serde(default)]
    commands: Vec<CommandConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct CommandConfig {
    name: String,
    cmd: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct WindowSettings {
    #[serde(default = "default_max_icons")]
    max_icons: usize,
    #[serde(default = "default_opacity")]
    opacity: f32,
    #[serde(default)]
    background_color: Option<String>,
    #[serde(default = "default_border")]
    border: String,
    #[serde(default = "default_title_bar")]
    title_bar: String,
    #[serde(default = "default_auto")]
    accent_line: String,
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
        }
    }
}

// ============================================================================
// State Persistence
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Default)]
struct AppState {
    positions: HashMap<String, [f32; 2]>,
}

impl AppState {
    fn load() -> Self {
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

    fn save(&self) {
        let state_path = Self::state_path();
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(content) = toml::to_string_pretty(self) {
            std::fs::write(&state_path, content).ok();
        }
    }

    fn state_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("launch-bar")
            .join("state.toml")
    }

    fn get_position(&self, cwd: &str) -> Option<egui::Pos2> {
        self.positions.get(cwd).map(|p| egui::pos2(p[0], p[1]))
    }

    fn set_position(&mut self, cwd: &str, pos: egui::Pos2) {
        self.positions.insert(cwd.to_string(), [pos.x, pos.y]);
    }
}

// ============================================================================
// Preset Detection
// ============================================================================

fn detect_preset<'a>(working_dir: &PathBuf, presets: &'a [Preset]) -> Option<&'a Preset> {
    for preset in presets {
        // Check detect_file
        if let Some(ref file) = preset.detect_file {
            if working_dir.join(file).exists() {
                return Some(preset);
            }
        }

        // Check cwd_pattern (simple glob: supports * at end)
        if let Some(ref pattern) = preset.cwd_pattern {
            let expanded = shellexpand::tilde(pattern).to_string();
            let cwd_str = working_dir.to_string_lossy();

            if expanded.ends_with('*') {
                let prefix = &expanded[..expanded.len() - 1];
                if cwd_str.starts_with(prefix) {
                    return Some(preset);
                }
            } else if cwd_str == expanded {
                return Some(preset);
            }
        }
    }
    None
}

// ============================================================================
// Color Utilities
// ============================================================================

fn parse_hex_color(hex: &str) -> Option<egui::Color32> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(egui::Color32::from_rgb(r, g, b))
    } else {
        None
    }
}

/// Vary color hue based on path hash for visual distinction
fn vary_color_by_path(base_color: egui::Color32, path: &str) -> egui::Color32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish();

    // Convert to HSV, shift hue slightly, convert back
    let [r, g, b, a] = base_color.to_array();
    let (h, s, v) = rgb_to_hsv(r, g, b);

    // Shift hue by up to ±15 degrees based on hash
    let hue_shift = ((hash % 31) as f32 - 15.0) / 360.0;
    let new_h = (h + hue_shift).rem_euclid(1.0);

    let (nr, ng, nb) = hsv_to_rgb(new_h, s, v);
    egui::Color32::from_rgba_unmultiplied(nr, ng, nb, a)
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };

    let s = if max == 0.0 { 0.0 } else { delta / max };
    let v = max;

    (h, s, v)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;

    let (r, g, b) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

// ============================================================================
// Platform Utilities
// ============================================================================

/// Execute a shell command on the current platform
fn spawn_shell_command(cmd: &str, cwd: &PathBuf) -> std::io::Result<std::process::Child> {
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
fn open_file(path: &PathBuf) {
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

// ============================================================================
// UI Helpers
// ============================================================================

/// Color constants
mod colors {
    use eframe::egui;

    pub const BUTTON_ICON: egui::Color32 = egui::Color32::from_rgb(200, 200, 200);
    pub const STATUS_TEXT: egui::Color32 = egui::Color32::from_rgb(200, 200, 200);
    pub const PRESET_LABEL: egui::Color32 = egui::Color32::from_rgb(150, 150, 150);
    pub const RUNNING_ICON: egui::Color32 = egui::Color32::from_rgb(255, 200, 100);
    pub const SUCCESS_UNDERLINE: egui::Color32 = egui::Color32::from_rgb(100, 200, 100);
    pub const ERROR_UNDERLINE: egui::Color32 = egui::Color32::from_rgb(255, 100, 100);
    pub const ERROR_TEXT: egui::Color32 = egui::Color32::from_rgb(255, 200, 200);
    pub const BASE_BG: egui::Color32 = egui::Color32::from_rgb(26, 26, 30);
}

/// Create a title bar button with consistent styling
fn title_bar_button(ui: &mut egui::Ui, icon: &str, tooltip: &str) -> egui::Response {
    let icon_text = egui::RichText::new(icon)
        .family(egui::FontFamily::Name("icons".into()))
        .size(14.0)
        .color(colors::BUTTON_ICON);
    let button = egui::Button::new(icon_text)
        .fill(egui::Color32::TRANSPARENT)
        .min_size(egui::vec2(20.0, 20.0));
    ui.add(button).on_hover_text(tooltip)
}

// ============================================================================
// Icon Mapping
// ============================================================================

fn get_icon(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "house" | "home" => icons::HOUSE,
        "arrow_left" | "left" => icons::ARROW_LEFT,
        "arrow_right" | "right" => icons::ARROW_RIGHT,
        "plus" | "add" => icons::PLUS,
        "minus" => icons::MINUS,
        "x" | "close" => icons::X,
        "check" | "ok" => icons::CHECK,
        "gear" | "settings" | "config" => icons::GEAR,
        "info" => icons::INFO,
        "warning" | "warn" => icons::WARNING,
        "hash" => icons::HASH,
        "user" => icons::USER,
        "floppy_disk" | "save" => icons::FLOPPY_DISK,
        "trash" | "delete" => icons::TRASH,
        "pencil" | "edit" => icons::PENCIL_SIMPLE,
        "folder" => icons::FOLDER_SIMPLE,
        "file" => icons::FILE,
        "search" | "magnifying_glass" => icons::MAGNIFYING_GLASS,
        "refresh" | "reload" => icons::ARROWS_CLOCKWISE,
        "play" | "run" | "start" => icons::PLAY,
        "pause" => icons::PAUSE,
        "stop" => icons::STOP,
        "record" => icons::RECORD,
        "copy" => icons::COPY,
        "download" => icons::DOWNLOAD_SIMPLE,
        "upload" => icons::UPLOAD_SIMPLE,
        "link" => icons::LINK_SIMPLE,
        "eye" | "view" => icons::EYE,
        "eye_slash" | "hide" => icons::EYE_SLASH,
        "fire" | "hot" => icons::FIRE,
        "bug" | "debug" => icons::BUG,
        "wrench" | "tool" | "build" => icons::WRENCH,
        "x_circle" | "error" => icons::X_CIRCLE,
        "skull" | "danger" => icons::SKULL,
        "caret_up" | "up" => icons::CARET_UP,
        "caret_down" | "down" => icons::CARET_DOWN,
        "lock" => icons::LOCK,
        "lock_open" | "unlock" => icons::LOCK_OPEN,
        "maximize" => icons::CORNERS_OUT,
        "minimize" => icons::CORNERS_IN,
        "stack" | "layers" => icons::STACK,
        "sliders" => icons::SLIDERS_HORIZONTAL,
        "image" => icons::IMAGE,
        "monitor" | "display" => icons::MONITOR_PLAY,
        "grid" => icons::GRID_FOUR,
        "squares" => icons::SQUARES_FOUR,
        "broom" | "clean" => icons::BROOM,
        "zoom" | "zoom_in" => icons::MAGNIFYING_GLASS_PLUS,
        "frame" => icons::FRAME_CORNERS,
        "package" | "cube" => icons::STACK,
        "terminal" | "console" => icons::MONITOR_PLAY,
        "code" => icons::FILE,
        _ => icons::PLAY,
    }
}

fn available_icons() -> Vec<&'static str> {
    vec![
        "play/run/start",
        "check/ok",
        "wrench/tool/build",
        "broom/clean",
        "pencil/edit",
        "trash/delete",
        "gear/settings",
        "bug/debug",
        "refresh/reload",
        "folder",
        "file",
        "plus/add",
        "minus",
        "x/close",
        "search",
        "copy",
        "download",
        "upload",
        "eye/view",
        "fire/hot",
        "lock",
        "unlock",
        "info",
        "warning",
        "stop",
        "pause",
        "home",
        "user",
        "terminal",
        "code",
        "package/cube",
    ]
}

// ============================================================================
// App
// ============================================================================

struct LaunchBarApp {
    commands: Vec<CommandConfig>,
    working_dir: PathBuf,
    working_dir_str: String,
    last_status: Option<String>,
    is_error: bool,
    opacity: f32,
    base_color: egui::Color32,
    border: String,
    title_bar: String,
    accent_line: String,
    saved_position: Option<egui::Pos2>,
    state: AppState,
    preset_name: Option<String>,
    config_path: PathBuf,
    // Process tracking
    running_processes: HashMap<usize, std::process::Child>,
    process_results: HashMap<usize, ProcessResult>,
    // File watcher for highlight
    file_changed: Arc<AtomicBool>,
    highlight_until: Option<Instant>,
    #[allow(dead_code)]
    watcher: Option<RecommendedWatcher>,
}

#[derive(Clone, Copy, PartialEq)]
enum ProcessResult {
    Success,
    Failed,
}

impl LaunchBarApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        commands: Vec<CommandConfig>,
        window: WindowSettings,
        base_color: egui::Color32,
        working_dir: PathBuf,
        preset_name: Option<String>,
        config_path: PathBuf,
    ) -> Self {
        egui_cha_ds::setup_fonts(&cc.egui_ctx);
        let working_dir_str = working_dir.to_string_lossy().to_string();
        let state = AppState::load();

        // Set immediate tooltip
        cc.egui_ctx.style_mut(|style| {
            style.interaction.tooltip_delay = 0.0;
        });

        // Restore saved position
        if let Some(pos) = state.get_position(&working_dir_str) {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }

        // Set up file watcher
        let file_changed = Arc::new(AtomicBool::new(false));
        let file_changed_clone = file_changed.clone();
        let watch_dir = working_dir.clone();

        let watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if let Ok(event) = res {
                // Ignore metadata-only changes
                if !matches!(event.kind, notify::EventKind::Access(_)) {
                    file_changed_clone.store(true, Ordering::SeqCst);
                }
            }
        })
        .ok()
        .and_then(|mut w| {
            w.watch(&watch_dir, RecursiveMode::NonRecursive).ok()?;
            Some(w)
        });

        Self {
            commands,
            working_dir,
            working_dir_str,
            last_status: None,
            is_error: false,
            opacity: window.opacity,
            base_color,
            border: window.border,
            title_bar: window.title_bar,
            accent_line: window.accent_line,
            saved_position: None,
            state,
            preset_name,
            config_path,
            running_processes: HashMap::new(),
            process_results: HashMap::new(),
            file_changed,
            highlight_until: None,
            watcher,
        }
    }

    fn run_command(&mut self, index: usize) {
        if let Some(cmd_config) = self.commands.get(index) {
            let cwd = cmd_config
                .cwd
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.working_dir.clone());

            // Expand $clipboard variable
            let cmd_str = if cmd_config.cmd.contains("$clipboard") {
                match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                    Ok(text) => cmd_config.cmd.replace("$clipboard", &text),
                    Err(_) => {
                        self.last_status = Some("Failed to read clipboard".to_string());
                        self.is_error = true;
                        return;
                    }
                }
            } else {
                cmd_config.cmd.clone()
            };

            let result = spawn_shell_command(&cmd_str, &cwd);

            match result {
                Ok(child) => {
                    // Clear all previous success results when a new command is run
                    self.process_results
                        .retain(|_, v| *v != ProcessResult::Success);
                    self.running_processes.insert(index, child);
                    self.last_status = Some(format!("Running: {}", cmd_config.name));
                    self.is_error = false;
                }
                Err(e) => {
                    self.last_status = Some(format!("Failed: {}", e));
                    self.is_error = true;
                    self.process_results.insert(index, ProcessResult::Failed);
                }
            }
        }
    }

    fn check_processes(&mut self) {
        let mut finished = Vec::new();
        for (&idx, child) in &mut self.running_processes {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let result = if status.success() {
                        ProcessResult::Success
                    } else {
                        ProcessResult::Failed
                    };
                    finished.push((idx, result));
                }
                Ok(None) => {} // Still running
                Err(_) => {
                    finished.push((idx, ProcessResult::Failed));
                }
            }
        }
        for (idx, result) in finished {
            self.running_processes.remove(&idx);
            self.process_results.insert(idx, result);
            if let Some(cmd) = self.commands.get(idx) {
                let status_msg = match result {
                    ProcessResult::Success => format!("Done: {}", cmd.name),
                    ProcessResult::Failed => format!("Failed: {}", cmd.name),
                };
                self.last_status = Some(status_msg);
                self.is_error = result == ProcessResult::Failed;
            }
        }
    }

    fn save_current_position(&mut self, ctx: &egui::Context) {
        let pos = ctx.input(|i| i.viewport().outer_rect.map(|r| r.min));
        if let Some(pos) = pos {
            self.state.set_position(&self.working_dir_str, pos);
            self.state.save();
        }
    }
}

impl eframe::App for LaunchBarApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Save position on exit
        // Note: ctx not available here, but state should be saved via corner button
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::current(ctx);

        // Request periodic repaint to check for file changes
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        // Fixed dark background
        let bg_color = egui::Color32::from_rgba_unmultiplied(
            colors::BASE_BG.r(),
            colors::BASE_BG.g(),
            colors::BASE_BG.b(),
            (self.opacity * 255.0) as u8,
        );

        // Check running processes
        self.check_processes();

        // Check file changes and update highlight state
        if self.file_changed.swap(false, Ordering::SeqCst) {
            self.highlight_until = Some(Instant::now() + std::time::Duration::from_secs(5));
            ctx.request_repaint(); // Ensure UI updates
        }

        // Determine if we should highlight (file change OR window hover)
        let is_file_highlighted = self
            .highlight_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false);
        let is_hovered = ctx.input(|i| i.pointer.has_pointer());
        let is_highlighted = is_file_highlighted || is_hovered;

        // Request repaint while highlighted (for smooth fade)
        if is_file_highlighted {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Preset color for accent line (top border)
        let preset_color = vary_color_by_path(self.base_color, &self.working_dir_str);
        let accent_color = match self.accent_line.as_str() {
            "show" => Some(preset_color),
            "hide" => None,
            _ => {
                // "auto": highlighted = full color, otherwise dimmed
                Some(if is_highlighted {
                    preset_color
                } else {
                    egui::Color32::from_rgba_unmultiplied(
                        (preset_color.r() as u16 / 3 + colors::BASE_BG.r() as u16 * 2 / 3) as u8,
                        (preset_color.g() as u16 / 3 + colors::BASE_BG.g() as u16 * 2 / 3) as u8,
                        (preset_color.b() as u16 / 3 + colors::BASE_BG.b() as u16 * 2 / 3) as u8,
                        180,
                    )
                })
            }
        };

        let show_border = match self.border.as_str() {
            "show" => true,
            "hide" => false,
            _ => self.opacity < 1.0,
        };
        let border_stroke = if show_border {
            egui::Stroke::new(
                1.0,
                egui::Color32::from_rgba_unmultiplied(128, 128, 128, 100),
            )
        } else {
            egui::Stroke::NONE
        };

        let show_title_bar = match self.title_bar.as_str() {
            "show" => true,
            "hide" => false,
            _ => ctx.input(|i| {
                i.pointer
                    .hover_pos()
                    .map(|pos| pos.y < 24.0)
                    .unwrap_or(false)
            }),
        };

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(bg_color)
                    .stroke(border_stroke)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                // Draw colored top accent line (at the very top edge)
                if let Some(color) = accent_color {
                    let rect = ui.max_rect();
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.left(), rect.top() - 10.0),
                            egui::pos2(rect.right(), rect.top() - 10.0),
                        ],
                        egui::Stroke::new(3.0, color),
                    );
                }

                // Window dragging
                let response = ui.interact(
                    ui.max_rect(),
                    ui.id().with("drag_area"),
                    egui::Sense::drag(),
                );
                if response.dragged() {
                    if let Some(_pos) = response.interact_pointer_pos() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }
                // Save position when drag ends
                if response.drag_stopped() {
                    self.save_current_position(ctx);
                }

                // Custom title bar
                if show_title_bar {
                    ui.horizontal(|ui| {
                        // Show preset name on the left
                        if let Some(ref name) = self.preset_name {
                            ui.label(
                                egui::RichText::new(name)
                                    .size(10.0)
                                    .color(colors::PRESET_LABEL),
                            );
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if title_bar_button(ui, icons::X, "Close").clicked() {
                                self.save_current_position(ctx);
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }

                            if title_bar_button(ui, icons::MINUS, "Minimize").clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }

                            let corner_tooltip = if self.saved_position.is_some() {
                                "Return to original"
                            } else {
                                "Move to corner"
                            };
                            if title_bar_button(ui, icons::CORNERS_IN, corner_tooltip).clicked() {
                                if let Some(saved_pos) = self.saved_position.take() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                        saved_pos,
                                    ));
                                    self.save_current_position(ctx);
                                } else {
                                    let info = ctx.input(|i| {
                                        (i.viewport().monitor_size, i.viewport().outer_rect)
                                    });
                                    if let (Some(monitor), Some(outer_rect)) = info {
                                        self.saved_position = Some(outer_rect.min);
                                        let win_size = outer_rect.size();
                                        let new_x = monitor.x - win_size.x - 20.0;
                                        let new_y = monitor.y - win_size.y - 110.0;
                                        ctx.send_viewport_cmd(
                                            egui::ViewportCommand::OuterPosition(egui::pos2(
                                                new_x, new_y,
                                            )),
                                        );
                                    }
                                }
                            }

                            if title_bar_button(ui, icons::GEAR, "Open config").clicked() {
                                open_file(&self.config_path);
                            }
                        });
                    });
                } else {
                    ui.add_space(theme.spacing_sm);
                }

                // Command buttons
                let mut clicked_index = None;
                let mut hovered_index: Option<usize> = None;
                ui.horizontal(|ui| {
                    ui.add_space(theme.spacing_sm);
                    for (index, cmd) in self.commands.iter().enumerate() {
                        let icon = cmd
                            .icon
                            .as_ref()
                            .map(|s| get_icon(s))
                            .unwrap_or(icons::PLAY);

                        // Determine icon color based on process state
                        let is_running = self.running_processes.contains_key(&index);
                        let process_result = self.process_results.get(&index);

                        let icon_color = if is_running {
                            colors::RUNNING_ICON
                        } else {
                            egui::Color32::WHITE
                        };

                        let icon_text = egui::RichText::new(icon)
                            .family(egui::FontFamily::Name("icons".into()))
                            .size(24.0)
                            .color(icon_color);

                        let button = egui::Button::new(icon_text)
                            .fill(egui::Color32::TRANSPARENT)
                            .min_size(egui::vec2(40.0, 40.0));

                        let response = ui.add(button);

                        // Track hovered command
                        if response.hovered() {
                            hovered_index = Some(index);
                        }

                        // Draw underline for finished processes
                        if let Some(result) = process_result {
                            let underline_color = match result {
                                ProcessResult::Success => colors::SUCCESS_UNDERLINE,
                                ProcessResult::Failed => colors::ERROR_UNDERLINE,
                            };
                            let rect = response.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left() + 5.0, rect.bottom() - 2.0),
                                    egui::pos2(rect.right() - 5.0, rect.bottom() - 2.0),
                                ],
                                egui::Stroke::new(2.0, underline_color),
                            );
                        }

                        if response.clicked() {
                            clicked_index = Some(index);
                        }
                    }
                });

                if let Some(index) = clicked_index {
                    self.run_command(index);
                }

                // Bottom line: show hovered command info or status
                ui.add_space(theme.spacing_xs);
                if let Some(idx) = hovered_index {
                    if let Some(cmd) = self.commands.get(idx) {
                        ui.label(
                            egui::RichText::new(format!("{}: {}", cmd.name, cmd.cmd))
                                .color(colors::STATUS_TEXT)
                                .size(theme.font_size_xs),
                        );
                    }
                } else if let Some(status) = &self.last_status {
                    let color = if self.is_error {
                        colors::ERROR_TEXT
                    } else {
                        egui::Color32::WHITE
                    };
                    ui.label(
                        egui::RichText::new(status)
                            .color(color)
                            .size(theme.font_size_xs),
                    );
                }
            });
    }
}

// ============================================================================
// Main
// ============================================================================

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
                println!("Usage: launch-bar [OPTIONS]");
                println!();
                println!("Options:");
                println!("  -p, --preset <NAME>  Use specific preset");
                println!("      --init           Create local config (./launch-bar.toml)");
                println!("      --init-global    Create/reset global config");
                println!("  -h, --help           Show this help");
                std::process::exit(0);
            }
            _ => i += 1,
        }
    }

    let (config, config_path): (Config, PathBuf) = if local_config_path.exists() {
        let content =
            std::fs::read_to_string(&local_config_path).expect("Failed to read launch-bar.toml");
        (
            toml::from_str(&content).expect("Failed to parse launch-bar.toml"),
            local_config_path,
        )
    } else if global_config_path.exists() {
        let content =
            std::fs::read_to_string(&global_config_path).expect("Failed to read global config");
        (
            toml::from_str(&content).expect("Failed to parse global config"),
            global_config_path.clone(),
        )
    } else {
        // Create example config
        let example = generate_example_config();
        if let Some(parent) = global_config_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&global_config_path, &example).ok();
        eprintln!(
            "Created example config at: {}",
            global_config_path.display()
        );
        (toml::from_str(&example).unwrap(), global_config_path)
    };

    // Find preset: explicit > auto-detect
    let detected_preset = if let Some(ref name) = explicit_preset {
        config
            .presets
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    } else {
        detect_preset(&working_dir, &config.presets)
    };
    let (commands, base_color, preset_name) = if let Some(preset) = detected_preset {
        let color = preset
            .base_color
            .as_ref()
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(egui::Color32::from_rgb(26, 26, 30));
        (preset.commands.clone(), color, Some(preset.name.clone()))
    } else if !config.commands.is_empty() {
        let color = config
            .window
            .background_color
            .as_ref()
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(egui::Color32::from_rgb(26, 26, 30));
        (config.commands.clone(), color, None)
    } else {
        // No preset matched, no fallback commands
        eprintln!("No preset matched and no fallback commands defined");
        (vec![], egui::Color32::from_rgb(26, 26, 30), None)
    };

    // Limit commands to max_icons
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
            )))
        }),
    )
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

# ============================================================================
# Presets - Auto-detected by file or path pattern
# ============================================================================

[[presets]]
name = "RustDev"
detect_file = "Cargo.toml"
base_color = "#FF7043"     # Deep Orange
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
commands = [
    {{ name = "Run", cmd = "python main.py", icon = "play" }},
    {{ name = "Test", cmd = "pytest", icon = "check" }},
    {{ name = "Lint", cmd = "ruff check .", icon = "eye" }},
    {{ name = "Fmt", cmd = "ruff format .", icon = "edit" }},
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

# Available icons: {icons}
"##,
        icons = icons_list
    )
}
