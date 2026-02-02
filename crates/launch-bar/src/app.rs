//! Launch Bar application

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use arboard::Clipboard;
use eframe::egui;
use egui_cha_ds::icons;
use egui_cha_ds::Theme;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::{AppState, CommandConfig, Preset, WindowSettings};
use crate::platform::{open_file, spawn_shell_command};
use crate::script::{resolve_script_type, run_script, ScriptConfig, ScriptType};
use crate::ui::{get_icon, palette, parse_hex_color, title_bar_button, vary_color_by_path};

/// Result from async script execution (internal)
struct AsyncScriptResult {
    index: usize,
    success: bool,
    message: String,
}

/// Process execution result
#[derive(Clone, Copy, PartialEq)]
enum ProcessResult {
    Success,
    Failed,
}

/// Main application state
pub struct LaunchBarApp {
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
    script_config: ScriptConfig,
    // Process tracking
    running_processes: HashMap<usize, std::process::Child>,
    process_results: HashMap<usize, ProcessResult>,
    running_scripts: std::collections::HashSet<usize>,
    script_rx: Receiver<AsyncScriptResult>,
    script_tx: Sender<AsyncScriptResult>,
    // File watcher for highlight
    file_changed: Arc<AtomicBool>,
    highlight_until: Option<Instant>,
    #[allow(dead_code)]
    watcher: Option<RecommendedWatcher>,
    // Preset switching
    all_presets: Vec<Preset>,
    preset_order: Vec<usize>,
    current_preset_idx: usize,
    max_icons: usize,
    global_default_script: Option<ScriptType>,
}

impl LaunchBarApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        commands: Vec<CommandConfig>,
        window: WindowSettings,
        base_color: egui::Color32,
        working_dir: PathBuf,
        preset_name: Option<String>,
        config_path: PathBuf,
        script_config: ScriptConfig,
        all_presets: Vec<Preset>,
        detected_preset_idx: Option<usize>,
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

        let (script_tx, script_rx) = mpsc::channel();

        // Build preset switching order: detected -> global -> others
        let preset_order = Self::build_preset_order(&all_presets, detected_preset_idx);
        let current_preset_idx = 0;

        let max_icons = window.max_icons;
        let global_default_script = window.default_script;

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
            script_config,
            running_processes: HashMap::new(),
            process_results: HashMap::new(),
            running_scripts: std::collections::HashSet::new(),
            script_rx,
            script_tx,
            file_changed,
            highlight_until: None,
            watcher,
            all_presets,
            preset_order,
            current_preset_idx,
            max_icons,
            global_default_script,
        }
    }

    /// Build preset order for switching: detected preset first, then globals, then others
    fn build_preset_order(presets: &[Preset], detected_idx: Option<usize>) -> Vec<usize> {
        let mut order = Vec::new();

        // 1. Detected preset first (if any)
        if let Some(idx) = detected_idx {
            order.push(idx);
        }

        // 2. Global presets (no detect_file, no cwd_pattern)
        for (i, preset) in presets.iter().enumerate() {
            if preset.is_global() && Some(i) != detected_idx {
                order.push(i);
            }
        }

        // 3. Other presets (has detection rules but wasn't detected)
        for (i, preset) in presets.iter().enumerate() {
            if !preset.is_global() && Some(i) != detected_idx {
                order.push(i);
            }
        }

        order
    }

    /// Switch to next preset in the cycle order
    fn switch_to_next_preset(&mut self) {
        if self.preset_order.is_empty() {
            return;
        }

        // Move to next preset in order (wrap around)
        self.current_preset_idx = (self.current_preset_idx + 1) % self.preset_order.len();
        let preset_idx = self.preset_order[self.current_preset_idx];

        if let Some(preset) = self.all_presets.get(preset_idx) {
            // Update commands
            self.commands = preset
                .commands
                .iter()
                .take(self.max_icons)
                .cloned()
                .collect();

            // Update base color
            self.base_color = preset
                .base_color
                .as_ref()
                .and_then(|c| parse_hex_color(c))
                .unwrap_or(palette::BASE_BG);

            // Update preset name
            self.preset_name = Some(preset.name.clone());

            // Update script config
            self.script_config = ScriptConfig {
                global_default: self.global_default_script,
                preset_default: preset.default_script,
            };

            // Clear running state
            self.running_processes.clear();
            self.process_results.clear();
            self.running_scripts.clear();
            self.last_status = Some(format!("Switched to: {}", preset.name));
            self.is_error = false;
        }
    }

    fn run_command(&mut self, index: usize) {
        if let Some(cmd_config) = self.commands.get(index) {
            let cwd = cmd_config
                .cwd
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.working_dir.clone());

            // Script execution (async)
            if let Some(ref script) = cmd_config.run {
                // Warn if both cmd and run are set
                if cmd_config.cmd.is_some() {
                    eprintln!(
                        "[warn] Command '{}' has both 'cmd' and 'run' set; 'run' takes priority",
                        cmd_config.name
                    );
                }

                // Don't run if already running
                if self.running_scripts.contains(&index) {
                    return;
                }

                self.running_scripts.insert(index);
                self.last_status = Some(format!("Running: {}", cmd_config.name));
                self.is_error = false;

                let script = script.clone();
                let script_type =
                    resolve_script_type(cmd_config.script_type, &script, &self.script_config);
                let cwd = Arc::new(cwd);
                let tx = self.script_tx.clone();

                std::thread::spawn(move || {
                    // Catch panics to ensure tx.send is always called
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_script(&script, script_type, cwd)
                    }));

                    let (success, message) = match result {
                        Ok(r) => (r.success, r.message),
                        Err(_) => (false, "Script panicked".to_string()),
                    };

                    let _ = tx.send(AsyncScriptResult {
                        index,
                        success,
                        message,
                    });
                });
                return;
            }

            // Shell command execution
            if let Some(ref cmd) = cmd_config.cmd {
                // Expand $clipboard variable
                let cmd_str = if cmd.contains("$clipboard") {
                    match Clipboard::new().and_then(|mut cb| cb.get_text()) {
                        Ok(text) => cmd.replace("$clipboard", &text),
                        Err(_) => {
                            self.last_status = Some("Failed to read clipboard".to_string());
                            self.is_error = true;
                            return;
                        }
                    }
                } else {
                    cmd.clone()
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
            } else {
                self.last_status = Some("No command or script defined".to_string());
                self.is_error = true;
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

    fn check_scripts(&mut self) {
        while let Ok(result) = self.script_rx.try_recv() {
            self.running_scripts.remove(&result.index);
            let proc_result = if result.success {
                ProcessResult::Success
            } else {
                ProcessResult::Failed
            };
            self.process_results.insert(result.index, proc_result);

            if let Some(cmd) = self.commands.get(result.index) {
                let status_msg = if result.success {
                    format!("Done: {}", cmd.name)
                } else {
                    result.message
                };
                self.last_status = Some(status_msg);
                self.is_error = !result.success;
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
            palette::BASE_BG.r(),
            palette::BASE_BG.g(),
            palette::BASE_BG.b(),
            (self.opacity * 255.0) as u8,
        );

        // Check running processes and scripts
        self.check_processes();
        self.check_scripts();

        // Check file changes and update highlight state
        if self.file_changed.swap(false, Ordering::SeqCst) {
            self.highlight_until = Some(Instant::now() + std::time::Duration::from_secs(5));
            ctx.request_repaint();
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
                        (preset_color.r() as u16 / 3 + palette::BASE_BG.r() as u16 * 2 / 3) as u8,
                        (preset_color.g() as u16 / 3 + palette::BASE_BG.g() as u16 * 2 / 3) as u8,
                        (preset_color.b() as u16 / 3 + palette::BASE_BG.b() as u16 * 2 / 3) as u8,
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

        let mut switch_preset = false;

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

                // Custom title bar (always reserve space, only show icons when enabled)
                ui.horizontal(|ui| {
                    ui.set_min_height(20.0);

                    if show_title_bar {
                        // Show preset name on the left
                        if let Some(ref name) = self.preset_name {
                            ui.label(
                                egui::RichText::new(name)
                                    .size(10.0)
                                    .color(palette::PRESET_LABEL),
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

                            // Preset switch button (only show if multiple presets available)
                            if self.preset_order.len() > 1 {
                                let next_idx =
                                    (self.current_preset_idx + 1) % self.preset_order.len();
                                let next_preset_idx = self.preset_order[next_idx];
                                let tooltip = self
                                    .all_presets
                                    .get(next_preset_idx)
                                    .map(|p| format!("Switch to: {}", p.name))
                                    .unwrap_or_else(|| "Switch preset".to_string());

                                if title_bar_button(ui, icons::ARROWS_CLOCKWISE, &tooltip).clicked()
                                {
                                    switch_preset = true;
                                }
                            }
                        });
                    }
                });

                // Handle preset switch outside of UI closure
                if switch_preset {
                    self.switch_to_next_preset();
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

                        // Determine state based on process/script
                        let is_running = self.running_processes.contains_key(&index)
                            || self.running_scripts.contains(&index);
                        let process_result = self.process_results.get(&index);

                        let icon_color = if is_running {
                            palette::RUNNING_ICON
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

                        // Draw underline for running or finished
                        let underline_color = if is_running {
                            Some(palette::RUNNING_ICON)
                        } else {
                            process_result.map(|r| match r {
                                ProcessResult::Success => palette::SUCCESS_UNDERLINE,
                                ProcessResult::Failed => palette::ERROR_UNDERLINE,
                            })
                        };
                        if let Some(color) = underline_color {
                            let rect = response.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left() + 5.0, rect.bottom() - 2.0),
                                    egui::pos2(rect.right() - 5.0, rect.bottom() - 2.0),
                                ],
                                egui::Stroke::new(2.0, color),
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
                        let detail =
                            cmd.cmd
                                .as_deref()
                                .or(cmd.run.as_deref().map(|s| {
                                    if s.len() > 30 {
                                        "[script]"
                                    } else {
                                        s
                                    }
                                }))
                                .unwrap_or("[no command]");
                        ui.label(
                            egui::RichText::new(format!("{}: {}", cmd.name, detail))
                                .color(palette::STATUS_TEXT)
                                .size(theme.font_size_xs),
                        );
                    }
                } else if let Some(status) = &self.last_status {
                    let color = if self.is_error {
                        palette::ERROR_TEXT
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
