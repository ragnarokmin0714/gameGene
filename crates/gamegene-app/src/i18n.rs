//! UI language strings and the CJK font loader.
//!
//! egui's bundled fonts contain no CJK glyphs, so [`install_cjk_font`] must find
//! and register a system Chinese font or Traditional Chinese renders as blank
//! boxes. English works regardless.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Selectable UI language.
#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    En,
    ZhHant,
}

impl Lang {
    pub fn strings(self) -> &'static Tr {
        match self {
            Lang::En => &EN,
            Lang::ZhHant => &ZH,
        }
    }
}

/// All user-facing UI strings for one language. Technical value-type labels
/// (Int32, Float, …) are intentionally left untranslated.
pub struct Tr {
    pub tagline: &'static str,
    pub theme_system: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub not_attached: &'static str,

    pub processes: &'static str,
    pub refresh: &'static str,
    pub filter_hint: &'static str,
    pub attach: &'static str,
    pub detach: &'static str,
    pub attached_prefix: &'static str,
    pub attach_failed: &'static str,
    pub proc_hint: &'static str,

    pub ty: &'static str,
    pub scan: &'static str,
    pub value_hint: &'static str,
    pub first_scan: &'static str,
    pub next_scan: &'static str,
    pub reset: &'static str,
    pub matches: &'static str,
    pub results: &'static str,
    pub no_scan: &'static str,
    pub add_table: &'static str,

    pub cheat_table: &'static str,
    pub save: &'static str,
    pub load: &'static str,
    pub table_subtitle: &'static str,
    pub freeze: &'static str,
    pub now_prefix: &'static str,
    pub set_hint: &'static str,
    pub apply: &'static str,

    pub m_exact: &'static str,
    pub m_greater: &'static str,
    pub m_less: &'static str,
    pub m_between: &'static str,
    pub m_unknown: &'static str,
    pub m_changed: &'static str,
    pub m_unchanged: &'static str,
    pub m_increased: &'static str,
    pub m_decreased: &'static str,
}

static EN: Tr = Tr {
    tagline: "Single-player memory editor",
    theme_system: "System",
    theme_light: "Light",
    theme_dark: "Dark",
    not_attached: "● not attached",

    processes: "Processes",
    refresh: "Refresh",
    filter_hint: "Filter…",
    attach: "Attach",
    detach: "Detach",
    attached_prefix: "✓ Attached: ",
    attach_failed: "⚠ Attach failed — run as Administrator (see status bar)",
    proc_hint: "Click to select · double-click or Attach to connect",

    ty: "Type",
    scan: "Scan",
    value_hint: "value",
    first_scan: "First scan",
    next_scan: "Next scan",
    reset: "Reset",
    matches: "matches",
    results: "Results",
    no_scan: "No scan yet.",
    add_table: "＋ Table",

    cheat_table: "Cheat table",
    save: "Save…",
    load: "Load…",
    table_subtitle: "Saved addresses persist — no need to rescan next time.",
    freeze: "Freeze",
    now_prefix: "now: ",
    set_hint: "set",
    apply: "Apply",

    m_exact: "Exact value",
    m_greater: "Greater than",
    m_less: "Less than",
    m_between: "Between",
    m_unknown: "Unknown initial value",
    m_changed: "Changed",
    m_unchanged: "Unchanged",
    m_increased: "Increased",
    m_decreased: "Decreased",
};

static ZH: Tr = Tr {
    tagline: "單機遊戲記憶體修改器",
    theme_system: "跟隨系統",
    theme_light: "淺色",
    theme_dark: "深色",
    not_attached: "● 未連接",

    processes: "處理程序",
    refresh: "重新整理",
    filter_hint: "篩選…",
    attach: "連接",
    detach: "中斷",
    attached_prefix: "✓ 已連接：",
    attach_failed: "⚠ 連接失敗 — 請以系統管理員身分執行（見下方狀態列）",
    proc_hint: "點擊選取 · 雙擊或按「連接」以附加",

    ty: "型別",
    scan: "掃描",
    value_hint: "數值",
    first_scan: "首次掃描",
    next_scan: "再次掃描",
    reset: "重設",
    matches: "筆符合",
    results: "結果",
    no_scan: "尚未掃描。",
    add_table: "＋ 加入表",

    cheat_table: "修改表",
    save: "儲存…",
    load: "載入…",
    table_subtitle: "已儲存的位址會保留 — 下次不用重新掃描。",
    freeze: "凍結",
    now_prefix: "目前：",
    set_hint: "設定值",
    apply: "套用",

    m_exact: "精確值",
    m_greater: "大於",
    m_less: "小於",
    m_between: "介於",
    m_unknown: "未知初始值",
    m_changed: "已改變",
    m_unchanged: "未改變",
    m_increased: "增加",
    m_decreased: "減少",
};

/// Find and register a system CJK font as a fallback so Chinese renders.
/// Returns the path that was loaded, or `None` if no candidate existed.
pub fn install_cjk_font(ctx: &egui::Context) -> Option<String> {
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

    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), FontData::from_owned(bytes));
        // Append as the last fallback for both families, so Latin glyphs still
        // come from egui's default font and only missing (CJK) ones fall here.
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        return Some((*path).to_owned());
    }
    None
}
