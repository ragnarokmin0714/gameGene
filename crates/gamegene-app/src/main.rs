//! GameGene desktop entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod fonts;
mod i18n;
mod icon;
mod theme;

use eframe::egui;
use gamegene_core::constants::APP_NAME;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([780.0, 520.0])
            .with_title(format!("{APP_NAME} v{}", env!("CARGO_PKG_VERSION")))
            .with_icon(icon::icon_data()),
        follow_system_theme: true,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::GameGeneApp::new(cc)))),
    )
}
