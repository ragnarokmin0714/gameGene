//! The GameGene desktop app: attach to a process, scan, narrow, and manage a
//! cheat table of found values.

use eframe::egui::{self, Key, RichText};
use gamegene_core::constants::{APP_NAME, FREEZE_INTERVAL_MS};
use gamegene_core::fill::{plan_fixed, plan_increment};
use gamegene_core::find::{find_pattern, parse_aob, text_pattern, TextEncoding};
use gamegene_core::group::{group_rescan, group_scan, GroupHit};
use gamegene_core::hexview::{ascii_char, interpret};
use gamegene_core::pointer::{pointer_scan, PointerScanOptions};
use gamegene_core::scan::{Compare, ScanSession};
use gamegene_core::structure::{dissect, infer_fields, Field, StrideOptions};
use gamegene_core::table::{CheatTable, Locator, TableEntry};
use gamegene_core::value::{ScanValue, ValueType};
use gamegene_core::MemorySource;
use gamegene_platform::{attach, foreground_process, list_processes, ProcessInfo, BACKEND_NAME};
use std::time::{Duration, Instant};

use crate::fonts;
use crate::i18n::{self, Lang};
use crate::settings::{Action, KeyBindings};
use crate::theme;
use serde::{Deserialize, Serialize};

mod array;
mod chrome;
mod memview;
mod process;
mod scan;
mod table;

/// User-facing scan predicate choices.
#[derive(Clone, Copy, PartialEq)]
enum ScanMode {
    Exact,
    GreaterThan,
    LessThan,
    Between,
    Unknown,
    Changed,
    Unchanged,
    Increased,
    Decreased,
}

impl ScanMode {
    const FIRST: [ScanMode; 5] = [
        ScanMode::Exact,
        ScanMode::GreaterThan,
        ScanMode::LessThan,
        ScanMode::Between,
        ScanMode::Unknown,
    ];
    const NEXT: [ScanMode; 8] = [
        ScanMode::Exact,
        ScanMode::GreaterThan,
        ScanMode::LessThan,
        ScanMode::Between,
        ScanMode::Changed,
        ScanMode::Unchanged,
        ScanMode::Increased,
        ScanMode::Decreased,
    ];

    fn label(self, tr: &i18n::Tr) -> &'static str {
        match self {
            ScanMode::Exact => tr.m_exact,
            ScanMode::GreaterThan => tr.m_greater,
            ScanMode::LessThan => tr.m_less,
            ScanMode::Between => tr.m_between,
            ScanMode::Unknown => tr.m_unknown,
            ScanMode::Changed => tr.m_changed,
            ScanMode::Unchanged => tr.m_unchanged,
            ScanMode::Increased => tr.m_increased,
            ScanMode::Decreased => tr.m_decreased,
        }
    }

    fn needs_value(self) -> bool {
        matches!(
            self,
            ScanMode::Exact | ScanMode::GreaterThan | ScanMode::LessThan | ScanMode::Between
        )
    }

    fn needs_two(self) -> bool {
        self == ScanMode::Between
    }
}

/// Theme selection: an Apple skin (follow OS / forced light / forced dark) or a
/// warm Claude skin (light / dark).
#[derive(Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
    Claude,
    ClaudeDark,
}

impl ThemeChoice {
    /// Resolve to the concrete (skin, dark) to paint. `sys_dark` is the OS
    /// preference, used only by [`ThemeChoice::System`].
    fn resolve(self, sys_dark: bool) -> (theme::Skin, bool) {
        match self {
            ThemeChoice::System => (theme::Skin::Apple, sys_dark),
            ThemeChoice::Light => (theme::Skin::Apple, false),
            ThemeChoice::Dark => (theme::Skin::Apple, true),
            ThemeChoice::Claude => (theme::Skin::Claude, false),
            ThemeChoice::ClaudeDark => (theme::Skin::Claude, true),
        }
    }
}

/// How the "Find" box interprets its query.
#[derive(Clone, Copy, PartialEq)]
enum FindMode {
    Text,
    Utf16,
    Aob,
}

/// Which scan mode the scan panel is showing — a single value, or a group of
/// several values that must occur close together.
#[derive(Clone, Copy, PartialEq, Default)]
enum ScanTab {
    #[default]
    Value,
    Group,
}

/// The slice of state saved between runs (via eframe's storage).
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Persisted {
    theme: ThemeChoice,
    lang: Lang,
    keys: KeyBindings,
}

pub struct GameGeneApp {
    // Attachment
    processes: Vec<ProcessInfo>,
    filter: String,
    source: Option<Box<dyn MemorySource>>,
    attached_name: String,
    selected_pid: Option<u32>,
    /// Last foreground process that wasn't ourselves — the "detect game" target.
    last_foreground: Option<ProcessInfo>,

    // Scan controls
    value_type: ValueType,
    mode: ScanMode,
    value_text: String,
    value2_text: String,
    session: Option<ScanSession>,

    // Cheat table
    table: CheatTable,
    entry_counter: u32,

    // Find (byte / text search)
    find_query: String,
    find_mode: FindMode,
    find_results: Vec<u64>,

    // Which scan tab is active (single value vs. group of values)
    scan_tab: ScanTab,

    // Group scan (multiple values close together)
    group_query: String,
    group_span: u64,
    group_results: Vec<GroupHit>,

    // Memory viewer
    show_hex: bool,
    hex_addr: u64,
    hex_addr_input: String,
    hex_sel: Option<u64>,
    hex_write_type: ValueType,
    hex_write_text: String,
    /// Show every interpreted type in the memory viewer, not just the common few.
    hex_more: bool,

    // Structure / array dissection
    show_struct: bool,
    struct_base: u64,
    struct_base_input: String,
    struct_stride: usize,
    struct_stride_input: String,
    struct_rows: usize,
    struct_fields: Vec<Field>,
    // Fill / bulk write (operates on the dissected array)
    fill_field: usize,
    fill_increment: bool,
    fill_value: String,
    fill_step: String,
    fill_count: usize,
    /// Previewed writes, shown before applying.
    fill_plan: Vec<(u64, Vec<u8>)>,
    /// Original bytes from the last applied fill, for undo.
    fill_backup: Vec<(u64, Vec<u8>)>,

    // Chrome
    theme: ThemeChoice,
    applied_theme: Option<(theme::Skin, bool)>,
    /// System CJK font bytes, loaded once; reused on every font rebuild.
    cjk_font: Option<Vec<u8>>,
    /// Whether the serif face is currently installed, to avoid rebuilding fonts
    /// every frame.
    applied_serif: Option<bool>,
    lang: Lang,

    // Settings / shortcuts
    keys: KeyBindings,
    show_settings: bool,
    /// Action whose shortcut is being re-bound (waiting for a key press).
    capturing: Option<Action>,

    status: String,
    last_freeze: Instant,
    started: Instant,
}

impl GameGeneApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Restore saved theme / language / shortcuts, if any.
        let saved: Persisted = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();

        // Install fonts up front: default sans + a CJK fallback so Traditional
        // Chinese renders. The serif face is swapped in later if the Claude
        // theme is chosen.
        let cjk_font = fonts::load_cjk();
        fonts::apply(&cc.egui_ctx, false, &cjk_font);
        GameGeneApp {
            processes: list_processes(),
            filter: String::new(),
            source: None,
            attached_name: String::new(),
            selected_pid: None,
            last_foreground: None,
            value_type: ValueType::I32,
            mode: ScanMode::Exact,
            value_text: String::new(),
            value2_text: String::new(),
            session: None,
            table: CheatTable::new(),
            entry_counter: 0,
            find_query: String::new(),
            find_mode: FindMode::Text,
            find_results: Vec::new(),
            scan_tab: ScanTab::default(),
            group_query: String::new(),
            group_span: 512,
            group_results: Vec::new(),
            show_hex: false,
            hex_addr: 0,
            hex_addr_input: String::new(),
            hex_sel: None,
            hex_write_type: ValueType::I32,
            hex_write_text: String::new(),
            hex_more: false,
            show_struct: false,
            struct_base: 0,
            struct_base_input: String::new(),
            struct_stride: 0,
            struct_stride_input: String::new(),
            struct_rows: 16,
            struct_fields: Vec::new(),
            fill_field: 0,
            fill_increment: false,
            fill_value: String::new(),
            fill_step: "1".to_owned(),
            fill_count: 0,
            fill_plan: Vec::new(),
            fill_backup: Vec::new(),
            theme: saved.theme,
            applied_theme: None,
            cjk_font,
            applied_serif: Some(false),
            lang: saved.lang,
            keys: saved.keys,
            show_settings: false,
            capturing: None,
            status: format!("Ready — {BACKEND_NAME}"),
            last_freeze: Instant::now(),
            started: Instant::now(),
        }
    }

    fn tr(&self) -> &'static i18n::Tr {
        self.lang.strings()
    }
}

impl eframe::App for GameGeneApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Resolve and apply the theme only when it actually changes.
        let sys_dark = ctx.style().visuals.dark_mode;
        let resolved = self.theme.resolve(sys_dark);
        if self.applied_theme != Some(resolved) {
            theme::apply(ctx, resolved.0, resolved.1);
            self.applied_theme = Some(resolved);
        }
        // The Claude skin uses a serif face; swap fonts only on change.
        let serif = resolved.0 == theme::Skin::Claude;
        if self.applied_serif != Some(serif) {
            fonts::apply(ctx, serif, &self.cjk_font);
            self.applied_serif = Some(serif);
        }

        // Track the foreground game so "Detect game" can lock onto it. Ignore
        // our own window (foreground whenever the user clicks here) and the
        // Windows shell/system UI (explorer, taskbar, alt-tab, etc.), which
        // otherwise clobber the real game as the user switches windows.
        if let Some(fg) = foreground_process() {
            if fg.pid != std::process::id() && fg.pid != 0 && !process::is_shell_process(&fg.name) {
                self.last_foreground = Some(fg);
            }
        }

        // Enforce frozen entries on a fixed cadence.
        if let Some(src) = self.source.as_deref() {
            if self.table.entries.iter().any(|e| e.frozen)
                && self.last_freeze.elapsed() >= Duration::from_millis(FREEZE_INTERVAL_MS)
            {
                self.table.tick_frozen(src);
                self.last_freeze = Instant::now();
            }
        }
        // Repaint at least once a second so the running-time clock ticks and
        // foreground detection stays current even when idle.
        ctx.request_repaint_after(Duration::from_millis(FREEZE_INTERVAL_MS.min(1000)));

        self.handle_shortcuts(ctx);

        self.top_bar(ctx);
        self.process_panel(ctx);
        self.table_panel(ctx);
        self.scan_panel(ctx);
        self.hex_window(ctx);
        self.struct_window(ctx);
        self.settings_window(ctx);
    }

    /// Persist theme / language / shortcuts. eframe calls this on exit and
    /// periodically while running.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let persisted = Persisted {
            theme: self.theme,
            lang: self.lang,
            keys: self.keys.clone(),
        };
        eframe::set_value(storage, eframe::APP_KEY, &persisted);
    }
}

// UI sections, split out for readability.

/// Shorten a display string to at most `max` characters, appending an ellipsis
/// when truncated. Used for grid cells where a long value (e.g. a float with
/// many decimals) would otherwise widen the column.
fn short_value(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Format a duration as `HH:MM:SS` for the running-time display.
fn fmt_hms(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Read one typed value from a source, or `None` if unreadable.
fn read_value(src: &dyn MemorySource, addr: u64, ty: ValueType) -> Option<ScanValue> {
    let mut buf = [0u8; 8];
    let n = src.read(addr, &mut buf[..ty.size()]).ok()?;
    if n < ty.size() {
        return None;
    }
    Some(ScanValue::from_le_bytes(ty, &buf))
}
