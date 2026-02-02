//! Color utilities and constants

use std::hash::{Hash, Hasher};

use eframe::egui;

/// Color constants for UI elements
pub mod palette {
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

/// Parse a hex color string (e.g., "#FF7043" or "FF7043")
pub fn parse_hex_color(hex: &str) -> Option<egui::Color32> {
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
pub fn vary_color_by_path(base_color: egui::Color32, path: &str) -> egui::Color32 {
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
