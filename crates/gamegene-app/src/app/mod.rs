//! The GameGene desktop app: attach to a process, scan, narrow, and manage a
//! cheat table of found values.

use eframe::egui::{self, Key, RichText};
use gamegene_core::constants::{APP_NAME, FREEZE_INTERVAL_MS, SETTLE_INTERVAL_MS, SETTLE_PASSES};
use gamegene_core::fill::{plan_fixed, plan_increment};
use gamegene_core::find::{
    find_pattern, parse_aob, preview, text_pattern, Pattern, PreviewStyle, TextEncoding,
};
use gamegene_core::group::{GroupHit, GroupQuery};
use gamegene_core::hexview::{ascii_char, focus_on, interpret, selected_offset};
use gamegene_core::pointer::{pointer_scan_with, revalidate, PointerScanOptions};
use gamegene_core::scan::{Compare, ResultFilter, ScanSession};
use gamegene_core::structure::{dissect, infer_fields, Field, StrideOptions};
use gamegene_core::table::{CheatTable, Locator, TableEntry};
use gamegene_core::value::{ScanValue, ValueType};
use gamegene_core::{read_prefix, MemorySource};
use gamegene_platform::{attach, foreground_process, list_processes, ProcessInfo, BACKEND_NAME};
use std::sync::Arc;
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
mod scan_job;
mod table;

use scan_job::{GroupDone, GroupJob, JobDone, JobKind, ScanJob};

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
    /// Narrowing modes, relative ones first. `Unknown` is gone (the snapshot is
    /// already taken), so the mode carried over from an unknown first scan falls
    /// back to `NEXT[0]` — which is why `Changed` leads: after "unknown initial
    /// value" the useful next step is always a relative comparison, never a
    /// value you would have typed into the first scan.
    const NEXT: [ScanMode; 8] = [
        ScanMode::Changed,
        ScanMode::Unchanged,
        ScanMode::Increased,
        ScanMode::Decreased,
        ScanMode::Exact,
        ScanMode::GreaterThan,
        ScanMode::LessThan,
        ScanMode::Between,
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

/// The value side of the results filter. Sign filters are the common case (a
/// negative or absurd number is almost always junk rather than the stat you are
/// hunting), so they get their own entries instead of making the user type a
/// range every time.
#[derive(Clone, Copy, PartialEq, Default)]
enum ValueFilter {
    #[default]
    Any,
    Positive,
    Negative,
    Between,
}

/// How the "Find" box interprets its query.
///
/// Text no longer asks which encoding: a game's strings are UTF-16 if it is
/// .NET/Unity and UTF-8 if it is native C++, and the user has no way to know
/// which. Making them pick turned one wrong guess into "this feature is
/// broken" — so Text searches both and each hit says which one matched.
#[derive(Clone, Copy, PartialEq)]
enum FindMode {
    Text,
    Aob,
}

/// One search hit: where it is, how to read it, and a preview of what is there.
struct FindHit {
    addr: u64,
    /// Which encoding matched, for the row's tag. Empty for a byte signature.
    encoding: &'static str,
    /// How to decode this hit's bytes — differs per hit once Text matches both
    /// encodings in the same search.
    style: PreviewStyle,
    preview: String,
}

/// Which scan mode the scan panel is showing — a single value, or a group of
/// several values that must occur close together.
#[derive(Clone, Copy, PartialEq, Default)]
enum ScanTab {
    #[default]
    Value,
    Group,
    /// Byte / text search. A peer of the other two rather than a fold-out under
    /// the value scan: it is a way of *locating* something, used on its own,
    /// and burying it made it look like an accessory to scanning.
    Find,
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
    source: Option<Arc<dyn MemorySource>>,
    attached_name: String,
    /// Raw attached process name (no pid), for defaulting the save-table
    /// file name to the game being edited.
    attached_game: String,
    selected_pid: Option<u32>,
    /// Last foreground process that wasn't ourselves — the "detect game" target.
    last_foreground: Option<ProcessInfo>,

    // Scan controls
    value_type: ValueType,
    mode: ScanMode,
    value_text: String,
    value2_text: String,
    session: Option<ScanSession>,
    /// A scan running on a background thread, if any. While set, the scan
    /// controls show a progress bar and a cancel button instead.
    scan_job: Option<ScanJob>,
    /// Narrowing passes the drop-fluctuating-values filter still owes, and when
    /// the next one is due. Zero means the filter isn't running.
    settle_left: u8,
    settle_next: Option<Instant>,

    /// Type the results list renders values as, when it should differ from the
    /// type being scanned. `None` follows the scan type.
    ///
    /// These come apart because scanning as Int32 deliberately covers positive
    /// floats — an f32's bit pattern orders the same way as the number, so
    /// Increased/Decreased track it correctly — but the *value* then reads as a
    /// huge integer (2135.0f shows as 1157984256). Rendering as Float turns the
    /// list back into numbers you recognize, without touching the session.
    display_as: Option<ValueType>,

    // Results filter — a view over the candidates, not a narrowing step.
    filter_addr_min: String,
    filter_addr_max: String,
    filter_value: ValueFilter,
    filter_lo: String,
    filter_hi: String,

    // Cheat table
    table: CheatTable,
    entry_counter: u32,
    /// Whether the "clear the whole table" confirmation is showing.
    confirm_clear: bool,

    // Find (byte / text search)
    find_query: String,
    find_mode: FindMode,
    /// Each hit's address plus a snapshot of the bytes there, decoded for
    /// reading. Captured once when the search runs rather than re-read every
    /// frame: a text search can return thousands of hits, and one read per row
    /// per frame would be thousands of syscalls a second for a list that only
    /// ever shows a screenful.
    find_results: Vec<FindHit>,
    /// The hit whose surroundings are pinned open below the list, and the text
    /// read there. Pinned because a hover tooltip vanishes the moment the
    /// pointer moves toward it — unreadable for anything longer than a glance,
    /// and impossible to copy from.
    find_pinned: Option<u64>,
    find_pinned_text: String,

    // Which scan tab is active (single value vs. group of values)
    scan_tab: ScanTab,

    // Group scan (multiple values close together)
    group_query: String,
    group_span: u64,
    group_results: Vec<GroupHit>,
    /// A running group scan, if any (background thread, like `scan_job`).
    group_job: Option<GroupJob>,
    /// Whether a first group scan has run — locks first scan until Reset, so an
    /// accidental click can't wipe narrowed group results (mirrors value scan).
    group_scanned: bool,

    // A cell click in the array view stages an add here; a confirmation window
    // (with an editable label) turns it into a table entry.
    pending_add: Option<(u64, ValueType)>,
    pending_add_label: String,

    // Pointer paths — candidates for one table entry, narrowed across restarts.
    //
    // Kept in app state rather than on the entry: they are working material,
    // only one of them ever becomes the entry's locator, and a list that has
    // not yet survived a restart is not worth saving to a cheat table.
    show_ptr: bool,
    /// Which table entry the candidates belong to.
    ptr_entry: Option<u64>,
    ptr_paths: Vec<Locator>,
    /// How many candidates the first scan produced, so the narrowing is visible.
    ptr_initial: usize,
    /// Target address for the next revalidation pass — where the value lives in
    /// the *current* run, which is not where it lived when the paths were found.
    ptr_target_input: String,

    // Memory viewer
    show_hex: bool,
    hex_addr: u64,
    hex_addr_input: String,
    hex_sel: Option<u64>,
    hex_write_type: ValueType,
    hex_write_text: String,
    /// Show every interpreted type in the memory viewer, not just the common few.
    hex_more: bool,
    /// Screen rect of the hex grid as drawn last frame, so a wheel event over it
    /// can be routed to address stepping instead of to the scroll area.
    hex_grid_rect: Option<egui::Rect>,
    /// Leftover wheel travel not yet worth a whole row, so a trackpad's fine
    /// scrolling accumulates instead of being rounded away every frame.
    hex_scroll_accum: f32,

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
            attached_game: String::new(),
            selected_pid: None,
            last_foreground: None,
            value_type: ValueType::I32,
            mode: ScanMode::Exact,
            value_text: String::new(),
            value2_text: String::new(),
            session: None,
            scan_job: None,
            settle_left: 0,
            settle_next: None,
            display_as: None,
            filter_addr_min: String::new(),
            filter_addr_max: String::new(),
            filter_value: ValueFilter::default(),
            filter_lo: String::new(),
            filter_hi: String::new(),
            table: CheatTable::new(),
            entry_counter: 0,
            confirm_clear: false,
            find_query: String::new(),
            find_mode: FindMode::Text,
            find_results: Vec::new(),
            find_pinned: None,
            find_pinned_text: String::new(),
            scan_tab: ScanTab::default(),
            group_query: String::new(),
            group_span: 512,
            group_results: Vec::new(),
            group_job: None,
            group_scanned: false,
            pending_add: None,
            pending_add_label: String::new(),
            show_ptr: false,
            ptr_entry: None,
            ptr_paths: Vec::new(),
            ptr_initial: 0,
            ptr_target_input: String::new(),
            show_hex: false,
            hex_addr: 0,
            hex_addr_input: String::new(),
            hex_sel: None,
            hex_write_type: ValueType::I32,
            hex_write_text: String::new(),
            hex_more: false,
            hex_grid_rect: None,
            hex_scroll_accum: 0.0,
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

        // Pick up a finished background scan (installs results or clears on
        // cancel). Kept before drawing so this frame shows the outcome.
        self.poll_scan_job();
        self.poll_group_job();
        self.tick_settle_filter();

        self.handle_shortcuts(ctx);

        self.top_bar(ctx);
        self.process_panel(ctx);
        self.table_panel(ctx);
        self.scan_panel(ctx);
        self.hex_window(ctx);
        self.struct_window(ctx);
        self.confirm_add_window(ctx);
        self.confirm_clear_window(ctx);
        self.pointer_window(ctx);
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

/// A single-line text input at the standard control height
/// ([`theme::CONTROL_HEIGHT`]). A plain `TextEdit` is shorter than a button,
/// so a control row mixing the two looks vertically ragged; control bars use
/// this so every control in the row shares one height and centreline.
fn control_edit(text: &mut String, width: f32) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .desired_width(width)
        .vertical_align(egui::Align::Center)
        .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
}

/// A label vertically centred to the control height. A bare `ui.label` in a row
/// of taller widgets (inputs, buttons) top-aligns — it shares the row's top,
/// not its centreline — so the text sits noticeably high. Sizing the label to
/// the control height and letting `add_sized` centre it fixes the alignment.
fn bar_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let w = ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
            .size()
            .x
    });
    ui.add_sized([w, theme::CONTROL_HEIGHT], egui::Label::new(text))
}

/// Format a duration as `HH:MM:SS` for the running-time display.
fn fmt_hms(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// A pointer path in the form the user recognizes: `module+base -> +o1 -> +o2`.
///
/// The offsets are the whole content of a path — two candidates differing only
/// in their last hop have to be told apart at a glance.
fn describe_locator(loc: &Locator) -> String {
    match loc {
        Locator::Pointer {
            module,
            base_offset,
            offsets,
        } => {
            let hops: Vec<String> = offsets.iter().map(|o| format!("+{o:#X}")).collect();
            format!("{module}+{base_offset:#X} -> {}", hops.join(" -> "))
        }
        Locator::Absolute(a) => format!("{a:#014X}"),
        other => format!("{other:?}"),
    }
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

/// Bytes read at a search hit to build its preview, and the character budget
/// that preview gets. Two bytes per character in UTF-16, so the byte figure is
/// the binding one there.
const PREVIEW_BYTES: usize = 192;
const PREVIEW_CHARS: usize = 96;

/// A pinned hit reads a much wider window than a row preview: the point of
/// pinning is to read a structure (a JSON object, a string table) rather than
/// identify it.
const DETAIL_BYTES: usize = 2048;
const DETAIL_CHARS: usize = 1024;

/// Half-width of the address window "narrow the scan to here" sets around a
/// hit. A game's fields sit in the same allocation as the strings that name
/// them, so a few KB either side is where the value almost always is.
const FIND_RANGE_HALF: u64 = 2048;

/// Read up to `want` bytes at `addr` for a preview.
fn read_context(src: &dyn MemorySource, addr: u64, want: usize) -> Vec<u8> {
    let mut buf = vec![0u8; want];
    let got = read_prefix(src, addr, &mut buf);
    buf.truncate(got);
    buf
}

/// Read as much of the slot at `addr` as is readable, up to the widest value
/// type, returning `(bytes read, buffer)`.
///
/// Narrows on failure rather than giving up: near the end of a region an 8-byte
/// read fails outright even though the 4-byte value the user is looking at is
/// perfectly readable.
fn read_slot(src: &dyn MemorySource, addr: u64) -> Option<(usize, [u8; 8])> {
    let mut buf = [0u8; 8];
    for want in [8usize, 4, 2, 1] {
        if matches!(src.read(addr, &mut buf[..want]), Ok(n) if n == want) {
            return Some((want, buf));
        }
    }
    None
}
