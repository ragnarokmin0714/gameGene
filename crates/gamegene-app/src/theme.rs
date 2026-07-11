//! Themes for egui. Two skins, each in light and dark, sharing the same
//! rounded, generously-spaced layout:
//!
//! - [`Skin::Apple`]: restrained soft neutrals with one blue accent.
//! - [`Skin::Claude`]: warm cream / terracotta, echoing the Claude palette.

use eframe::egui::{self, Color32, Rounding, Stroke, Visuals};

/// Which colour skin to paint with.
#[derive(Clone, Copy, PartialEq)]
pub enum Skin {
    Apple,
    Claude,
}

/// Resolved palette for one skin + mode.
struct Palette {
    accent: Color32,
    panel: Color32,
    window: Color32,
    extreme: Color32,
    faint: Color32,
    border: Color32,
    btn: Color32,
    btn_hover: Color32,
    /// Warm text override, or `None` to keep egui's default.
    text: Option<Color32>,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn palette(skin: Skin, dark: bool) -> Palette {
    match (skin, dark) {
        (Skin::Apple, true) => Palette {
            accent: rgb(10, 132, 255), // iOS systemBlue (dark)
            panel: rgb(30, 30, 32),
            window: rgb(38, 38, 41),
            extreme: rgb(22, 22, 24),
            faint: rgb(44, 44, 47),
            border: rgb(58, 58, 63),
            btn: rgb(52, 52, 56),
            btn_hover: rgb(64, 64, 69),
            text: None,
        },
        (Skin::Apple, false) => Palette {
            accent: rgb(0, 122, 255), // iOS systemBlue (light)
            panel: rgb(246, 246, 248),
            window: rgb(255, 255, 255),
            extreme: rgb(255, 255, 255),
            faint: rgb(236, 236, 240),
            border: rgb(220, 220, 226),
            btn: rgb(255, 255, 255),
            btn_hover: rgb(244, 244, 247),
            text: None,
        },
        (Skin::Claude, false) => Palette {
            accent: rgb(204, 120, 92), // Claude terracotta
            panel: rgb(250, 249, 245), // warm cream
            window: rgb(255, 254, 250),
            extreme: rgb(255, 255, 255),
            faint: rgb(240, 238, 230),
            border: rgb(228, 225, 214),
            btn: rgb(245, 243, 236),
            btn_hover: rgb(237, 234, 224),
            text: Some(rgb(61, 61, 58)),
        },
        (Skin::Claude, true) => Palette {
            accent: rgb(217, 119, 87), // Claude terracotta (dark)
            panel: rgb(38, 38, 36),
            window: rgb(48, 48, 46),
            extreme: rgb(32, 32, 30),
            faint: rgb(54, 54, 50),
            border: rgb(70, 70, 64),
            btn: rgb(60, 60, 56),
            btn_hover: rgb(74, 74, 68),
            text: Some(rgb(232, 230, 220)),
        },
    }
}

/// Apply the theme to the context for the given skin and mode.
pub fn apply(ctx: &egui::Context, skin: Skin, dark: bool) {
    let Palette {
        accent,
        panel,
        window,
        extreme,
        faint,
        border,
        btn,
        btn_hover,
        text,
    } = palette(skin, dark);

    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.panel_fill = panel;
    v.window_fill = window;
    v.extreme_bg_color = extreme;
    v.faint_bg_color = faint;
    v.override_text_color = text;
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
