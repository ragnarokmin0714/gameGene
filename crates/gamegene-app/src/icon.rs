//! The window / taskbar icon.
//!
//! Embedded as raw RGBA (256x256) so no image decoder or asset lookup is needed
//! at runtime. Regenerate with `python3 assets/make_icon.py`.

use eframe::egui::IconData;

const ICON_RGBA: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon.rgba"));
const ICON_SIZE: u32 = 256;

pub fn icon_data() -> IconData {
    IconData {
        rgba: ICON_RGBA.to_vec(),
        width: ICON_SIZE,
        height: ICON_SIZE,
    }
}
