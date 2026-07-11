//! Font setup: a bundled serif for the Claude theme, plus a system CJK fallback
//! so Traditional Chinese renders. Both families are (re)installed together
//! whenever the serif preference changes, since egui replaces the whole font
//! set on each `set_fonts`.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Liberation Serif (SIL OFL 1.1, see `assets/serif.LICENSE.txt`), used as the
/// proportional face for the Claude theme. Embedded so no asset lookup is
/// needed at runtime.
const SERIF: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/serif.ttf"));

/// Read a system CJK font once, if one is present. Returned to the app so it can
/// be reused on every font rebuild without touching disk again.
pub fn load_cjk() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        // Windows — always present on Windows 10/11.
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\msjhl.ttc",
        r"C:\Windows\Fonts\msyh.ttc",
        // Linux / SteamOS — Noto CJK / WenQuanYi.
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        // macOS.
        "/System/Library/Fonts/PingFang.ttc",
    ];
    CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
}

/// Install fonts. When `serif` is true the bundled serif becomes the primary
/// proportional face (the Claude look); otherwise egui's default sans is kept.
/// `cjk` (from [`load_cjk`]) is appended as the last fallback for both families.
pub fn apply(ctx: &egui::Context, serif: bool, cjk: &Option<Vec<u8>>) {
    let mut fonts = FontDefinitions::default();

    fonts
        .font_data
        .insert("serif".to_owned(), FontData::from_static(SERIF));
    if let Some(bytes) = cjk {
        fonts
            .font_data
            .insert("cjk".to_owned(), FontData::from_owned(bytes.clone()));
    }

    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    if serif {
        // Primary face; egui's default sans stays behind it as a glyph fallback.
        prop.insert(0, "serif".to_owned());
    }
    if cjk.is_some() {
        prop.push("cjk".to_owned());
        // Latin glyphs still come from the default monospace; CJK falls back.
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
    }

    ctx.set_fonts(fonts);
}
