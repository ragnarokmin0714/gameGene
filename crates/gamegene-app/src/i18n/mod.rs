//! UI language strings. Font installation (including the CJK fallback) lives in
//! the [`crate::fonts`] module.
//!
//! Each language is one file with a `static` [`Tr`] — the struct guarantees at
//! compile time that every language defines every string. To add a language:
//! a new file + a [`Lang`] variant, and the compiler walks you through the
//! rest.

use serde::{Deserialize, Serialize};

mod en;
mod ja;
mod zh_hant;

/// Selectable UI language.
#[derive(Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    En,
    ZhHant,
    Ja,
}

impl Lang {
    pub fn strings(self) -> &'static Tr {
        match self {
            Lang::En => &en::EN,
            Lang::ZhHant => &zh_hant::ZH,
            Lang::Ja => &ja::JA,
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
    pub theme_claude: &'static str,
    pub theme_claude_dark: &'static str,
    pub not_attached: &'static str,

    pub settings: &'static str,
    pub sc_title: &'static str,
    pub sc_change: &'static str,
    pub sc_press: &'static str,
    pub sc_reset: &'static str,
    pub sc_hint: &'static str,

    pub processes: &'static str,
    pub refresh: &'static str,
    pub filter_hint: &'static str,
    pub attach: &'static str,
    pub detach: &'static str,
    pub detect_game: &'static str,
    pub detect_hint: &'static str,
    pub attached_prefix: &'static str,
    pub attach_failed: &'static str,
    pub proc_hint: &'static str,
    pub uptime_prefix: &'static str,

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
    pub row_hint: &'static str,

    pub cheat_table: &'static str,
    pub save: &'static str,
    pub load: &'static str,
    pub table_subtitle: &'static str,
    pub freeze: &'static str,
    pub now_prefix: &'static str,
    pub set_hint: &'static str,
    pub apply: &'static str,
    pub pin: &'static str,
    pub pin_hint: &'static str,

    pub find_title: &'static str,
    pub find_search: &'static str,
    pub find_hint: &'static str,
    pub find_text: &'static str,
    pub find_utf16: &'static str,
    pub find_aob: &'static str,

    pub tab_value: &'static str,
    pub tab_group: &'static str,
    pub group_hint: &'static str,
    pub group_values_hint: &'static str,
    pub group_span: &'static str,
    pub group_range_note: &'static str,
    pub group_others_hint: &'static str,

    pub mem_view: &'static str,
    pub mem_title: &'static str,
    pub mem_goto: &'static str,
    pub mem_write: &'static str,
    pub mem_addr_hint: &'static str,
    pub mem_pick_hint: &'static str,
    pub mem_raw: &'static str,
    pub mem_more: &'static str,
    pub mem_less: &'static str,
    pub entry_goto_hint: &'static str,

    pub arr_view: &'static str,
    pub arr_title: &'static str,
    pub arr_detect: &'static str,
    pub arr_stride: &'static str,
    pub arr_apply: &'static str,
    pub arr_rows: &'static str,
    pub arr_hint: &'static str,
    pub arr_none: &'static str,
    pub arr_addr: &'static str,
    pub arr_dissect: &'static str,
    pub arr_cell_hint: &'static str,

    pub fill_title: &'static str,
    pub fill_field: &'static str,
    pub fill_increment: &'static str,
    pub fill_value: &'static str,
    pub fill_start: &'static str,
    pub fill_step: &'static str,
    pub fill_count: &'static str,
    pub fill_count_hint: &'static str,
    pub fill_preview_btn: &'static str,
    pub fill_apply_btn: &'static str,
    pub fill_undo_btn: &'static str,
    pub fill_writes: &'static str,

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
