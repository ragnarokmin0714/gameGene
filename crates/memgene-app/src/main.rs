//! MemGene desktop entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod i18n;
mod theme;

use eframe::egui;
use memgene_core::constants::APP_NAME;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([780.0, 520.0])
            .with_title(APP_NAME),
        follow_system_theme: true,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::MemGeneApp::new(cc)))),
    )
}
