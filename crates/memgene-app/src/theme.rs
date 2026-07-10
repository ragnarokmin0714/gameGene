//! A restrained, Apple-flavored theme for egui: soft neutrals, one blue accent,
//! rounded corners, generous spacing. Works in both light and dark.

use eframe::egui::{self, Color32, Rounding, Stroke, Visuals};

/// Apply the theme to the context for the given mode.
pub fn apply(ctx: &egui::Context, dark: bool) {
    let accent = if dark {
        Color32::from_rgb(10, 132, 255) // iOS systemBlue (dark)
    } else {
        Color32::from_rgb(0, 122, 255) // iOS systemBlue (light)
    };

    let mut v = if dark { Visuals::dark() } else { Visuals::light() };

    let (panel, window, extreme, faint, border, btn, btn_hover) = if dark {
        (
            Color32::from_rgb(30, 30, 32),
            Color32::from_rgb(38, 38, 41),
            Color32::from_rgb(22, 22, 24),
            Color32::from_rgb(44, 44, 47),
            Color32::from_rgb(58, 58, 63),
            Color32::from_rgb(52, 52, 56),
            Color32::from_rgb(64, 64, 69),
        )
    } else {
        (
            Color32::from_rgb(246, 246, 248),
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(236, 236, 240),
            Color32::from_rgb(220, 220, 226),
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(244, 244, 247),
        )
    };

    v.panel_fill = panel;
    v.window_fill = window;
    v.extreme_bg_color = extreme;
    v.faint_bg_color = faint;
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0_f32, border);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 90);
    v.selection.stroke = Stroke::new(1.0_f32, accent);
    v.hyperlink_color = accent;

    let rounding = Rounding::same(8.0);
    v.widgets.noninteractive.rounding = rounding;
    v.widgets.inactive.rounding = rounding;
    v.widgets.hovered.rounding = rounding;
    v.widgets.active.rounding = rounding;
    v.widgets.open.rounding = rounding;

    v.widgets.inactive.weak_bg_fill = btn;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border);
    v.widgets.hovered.weak_bg_fill = btn_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
    v.widgets.active.weak_bg_fill = accent;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);

    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        use egui::{FontFamily, FontId, TextStyle};
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.window_margin = egui::Margin::same(14.0);
        s.spacing.interact_size.y = 28.0;
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(21.0, FontFamily::Proportional),
        );
        s.text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
    });
}
