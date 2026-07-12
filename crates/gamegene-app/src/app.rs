//! The GameGene desktop app: attach to a process, scan, narrow, and manage a
//! cheat table of found values.

use eframe::egui::{self, Key, RichText};
use gamegene_core::constants::{APP_NAME, FREEZE_INTERVAL_MS};
use gamegene_core::fill::{plan_fixed, plan_increment};
use gamegene_core::find::{find_pattern, parse_aob, text_pattern, TextEncoding};
use gamegene_core::group::group_scan;
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
    group_results: Vec<u64>,

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

    /// Translate the current UI mode + inputs into a [`Compare`].
    fn build_compare(&self) -> Result<Compare, String> {
        let parse = |t: &str| ScanValue::parse(self.value_type, t).map_err(|e| e.to_string());
        Ok(match self.mode {
            ScanMode::Exact => Compare::Exact(parse(&self.value_text)?),
            ScanMode::GreaterThan => Compare::GreaterThan(parse(&self.value_text)?),
            ScanMode::LessThan => Compare::LessThan(parse(&self.value_text)?),
            ScanMode::Between => {
                Compare::Between(parse(&self.value_text)?, parse(&self.value2_text)?)
            }
            ScanMode::Unknown => Compare::Unknown,
            ScanMode::Changed => Compare::Changed,
            ScanMode::Unchanged => Compare::Unchanged,
            ScanMode::Increased => Compare::Increased,
            ScanMode::Decreased => Compare::Decreased,
        })
    }

    /// Tidy up "between" inputs before scanning: fill a missing upper bound with
    /// lower+1, and swap the two if they are the wrong way round.
    fn normalize_between_inputs(&mut self) {
        if self.mode != ScanMode::Between {
            return;
        }
        let ty = self.value_type;
        let is_float = matches!(ty, ValueType::F32 | ValueType::F64);

        // Fill an empty upper bound with lower + 1 so "11…" becomes "11…12".
        if self.value2_text.trim().is_empty() {
            if let Ok(v) = ScanValue::parse(ty, self.value_text.trim()) {
                self.value2_text = if is_float {
                    format!("{}", v.as_f64() + 1.0)
                } else {
                    format!("{}", v.as_f64() as i64 + 1)
                };
            }
        }
        // Swap if reversed so "28…11" becomes "11…28".
        if let (Ok(a), Ok(b)) = (
            ScanValue::parse(ty, self.value_text.trim()),
            ScanValue::parse(ty, self.value2_text.trim()),
        ) {
            if a.as_f64() > b.as_f64() {
                std::mem::swap(&mut self.value_text, &mut self.value2_text);
            }
        }
    }

    fn do_first_scan(&mut self) {
        self.normalize_between_inputs();
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        let compare = match self.build_compare() {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        match ScanSession::first_scan(src, self.value_type, compare) {
            Ok(s) => {
                self.status = format!("First scan: {} matches", s.len());
                self.session = Some(s);
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn do_next_scan(&mut self) {
        self.normalize_between_inputs();
        let compare = match self.build_compare() {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let (Some(src), Some(session)) = (self.source.as_deref(), self.session.as_mut()) else {
            self.status = "Run a first scan before narrowing.".into();
            return;
        };
        match session.next_scan(src, compare) {
            Ok(()) => self.status = format!("Narrowed to {} matches", session.len()),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn attach_to(&mut self, pid: u32, name: String) {
        match attach(pid) {
            Ok(src) => {
                self.source = Some(src);
                self.attached_name = format!("{name} ({pid})");
                self.session = None;
                self.status = format!("Attached to {name} (pid {pid})");
            }
            Err(e) => {
                self.source = None;
                self.attached_name.clear();
                self.status = format!("Attach failed: {e} — try running as Administrator");
            }
        }
    }

    fn do_find(&mut self) {
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        let query = self.find_query.trim();
        if query.is_empty() {
            return;
        }
        let pattern = match self.find_mode {
            FindMode::Text => text_pattern(query, TextEncoding::Utf8),
            FindMode::Utf16 => text_pattern(query, TextEncoding::Utf16Le),
            FindMode::Aob => match parse_aob(query) {
                Ok(p) => p,
                Err(e) => {
                    self.status = format!("Bad pattern: {e}");
                    return;
                }
            },
        };
        self.find_results =
            find_pattern(src, &pattern, gamegene_core::constants::MAX_RESULTS_DISPLAY);
        self.status = format!("Found {} match(es)", self.find_results.len());
    }

    /// Group scan: parse the space/comma-separated values (as the selected type)
    /// and find where they all occur within `group_span` bytes of each other.
    fn do_group_scan(&mut self) {
        let ty = self.value_type;
        let values: Result<Vec<ScanValue>, _> = self
            .group_query
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|t| ScanValue::parse(ty, t))
            .collect();
        let values = match values {
            Ok(v) => v,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        if values.len() < 2 {
            self.status = "Enter at least two values, separated by spaces.".into();
            return;
        }
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        self.group_results = group_scan(
            src,
            &values,
            self.group_span,
            gamegene_core::constants::MAX_RESULTS_DISPLAY,
        );
        self.status = format!("Group scan: {} match(es)", self.group_results.len());
    }

    fn add_to_table(&mut self, address: u64, value_type: ValueType) {
        self.entry_counter += 1;
        let desired = self
            .source
            .as_deref()
            .and_then(|s| read_value(s, address, value_type));
        self.table.add(TableEntry {
            id: 0,
            label: format!("Value {}", self.entry_counter),
            value_type,
            locator: Locator::Absolute(address),
            desired,
            frozen: false,
            notes: String::new(),
        });
        self.status = format!("Added {address:#x} to the table");
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
            if fg.pid != std::process::id() && fg.pid != 0 && !is_shell_process(&fg.name) {
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

impl GameGeneApp {
    /// Capture a key when re-binding, otherwise fire any matching shortcut.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.capturing {
            // Bind the first non-modifier key pressed; Esc cancels.
            let captured = ctx.input(|i| {
                Key::ALL
                    .iter()
                    .find(|k| i.key_pressed(**k))
                    .map(|k| (i.modifiers, *k))
            });
            if let Some((mods, key)) = captured {
                if key != Key::Escape {
                    self.keys
                        .set(action, crate::settings::Hotkey::from_parts(mods, key));
                }
                self.capturing = None;
            }
            return; // don't also trigger actions while binding
        }

        for action in Action::ALL {
            if let Some(sc) = self.keys.get(action).to_shortcut() {
                if ctx.input_mut(|i| i.consume_shortcut(&sc)) {
                    self.run_action(action);
                }
            }
        }
    }

    /// Perform a shortcut action (also usable from buttons).
    fn run_action(&mut self, action: Action) {
        match action {
            Action::DetectGame => {
                if let Some(fg) = self.last_foreground.clone() {
                    self.selected_pid = Some(fg.pid);
                    self.attach_to(fg.pid, fg.name);
                }
            }
            Action::Attach => {
                if let Some(pid) = self.selected_pid {
                    let name = self
                        .processes
                        .iter()
                        .find(|p| p.pid == pid)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.attach_to(pid, name);
                }
            }
            Action::Save => self.save_table(),
            Action::Load => self.load_table(),
            Action::ToggleMemory => self.show_hex = !self.show_hex,
            Action::FirstScan => {
                if self.session.is_none() {
                    self.do_first_scan();
                }
            }
            Action::NextScan => {
                if self.session.is_some() {
                    self.do_next_scan();
                }
            }
            Action::ResetScan => {
                self.session = None;
                self.mode = ScanMode::Exact;
                self.status = "Scan reset".into();
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let tr = self.tr();
        let mut open = self.show_settings;
        let mut start_capture = None;
        let mut reset = false;

        egui::Window::new(tr.settings)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.strong(tr.sc_title);
                ui.label(RichText::new(tr.sc_hint).weak());
                ui.add_space(4.0);
                egui::Grid::new("shortcuts")
                    .num_columns(3)
                    .striped(true)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        for action in Action::ALL {
                            ui.label(action_label(tr, action));
                            if self.capturing == Some(action) {
                                ui.colored_label(egui::Color32::from_rgb(0, 122, 255), tr.sc_press);
                            } else {
                                ui.monospace(self.keys.get(action).label());
                            }
                            if ui.small_button(tr.sc_change).clicked() {
                                start_capture = Some(action);
                            }
                            ui.end_row();
                        }
                    });
                ui.separator();
                if ui.button(tr.sc_reset).clicked() {
                    reset = true;
                }
            });

        if let Some(a) = start_capture {
            self.capturing = Some(a);
        }
        if reset {
            self.keys = KeyBindings::default();
            self.capturing = None;
        }
        self.show_settings = open;
    }
}

// UI sections, split out for readability.
impl GameGeneApp {
    fn top_bar(&mut self, ctx: &egui::Context) {
        let tr = self.tr();
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading(APP_NAME);
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .weak()
                        .small(),
                );
                ui.label(RichText::new(tr.tagline).weak());
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        tr.uptime_prefix,
                        fmt_hms(self.started.elapsed())
                    ))
                    .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.show_settings, tr.settings);
                    ui.toggle_value(&mut self.show_struct, tr.arr_view);
                    ui.toggle_value(&mut self.show_hex, tr.mem_view);
                    egui::ComboBox::from_id_source("lang")
                        .selected_text(match self.lang {
                            Lang::En => "English",
                            Lang::ZhHant => "繁體中文",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.lang, Lang::En, "English");
                            ui.selectable_value(&mut self.lang, Lang::ZhHant, "繁體中文");
                        });
                    egui::ComboBox::from_id_source("theme")
                        .selected_text(match self.theme {
                            ThemeChoice::System => tr.theme_system,
                            ThemeChoice::Light => tr.theme_light,
                            ThemeChoice::Dark => tr.theme_dark,
                            ThemeChoice::Claude => tr.theme_claude,
                            ThemeChoice::ClaudeDark => tr.theme_claude_dark,
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.theme,
                                ThemeChoice::System,
                                tr.theme_system,
                            );
                            ui.selectable_value(
                                &mut self.theme,
                                ThemeChoice::Light,
                                tr.theme_light,
                            );
                            ui.selectable_value(&mut self.theme, ThemeChoice::Dark, tr.theme_dark);
                            ui.selectable_value(
                                &mut self.theme,
                                ThemeChoice::Claude,
                                tr.theme_claude,
                            );
                            ui.selectable_value(
                                &mut self.theme,
                                ThemeChoice::ClaudeDark,
                                tr.theme_claude_dark,
                            );
                        });
                    if self.source.is_some() {
                        ui.colored_label(
                            egui::Color32::from_rgb(52, 199, 89),
                            format!("• {}", self.attached_name),
                        );
                    } else {
                        ui.label(RichText::new(tr.not_attached).weak());
                    }
                });
            });
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(&self.status);
            ui.add_space(2.0);
        });
    }

    fn process_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("processes")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                let tr = self.tr();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong(tr.processes);
                    if ui.small_button(tr.refresh).clicked() {
                        self.processes = list_processes();
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(tr.filter_hint));

                // Attach / detach controls for the selected process.
                ui.horizontal(|ui| {
                    let can_attach = self.selected_pid.is_some();
                    if ui
                        .add_enabled(can_attach, egui::Button::new(tr.attach))
                        .clicked()
                    {
                        if let Some(pid) = self.selected_pid {
                            let name = self
                                .processes
                                .iter()
                                .find(|p| p.pid == pid)
                                .map(|p| p.name.clone())
                                .unwrap_or_default();
                            self.attach_to(pid, name);
                        }
                    }
                    if self.source.is_some() && ui.small_button(tr.detach).clicked() {
                        self.source = None;
                        self.attached_name.clear();
                        self.session = None;
                        self.status = "Detached".into();
                    }
                });

                // Lock onto whatever game is currently in the foreground.
                if ui
                    .add_enabled(
                        self.last_foreground.is_some(),
                        egui::Button::new(tr.detect_game),
                    )
                    .on_hover_text(tr.detect_hint)
                    .clicked()
                {
                    if let Some(fg) = self.last_foreground.clone() {
                        self.selected_pid = Some(fg.pid);
                        self.attach_to(fg.pid, fg.name);
                    }
                }
                // Show what "Detect game" would grab, so the target is visible.
                if let Some(fg) = &self.last_foreground {
                    ui.label(RichText::new(format!("→ {} ({})", fg.name, fg.pid)).weak());
                }

                // Prominent attached / error state so the result is never missed.
                if self.source.is_some() {
                    ui.colored_label(
                        egui::Color32::from_rgb(52, 199, 89),
                        format!("{}{}", tr.attached_prefix, self.attached_name),
                    );
                } else if self.status.starts_with("Attach failed") {
                    ui.colored_label(egui::Color32::from_rgb(255, 69, 58), tr.attach_failed);
                }

                ui.separator();
                ui.label(RichText::new(tr.proc_hint).weak());

                let filter = self.filter.to_lowercase();
                let mut new_selected = None;
                let mut attach_now = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for p in self
                        .processes
                        .iter()
                        .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
                    {
                        let selected = self.selected_pid == Some(p.pid);
                        let resp =
                            ui.selectable_label(selected, format!("{}  ·  {}", p.name, p.pid));
                        if resp.clicked() {
                            new_selected = Some(p.pid);
                        }
                        if resp.double_clicked() {
                            attach_now = Some((p.pid, p.name.clone()));
                        }
                    }
                });
                if let Some(pid) = new_selected {
                    self.selected_pid = Some(pid);
                }
                if let Some((pid, name)) = attach_now {
                    self.selected_pid = Some(pid);
                    self.attach_to(pid, name);
                }
            });
    }

    fn scan_panel(&mut self, ctx: &egui::Context) {
        let tr = self.tr();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.scan_tab, ScanTab::Value, tr.tab_value);
                ui.selectable_value(&mut self.scan_tab, ScanTab::Group, tr.tab_group);
            });
            ui.separator();

            // Group scan lives on its own tab; single-value scan below is
            // unchanged.
            if self.scan_tab == ScanTab::Group {
                self.group_tab(ui);
                return;
            }

            ui.horizontal(|ui| {
                ui.label(tr.ty);
                egui::ComboBox::from_id_source("vt")
                    .selected_text(self.value_type.label())
                    .show_ui(ui, |ui| {
                        for t in ValueType::ALL {
                            ui.selectable_value(&mut self.value_type, t, t.label());
                        }
                    });

                ui.label(tr.scan);
                let modes: &[ScanMode] = if self.session.is_some() {
                    &ScanMode::NEXT
                } else {
                    &ScanMode::FIRST
                };
                if !modes.contains(&self.mode) {
                    self.mode = modes[0];
                }
                egui::ComboBox::from_id_source("mode")
                    .selected_text(self.mode.label(tr))
                    .show_ui(ui, |ui| {
                        for m in modes {
                            ui.selectable_value(&mut self.mode, *m, m.label(tr));
                        }
                    });
            });

            ui.horizontal(|ui| {
                if self.mode.needs_value() {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.value_text)
                            .desired_width(120.0)
                            .hint_text(tr.value_hint),
                    );
                }
                if self.mode.needs_two() {
                    ui.label("…");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.value2_text)
                            .desired_width(120.0)
                            .hint_text(tr.value_hint),
                    );
                }
            });

            ui.horizontal(|ui| {
                // Once a scan is in progress, first scan is disabled until Reset
                // so an accidental click can't wipe the narrowed results.
                ui.add_enabled_ui(self.session.is_none(), |ui| {
                    if ui.button(tr.first_scan).clicked() {
                        self.do_first_scan();
                    }
                });
                ui.add_enabled_ui(self.session.is_some(), |ui| {
                    if ui.button(tr.next_scan).clicked() {
                        self.do_next_scan();
                    }
                });
                if ui.button(tr.reset).clicked() {
                    self.session = None;
                    self.mode = ScanMode::Exact;
                    self.status = "Scan reset".into();
                }
                if let Some(s) = &self.session {
                    ui.label(RichText::new(format!("{} {}", s.len(), tr.matches)).weak());
                }
            });

            // Find bytes / text — a locate tool (collapsed by default).
            let mut do_find = false;
            let mut find_add = None;
            egui::CollapsingHeader::new(tr.find_title).show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("findmode")
                        .selected_text(match self.find_mode {
                            FindMode::Text => tr.find_text,
                            FindMode::Utf16 => tr.find_utf16,
                            FindMode::Aob => tr.find_aob,
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.find_mode, FindMode::Text, tr.find_text);
                            ui.selectable_value(
                                &mut self.find_mode,
                                FindMode::Utf16,
                                tr.find_utf16,
                            );
                            ui.selectable_value(&mut self.find_mode, FindMode::Aob, tr.find_aob);
                        });
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.find_query)
                            .desired_width(180.0)
                            .hint_text(tr.find_hint),
                    );
                    let entered =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button(tr.find_search).clicked() || entered {
                        do_find = true;
                    }
                });
                egui::ScrollArea::vertical()
                    .id_source("find_results")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        egui::Grid::new("find_grid")
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for &addr in &self.find_results {
                                    ui.monospace(format!("{addr:#014x}"));
                                    if ui.small_button(tr.add_table).clicked() {
                                        find_add = Some(addr);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
            if do_find {
                self.do_find();
            }
            if let Some(addr) = find_add {
                self.add_to_table(addr, self.value_type);
            }

            ui.separator();
            ui.strong(tr.results);

            let addrs: Vec<u64> = self
                .session
                .as_ref()
                .map(|s| s.display_matches().iter().map(|m| m.address).collect())
                .unwrap_or_default();
            if self.session.is_none() {
                ui.label(RichText::new(tr.no_scan).weak());
            }
            let (add_addr, goto_addr) = self.results_list(ui, "results", &addrs);

            if let Some(addr) = add_addr {
                self.add_to_table(addr, self.value_type);
            }
            if let Some(a) = goto_addr {
                self.open_hex_at(a);
            }
        });
    }

    /// The "group scan" tab: pick the type, enter several values, and search for
    /// where they all occur within a byte span of each other.
    fn group_tab(&mut self, ui: &mut egui::Ui) {
        let tr = self.tr();
        ui.horizontal(|ui| {
            ui.label(tr.ty);
            egui::ComboBox::from_id_source("group_vt")
                .selected_text(self.value_type.label())
                .show_ui(ui, |ui| {
                    for t in ValueType::ALL {
                        ui.selectable_value(&mut self.value_type, t, t.label());
                    }
                });
        });
        ui.label(RichText::new(tr.group_hint).weak());
        let mut do_group = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.group_query)
                    .desired_width(200.0)
                    .hint_text(tr.group_values_hint),
            );
            ui.label(tr.group_span);
            ui.add(egui::DragValue::new(&mut self.group_span).range(4..=65536));
            if ui.button(tr.find_search).clicked() {
                do_group = true;
            }
        });
        if do_group {
            self.do_group_scan();
        }

        ui.separator();
        ui.strong(tr.results);
        let addrs = self.group_results.clone();
        let (add_addr, goto_addr) = self.results_list(ui, "group_results", &addrs);
        if let Some(addr) = add_addr {
            self.add_to_table(addr, self.value_type);
        }
        if let Some(a) = goto_addr {
            self.open_hex_at(a);
        }
    }

    /// Render a scrollable list of result addresses (address · live value ·
    /// add), returning any address the user chose to add to the table or to
    /// open in the memory viewer. Shared by the single-value and group tabs.
    fn results_list(
        &self,
        ui: &mut egui::Ui,
        id: &str,
        addrs: &[u64],
    ) -> (Option<u64>, Option<u64>) {
        let tr = self.tr();
        let src = self.source.as_deref();
        let vt = self.value_type;
        let mut add_addr = None;
        let mut goto_addr = None;
        egui::ScrollArea::vertical()
            .id_source(id)
            .max_height(ui.available_height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new(format!("{id}_grid"))
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for &address in addrs {
                            // Double-click a row to add it; right-click for a
                            // menu (add / open in the memory viewer).
                            let resp = ui
                                .add(
                                    egui::Label::new(
                                        RichText::new(format!("{address:#014x}")).monospace(),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text(tr.row_hint);
                            if resp.double_clicked() {
                                add_addr = Some(address);
                            }
                            resp.context_menu(|ui| {
                                if ui.button(tr.add_table).clicked() {
                                    add_addr = Some(address);
                                    ui.close_menu();
                                }
                                if ui.button(tr.mem_view).clicked() {
                                    goto_addr = Some(address);
                                    ui.close_menu();
                                }
                            });
                            let now = src
                                .and_then(|s| read_value(s, address, vt))
                                .map(|v| v.display())
                                .unwrap_or_else(|| "—".into());
                            // Fixed width so a live value changing length does not
                            // reflow the grid and shake the list; long values
                            // (many decimals) truncate, full value on hover.
                            ui.add_sized(
                                [90.0, ui.spacing().interact_size.y],
                                egui::Label::new(&now).truncate(),
                            )
                            .on_hover_text(&now);
                            if ui.small_button(tr.add_table).clicked() {
                                add_addr = Some(address);
                            }
                            ui.end_row();
                        }
                    });
            });
        (add_addr, goto_addr)
    }

    /// Open the memory viewer focused on `a` (row-aligned, byte selected).
    fn open_hex_at(&mut self, a: u64) {
        self.show_hex = true;
        self.hex_addr = a & !0xF;
        self.hex_sel = Some(a);
        self.hex_addr_input = format!("{a:X}");
    }

    fn table_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("table")
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
                let tr = self.tr();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong(tr.cheat_table);
                    if ui.small_button(tr.save).clicked() {
                        self.save_table();
                    }
                    if ui.small_button(tr.load).clicked() {
                        self.load_table();
                    }
                });
                ui.label(RichText::new(tr.table_subtitle).weak());
                ui.separator();

                let src = self.source.as_deref();
                let mut remove_id = None;
                let mut apply_id = None;
                let mut pin_id = None;
                let mut goto_addr = None;

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &mut self.table.entries {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut entry.label)
                                        .desired_width(120.0),
                                );
                                ui.checkbox(&mut entry.frozen, tr.freeze);
                                if ui.small_button("×").clicked() {
                                    remove_id = Some(entry.id);
                                }
                            });
                            // Show the entry's current address so it can be told
                            // apart from others, and jump the memory viewer there.
                            ui.horizontal(|ui| {
                                let addr = match &entry.locator {
                                    Locator::Absolute(a) => Some(*a),
                                    _ => src.and_then(|s| entry.locator.resolve(s)),
                                };
                                if let Some(a) = addr {
                                    ui.monospace(
                                        RichText::new(format!("{a:#014X}"))
                                            .color(egui::Color32::from_rgb(0, 122, 255)),
                                    );
                                    if ui
                                        .small_button(tr.mem_view)
                                        .on_hover_text(tr.entry_goto_hint)
                                        .clicked()
                                    {
                                        goto_addr = Some(a);
                                    }
                                } else {
                                    ui.label(RichText::new("—").weak());
                                }
                            });
                            ui.horizontal(|ui| {
                                // A pointer/module locator already survives restarts.
                                let persistent =
                                    !matches!(entry.locator, gamegene_core::Locator::Absolute(_));
                                if persistent {
                                    ui.label(RichText::new(tr.pin).weak());
                                } else if ui
                                    .small_button(tr.pin)
                                    .on_hover_text(tr.pin_hint)
                                    .clicked()
                                {
                                    pin_id = Some(entry.id);
                                }
                            });
                            ui.horizontal(|ui| {
                                let current = src
                                    .and_then(|s| entry.read_current(s))
                                    .map(|v| v.display())
                                    .unwrap_or_else(|| "—".into());
                                ui.label(
                                    RichText::new(format!("{}{current}", tr.now_prefix)).weak(),
                                );
                                ui.label("->");
                                let mut txt =
                                    entry.desired.map(|v| v.display()).unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut txt)
                                            .desired_width(90.0)
                                            .hint_text(tr.set_hint),
                                    )
                                    .changed()
                                {
                                    entry.desired = ScanValue::parse(entry.value_type, &txt).ok();
                                }
                                if ui.small_button(tr.apply).clicked() {
                                    apply_id = Some(entry.id);
                                }
                            });
                        });
                    }
                });

                if let Some(a) = goto_addr {
                    self.show_hex = true;
                    self.hex_addr = a & !0xF;
                    self.hex_sel = Some(a);
                    self.hex_addr_input = format!("{a:X}");
                }
                if let Some(id) = remove_id {
                    self.table.remove(id);
                }
                if let Some(id) = apply_id {
                    if let (Some(src), Some(entry)) =
                        (self.source.as_deref(), self.table.get_mut(id))
                    {
                        match entry.apply_desired(src) {
                            Ok(()) => self.status = format!("Applied {}", entry.label),
                            Err(e) => self.status = format!("Apply failed: {e}"),
                        }
                    }
                }
                if let Some(id) = pin_id {
                    self.pin_entry(id);
                }
            });
    }

    /// Run a pointer scan for a table entry's current address and, if a stable
    /// pointer path is found, replace its locator so it survives restarts.
    fn pin_entry(&mut self, id: u64) {
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        let Some(entry) = self.table.get_mut(id) else {
            return;
        };
        let Some(addr) = entry.locator.resolve(src) else {
            self.status = "Could not resolve the entry's address.".into();
            return;
        };
        self.status = format!("Scanning for a pointer path to {addr:#x}…");
        match pointer_scan(src, addr, PointerScanOptions::default())
            .into_iter()
            .next()
        {
            Some(path) => {
                entry.locator = path;
                self.status = format!("Pinned {} — now survives restart", entry.label);
            }
            None => {
                self.status = "No pointer path found (try again or keep the raw address)".into()
            }
        }
    }

    fn hex_window(&mut self, ctx: &egui::Context) {
        if !self.show_hex {
            return;
        }
        let tr = self.tr();
        let mut open = true;
        let mut new_sel = None;
        let mut do_write = None;
        let mut add_addr = None;
        let mut dissect_from = None;

        egui::Window::new(tr.mem_title)
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .default_height(480.0)
            .show(ctx, |ui| {
                // Windowed read: only the visible 256 bytes, so this is cheap.
                let mut buf = [0u8; 256];
                let got = self
                    .source
                    .as_deref()
                    .map(|s| s.read(self.hex_addr, &mut buf).unwrap_or(0))
                    .unwrap_or(0);

                // Fixed address bar at the top.
                egui::TopBottomPanel::top("hex_top").show_inside(ui, |ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label("0x");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.hex_addr_input)
                                .desired_width(130.0)
                                .hint_text(tr.mem_addr_hint),
                        );
                        if ui.button(tr.mem_goto).clicked() {
                            let s = self.hex_addr_input.trim().trim_start_matches("0x");
                            if let Ok(a) = u64::from_str_radix(s, 16) {
                                self.hex_addr = a & !0xF; // align to a 16-byte row
                            }
                        }
                        if ui.small_button("- 256").clicked() {
                            self.hex_addr = self.hex_addr.saturating_sub(256);
                        }
                        if ui.small_button("+ 256").clicked() {
                            self.hex_addr = self.hex_addr.saturating_add(256);
                        }
                    });
                    ui.add_space(2.0);
                });

                // Everything below the address bar lives in one scroll area, so
                // the window can shrink freely without clipping the inspector or
                // the grid. The inspector stays right under the address bar for
                // easy reach.
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            if let Some(sel) = self.hex_sel {
                                ui.monospace(format!("@ {sel:#014X}"));
                                let off = sel.wrapping_sub(self.hex_addr) as usize;
                                if off < got {
                                    // Show the raw bytes plus the common types (Int32,
                                    // Float); "more" expands to every type. Long values
                                    // (f64) truncate with the full value on hover, so
                                    // they never blow out the panel width.
                                    let more = self.hex_more;
                                    let region = &buf[off..got];
                                    egui::Grid::new("hex_interp")
                                        .num_columns(2)
                                        .striped(true)
                                        .show(ui, |ui| {
                                            let h = ui.spacing().interact_size.y;
                                            // Fixed-width value cell: a live value that
                                            // changes length must not reflow the grid and
                                            // shake the whole window left-right.
                                            let value_row =
                                                |ui: &mut egui::Ui, label: &str, val: &str| {
                                                    ui.monospace(label);
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(220.0, h),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            ui.add(
                                                                egui::Label::new(
                                                                    RichText::new(val).monospace(),
                                                                )
                                                                .truncate(),
                                                            )
                                                            .on_hover_text(val);
                                                        },
                                                    );
                                                    ui.end_row();
                                                };

                                            let raw: String = region
                                                .iter()
                                                .take(8)
                                                .map(|b| format!("{b:02X} "))
                                                .collect();
                                            value_row(ui, tr.mem_raw, raw.trim_end());

                                            for (ty, v) in interpret(region) {
                                                let common =
                                                    matches!(ty, ValueType::I32 | ValueType::F32);
                                                if more || common {
                                                    value_row(ui, ty.label(), &v.display());
                                                }
                                            }
                                        });
                                    let label = if more { tr.mem_less } else { tr.mem_more };
                                    if ui.small_button(label).clicked() {
                                        self.hex_more = !more;
                                    }
                                }
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_source("hexwt")
                                        .selected_text(self.hex_write_type.label())
                                        .show_ui(ui, |ui| {
                                            for t in ValueType::ALL {
                                                ui.selectable_value(
                                                    &mut self.hex_write_type,
                                                    t,
                                                    t.label(),
                                                );
                                            }
                                        });
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.hex_write_text)
                                            .desired_width(110.0)
                                            .hint_text(tr.set_hint),
                                    );
                                    if ui.button(tr.mem_write).clicked() {
                                        do_write = Some(sel);
                                    }
                                    if ui.small_button(tr.add_table).clicked() {
                                        add_addr = Some(sel);
                                    }
                                    if ui.small_button(tr.arr_dissect).clicked() {
                                        dissect_from = Some(sel);
                                    }
                                });
                            } else {
                                ui.label(RichText::new(tr.mem_pick_hint).weak());
                            }
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(2.0);

                            // Hex/ASCII grid, in the same scroll area as the inspector.
                            egui::Grid::new("hexgrid")
                                .spacing([3.0, 2.0])
                                .show(ui, |ui| {
                                    for row in 0..16usize {
                                        let row_addr = self.hex_addr + (row * 16) as u64;
                                        ui.monospace(format!("{row_addr:012X}"));
                                        for col in 0..16usize {
                                            let i = row * 16 + col;
                                            let addr = self.hex_addr + i as u64;
                                            if i < got {
                                                let selected = self.hex_sel == Some(addr);
                                                if ui
                                                    .selectable_label(
                                                        selected,
                                                        format!("{:02X}", buf[i]),
                                                    )
                                                    .clicked()
                                                {
                                                    new_sel = Some(addr);
                                                }
                                            } else {
                                                ui.monospace("··");
                                            }
                                        }
                                        let ascii: String = (0..16)
                                            .map(|col| {
                                                let i = row * 16 + col;
                                                if i < got {
                                                    ascii_char(buf[i])
                                                } else {
                                                    ' '
                                                }
                                            })
                                            .collect();
                                        ui.monospace(ascii);
                                        ui.end_row();
                                    }
                                });
                        });
                });
            });

        if let Some(a) = new_sel {
            self.hex_sel = Some(a);
        }
        if let Some(addr) = do_write {
            self.hex_write_at(addr);
        }
        if let Some(addr) = add_addr {
            self.add_to_table(addr, self.value_type);
        }
        if let Some(addr) = dissect_from {
            self.show_struct = true;
            self.struct_base = addr;
            self.struct_base_input = format!("{addr:X}");
            self.struct_detect();
        }
        self.show_hex = open;
    }

    fn hex_write_at(&mut self, addr: u64) {
        let value = match ScanValue::parse(self.hex_write_type, &self.hex_write_text) {
            Ok(v) => v,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        match self.source.as_deref() {
            Some(src) => match src.write(addr, &value.to_le_bytes()) {
                Ok(()) => self.status = format!("Wrote {} to {addr:#x}", value.display()),
                Err(e) => self.status = format!("Write failed: {e}"),
            },
            None => self.status = "Attach to a process first.".into(),
        }
    }

    /// Parse the array base-address input (hex) into `struct_base`.
    fn struct_parse_base(&mut self) -> bool {
        let s = self.struct_base_input.trim().trim_start_matches("0x");
        match u64::from_str_radix(s, 16) {
            Ok(a) => {
                self.struct_base = a;
                true
            }
            Err(_) => {
                self.status = "Enter a hex address for the array base.".into();
                false
            }
        }
    }

    /// Auto-detect the record size at the base address and infer its fields.
    fn struct_detect(&mut self) {
        if !self.struct_parse_base() {
            return;
        }
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        match dissect(src, self.struct_base, StrideOptions::default()) {
            Some(d) => {
                self.struct_stride = d.stride;
                self.struct_stride_input = d.stride.to_string();
                self.struct_fields = d.fields;
                self.status = format!(
                    "Detected a {}-byte record with {} fields",
                    self.struct_stride,
                    self.struct_fields.len()
                );
            }
            None => {
                self.status = "Couldn't detect a stride — enter one and press Apply.".into();
            }
        }
    }

    /// Re-infer fields for a manually-entered stride.
    fn struct_apply_stride(&mut self) {
        if !self.struct_parse_base() {
            return;
        }
        let Ok(stride) = self.struct_stride_input.trim().parse::<usize>() else {
            self.status = "Stride must be a whole number of bytes.".into();
            return;
        };
        if stride < 4 {
            self.status = "Stride must be at least 4 bytes.".into();
            return;
        }
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        let opts = StrideOptions::default();
        let mut buf = vec![0u8; opts.window];
        let got = src.read(self.struct_base, &mut buf).unwrap_or(0);
        buf.truncate(got);
        let records = (buf.len() / stride).max(1);
        self.struct_stride = stride;
        self.struct_fields = infer_fields(&buf, stride, records);
        self.status = format!(
            "Using a {stride}-byte record with {} fields",
            self.struct_fields.len()
        );
    }

    /// Build the list of writes for the current fill settings, for preview.
    fn fill_preview(&mut self) {
        if self.struct_fields.is_empty() || self.struct_stride < 4 {
            self.status = "Detect an array first.".into();
            return;
        }
        let idx = self.fill_field.min(self.struct_fields.len() - 1);
        let field = self.struct_fields[idx];
        let count = if self.fill_count == 0 {
            self.struct_rows
        } else {
            self.fill_count
        };

        let plan = if self.fill_increment {
            if matches!(field.ty, ValueType::F32 | ValueType::F64) {
                self.status = "Increment is for integer fields only.".into();
                return;
            }
            let Ok(start) = self.fill_value.trim().parse::<i64>() else {
                self.status = "Start must be a whole number.".into();
                return;
            };
            let Ok(step) = self.fill_step.trim().parse::<i64>() else {
                self.status = "Step must be a whole number.".into();
                return;
            };
            plan_increment(
                self.struct_base,
                self.struct_stride,
                field.offset,
                field.ty,
                start,
                step,
                count,
            )
        } else {
            let value = match ScanValue::parse(field.ty, self.fill_value.trim()) {
                Ok(v) => v,
                Err(e) => {
                    self.status = e.to_string();
                    return;
                }
            };
            plan_fixed(
                self.struct_base,
                self.struct_stride,
                field.offset,
                &value,
                count,
            )
        };

        self.status = format!("Preview: {} write(s). Review, then Apply.", plan.len());
        self.fill_plan = plan;
    }

    /// Apply the previewed writes, backing up the original bytes for undo.
    fn fill_apply(&mut self) {
        let plan = std::mem::take(&mut self.fill_plan);
        if plan.is_empty() {
            self.status = "Preview a fill first.".into();
            return;
        }
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            self.fill_plan = plan;
            return;
        };
        let mut backup = Vec::with_capacity(plan.len());
        let mut written = 0usize;
        for (addr, bytes) in &plan {
            let mut orig = vec![0u8; bytes.len()];
            if src.read(*addr, &mut orig).unwrap_or(0) == bytes.len() {
                backup.push((*addr, orig));
            }
            if src.write(*addr, bytes).is_ok() {
                written += 1;
            }
        }
        self.fill_backup = backup;
        self.status = format!("Wrote {written} value(s) — Undo available");
    }

    /// Restore the bytes backed up by the last [`fill_apply`].
    fn fill_undo(&mut self) {
        let backup = std::mem::take(&mut self.fill_backup);
        if backup.is_empty() {
            self.status = "Nothing to undo.".into();
            return;
        }
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            self.fill_backup = backup;
            return;
        };
        let mut reverted = 0usize;
        for (addr, bytes) in &backup {
            if src.write(*addr, bytes).is_ok() {
                reverted += 1;
            }
        }
        self.status = format!("Reverted {reverted} value(s)");
    }

    fn struct_window(&mut self, ctx: &egui::Context) {
        if !self.show_struct {
            return;
        }
        let tr = self.tr();
        let mut open = self.show_struct;
        let mut detect = false;
        let mut apply = false;
        let mut add: Option<(u64, ValueType)> = None;
        let mut want_preview = false;
        let mut want_apply_fill = false;
        let mut want_undo_fill = false;

        egui::Window::new(tr.arr_title)
            .open(&mut open)
            .resizable(true)
            .default_width(660.0)
            .default_height(460.0)
            .show(ctx, |ui| {
                egui::TopBottomPanel::top("arr_top").show_inside(ui, |ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label("0x");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.struct_base_input)
                                .desired_width(120.0)
                                .hint_text(tr.mem_addr_hint),
                        );
                        if ui.button(tr.arr_detect).clicked() {
                            detect = true;
                        }
                        ui.separator();
                        ui.label(tr.arr_stride);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.struct_stride_input)
                                .desired_width(56.0),
                        );
                        if ui.button(tr.arr_apply).clicked() {
                            apply = true;
                        }
                        ui.label(tr.arr_rows);
                        ui.add(egui::DragValue::new(&mut self.struct_rows).range(1..=256));
                    });
                    ui.label(RichText::new(tr.arr_hint).weak());
                    ui.add_space(2.0);
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    if self.struct_fields.is_empty() || self.struct_stride < 4 {
                        ui.label(RichText::new(tr.arr_none).weak());
                        return;
                    }
                    let src = self.source.as_deref();
                    let stride = self.struct_stride as u64;
                    // Fill controls and the array grid share one scroll area, so
                    // the window resizes cleanly and nothing spills outside it.
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // Fill / bulk write, collapsed by default.
                            egui::CollapsingHeader::new(tr.fill_title).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(tr.fill_field);
                                    let cur = self.fill_field.min(self.struct_fields.len() - 1);
                                    let cur_f = self.struct_fields[cur];
                                    egui::ComboBox::from_id_source("fill_field")
                                        .selected_text(format!(
                                            "+{:X} {}",
                                            cur_f.offset,
                                            cur_f.ty.label()
                                        ))
                                        .show_ui(ui, |ui| {
                                            for i in 0..self.struct_fields.len() {
                                                let f = self.struct_fields[i];
                                                let lbl =
                                                    format!("+{:X} {}", f.offset, f.ty.label());
                                                ui.selectable_value(&mut self.fill_field, i, lbl);
                                            }
                                        });
                                    ui.checkbox(&mut self.fill_increment, tr.fill_increment);
                                });
                                ui.horizontal(|ui| {
                                    ui.label(if self.fill_increment {
                                        tr.fill_start
                                    } else {
                                        tr.fill_value
                                    });
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.fill_value)
                                            .desired_width(90.0),
                                    );
                                    if self.fill_increment {
                                        ui.label(tr.fill_step);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.fill_step)
                                                .desired_width(50.0),
                                        );
                                    }
                                    ui.label(tr.fill_count);
                                    ui.add(
                                        egui::DragValue::new(&mut self.fill_count)
                                            .range(0..=gamegene_core::fill::MAX_FILL),
                                    );
                                });
                                ui.label(RichText::new(tr.fill_count_hint).weak());
                                ui.horizontal(|ui| {
                                    if ui.button(tr.fill_preview_btn).clicked() {
                                        want_preview = true;
                                    }
                                    if ui
                                        .add_enabled(
                                            !self.fill_plan.is_empty(),
                                            egui::Button::new(tr.fill_apply_btn),
                                        )
                                        .clicked()
                                    {
                                        want_apply_fill = true;
                                    }
                                    if ui
                                        .add_enabled(
                                            !self.fill_backup.is_empty(),
                                            egui::Button::new(tr.fill_undo_btn),
                                        )
                                        .clicked()
                                    {
                                        want_undo_fill = true;
                                    }
                                });
                                if !self.fill_plan.is_empty() {
                                    ui.label(
                                        RichText::new(format!(
                                            "{} {}",
                                            self.fill_plan.len(),
                                            tr.fill_writes
                                        ))
                                        .weak(),
                                    );
                                    egui::ScrollArea::vertical()
                                        .id_source("fill_preview")
                                        .max_height(90.0)
                                        .show(ui, |ui| {
                                            for (addr, bytes) in self.fill_plan.iter().take(64) {
                                                let hex: String = bytes
                                                    .iter()
                                                    .map(|b| format!("{b:02X} "))
                                                    .collect();
                                                ui.monospace(format!(
                                                    "{addr:012X}  {}",
                                                    hex.trim_end()
                                                ));
                                            }
                                        });
                                }
                            });
                            egui::Grid::new("arr_grid").striped(true).show(ui, |ui| {
                                ui.strong(tr.arr_addr);
                                for f in &self.struct_fields {
                                    ui.strong(format!("+{:X} {}", f.offset, f.ty.label()));
                                }
                                ui.end_row();

                                let h = ui.spacing().interact_size.y;
                                for r in 0..self.struct_rows {
                                    let row_addr = self.struct_base + r as u64 * stride;
                                    ui.monospace(format!("{row_addr:012X}"));
                                    for f in &self.struct_fields {
                                        let addr = row_addr + f.offset as u64;
                                        let full = src
                                            .and_then(|s| read_value(s, addr, f.ty))
                                            .map(|v| v.display())
                                            .unwrap_or_else(|| "—".into());
                                        // Truncate long values (e.g. floats with
                                        // many decimals) so a cell can't widen the
                                        // grid; the full value shows on hover.
                                        let shown = short_value(&full, 10);
                                        let resp = ui
                                            .add_sized([92.0, h], egui::Button::new(shown).small());
                                        if resp.clicked() {
                                            add = Some((addr, f.ty));
                                        }
                                        resp.on_hover_text(format!(
                                            "{addr:#014X}\n{full} — {}",
                                            tr.arr_cell_hint
                                        ));
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                });
            });

        if detect {
            self.struct_detect();
        }
        if apply {
            self.struct_apply_stride();
        }
        if let Some((addr, ty)) = add {
            self.add_to_table(addr, ty);
        }
        if want_preview {
            self.fill_preview();
        }
        if want_apply_fill {
            self.fill_apply();
        }
        if want_undo_fill {
            self.fill_undo();
        }
        self.show_struct = open;
    }

    fn save_table(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "GameGene table",
                &[gamegene_core::constants::TABLE_FILE_EXT],
            )
            // Default to the app name; the .ggtable extension already reads as
            // "table", so no redundant "table" suffix is added.
            .set_file_name(format!(
                "{APP_NAME}.{}",
                gamegene_core::constants::TABLE_FILE_EXT
            ))
            .save_file()
        {
            // Stamp the current app version so the file records who wrote it,
            // even if this table was loaded from an older build.
            self.table.app_version = env!("CARGO_PKG_VERSION").to_owned();
            match self.table.save(&path) {
                Ok(()) => self.status = format!("Saved {}", path.display()),
                Err(e) => self.status = format!("Save failed: {e}"),
            }
        }
    }

    fn load_table(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "GameGene table",
                &[gamegene_core::constants::TABLE_FILE_EXT],
            )
            .pick_file()
        {
            match CheatTable::load(&path) {
                Ok(t) => {
                    self.table = t;
                    self.status = format!("Loaded {}", path.display());
                }
                Err(e) => self.status = format!("Load failed: {e}"),
            }
        }
    }
}

/// Label for an action in the shortcuts list, reusing the button strings.
fn action_label(tr: &i18n::Tr, action: Action) -> &'static str {
    match action {
        Action::DetectGame => tr.detect_game,
        Action::Attach => tr.attach,
        Action::Save => tr.save,
        Action::Load => tr.load,
        Action::ToggleMemory => tr.mem_view,
        Action::FirstScan => tr.first_scan,
        Action::NextScan => tr.next_scan,
        Action::ResetScan => tr.reset,
    }
}

/// Whether a process is the OS shell / system UI rather than a real app.
/// These briefly take the foreground as the user switches windows (clicking the
/// taskbar, alt-tab, the desktop), so they must not overwrite the detected game.
fn is_shell_process(name: &str) -> bool {
    const IGNORE: &[&str] = &[
        "explorer.exe",
        "dwm.exe",
        "applicationframehost.exe",
        "searchhost.exe",
        "searchapp.exe",
        "startmenuexperiencehost.exe",
        "shellexperiencehost.exe",
        "textinputhost.exe",
        "systemsettings.exe",
        "lockapp.exe",
    ];
    let lower = name.to_ascii_lowercase();
    IGNORE.contains(&lower.as_str())
}

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
