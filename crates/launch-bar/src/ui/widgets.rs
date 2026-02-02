//! Reusable UI widgets

use eframe::egui;

use super::colors::palette;

/// Create a title bar button with consistent styling
pub fn title_bar_button(ui: &mut egui::Ui, icon: &str, tooltip: &str) -> egui::Response {
    let icon_text = egui::RichText::new(icon)
        .family(egui::FontFamily::Name("icons".into()))
        .size(14.0)
        .color(palette::BUTTON_ICON);
    let button = egui::Button::new(icon_text)
        .fill(egui::Color32::TRANSPARENT)
        .min_size(egui::vec2(20.0, 20.0));
    ui.add(button).on_hover_text(tooltip)
}
