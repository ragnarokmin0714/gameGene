//! The MemGene desktop app: attach to a process, scan, narrow, and manage a
//! cheat table of found values.

use eframe::egui::{self, RichText};
use memgene_core::constants::{APP_NAME, APP_TAGLINE, FREEZE_INTERVAL_MS};
use memgene_core::scan::{Compare, ScanSession};
use memgene_core::table::{CheatTable, Locator, TableEntry};
use memgene_core::value::{ScanValue, ValueType};
use memgene_core::MemorySource;
use memgene_platform::{attach, list_processes, ProcessInfo, BACKEND_NAME};
use std::time::{Duration, Instant};

use crate::theme;

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

    fn label(self) -> &'static str {
        match self {
            ScanMode::Exact => "Exact value",
            ScanMode::GreaterThan => "Greater than",
            ScanMode::LessThan => "Less than",
            ScanMode::Between => "Between",
            ScanMode::Unknown => "Unknown initial value",
            ScanMode::Changed => "Changed",
            ScanMode::Unchanged => "Unchanged",
            ScanMode::Increased => "Increased",
            ScanMode::Decreased => "Decreased",
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

/// Theme selection: follow the OS, or force one.
#[derive(Clone, Copy, PartialEq)]
enum ThemeChoice {
    System,
    Light,
    Dark,
}

pub struct MemGeneApp {
    // Attachment
    processes: Vec<ProcessInfo>,
    filter: String,
    source: Option<Box<dyn MemorySource>>,
    attached_name: String,

    // Scan controls
    value_type: ValueType,
    mode: ScanMode,
    value_text: String,
    value2_text: String,
    session: Option<ScanSession>,

    // Cheat table
    table: CheatTable,
    entry_counter: u32,

    // Chrome
    theme: ThemeChoice,
    applied_dark: Option<bool>,
    status: String,
    last_freeze: Instant,
}

impl MemGeneApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        MemGeneApp {
            processes: list_processes(),
            filter: String::new(),
            source: None,
            attached_name: String::new(),
            value_type: ValueType::I32,
            mode: ScanMode::Exact,
            value_text: String::new(),
            value2_text: String::new(),
            session: None,
            table: CheatTable::new(),
            entry_counter: 0,
            theme: ThemeChoice::System,
            applied_dark: None,
            status: format!("Ready — {BACKEND_NAME}"),
            last_freeze: Instant::now(),
        }
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

    fn do_first_scan(&mut self) {
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

    fn add_to_table(&mut self, address: u64) {
        self.entry_counter += 1;
        let desired = self
            .source
            .as_deref()
            .and_then(|s| read_value(s, address, self.value_type));
        self.table.add(TableEntry {
            id: 0,
            label: format!("Value {}", self.entry_counter),
            value_type: self.value_type,
            locator: Locator::Absolute(address),
            desired,
            frozen: false,
            notes: String::new(),
        });
        self.status = format!("Added {address:#x} to the table");
    }
}

impl eframe::App for MemGeneApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Resolve and apply the theme only when it actually changes.
        let dark = match self.theme {
            ThemeChoice::System => ctx.style().visuals.dark_mode,
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
        };
        if self.applied_dark != Some(dark) {
            theme::apply(ctx, dark);
            self.applied_dark = Some(dark);
        }

        // Enforce frozen entries on a fixed cadence.
        if let Some(src) = self.source.as_deref() {
            if self.table.entries.iter().any(|e| e.frozen)
                && self.last_freeze.elapsed() >= Duration::from_millis(FREEZE_INTERVAL_MS)
            {
                self.table.tick_frozen(src);
                self.last_freeze = Instant::now();
            }
            ctx.request_repaint_after(Duration::from_millis(FREEZE_INTERVAL_MS));
        }

        self.top_bar(ctx);
        self.process_panel(ctx);
        self.table_panel(ctx);
        self.scan_panel(ctx);
    }
}

// UI sections, split out for readability.
impl MemGeneApp {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading(APP_NAME);
                ui.label(RichText::new(APP_TAGLINE).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_source("theme")
                        .selected_text(match self.theme {
                            ThemeChoice::System => "System",
                            ThemeChoice::Light => "Light",
                            ThemeChoice::Dark => "Dark",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.theme, ThemeChoice::System, "System");
                            ui.selectable_value(&mut self.theme, ThemeChoice::Light, "Light");
                            ui.selectable_value(&mut self.theme, ThemeChoice::Dark, "Dark");
                        });
                    if let Some(name) =
                        (!self.attached_name.is_empty()).then_some(&self.attached_name)
                    {
                        ui.label(RichText::new(format!("● {name}")).weak());
                    }
                });
            });
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(RichText::new(&self.status).weak());
            ui.add_space(2.0);
        });
    }

    fn process_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("processes")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong("Processes");
                    if ui.small_button("Refresh").clicked() {
                        self.processes = list_processes();
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter…"));
                ui.separator();

                let filter = self.filter.to_lowercase();
                let mut attach_pid = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for p in self
                        .processes
                        .iter()
                        .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
                    {
                        let label = format!("{}  ·  {}", p.name, p.pid);
                        if ui.selectable_label(false, label).clicked() {
                            attach_pid = Some((p.pid, p.name.clone()));
                        }
                    }
                });

                if let Some((pid, name)) = attach_pid {
                    match attach(pid) {
                        Ok(src) => {
                            self.source = Some(src);
                            self.attached_name = format!("{name} ({pid})");
                            self.session = None;
                            self.status = format!("Attached to {name} (pid {pid})");
                        }
                        Err(e) => self.status = format!("Attach failed: {e}"),
                    }
                }
            });
    }

    fn scan_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Type");
                egui::ComboBox::from_id_source("vt")
                    .selected_text(self.value_type.label())
                    .show_ui(ui, |ui| {
                        for t in ValueType::ALL {
                            ui.selectable_value(&mut self.value_type, t, t.label());
                        }
                    });

                ui.label("Scan");
                let modes: &[ScanMode] = if self.session.is_some() {
                    &ScanMode::NEXT
                } else {
                    &ScanMode::FIRST
                };
                if !modes.contains(&self.mode) {
                    self.mode = modes[0];
                }
                egui::ComboBox::from_id_source("mode")
                    .selected_text(self.mode.label())
                    .show_ui(ui, |ui| {
                        for m in modes {
                            ui.selectable_value(&mut self.mode, *m, m.label());
                        }
                    });
            });

            ui.horizontal(|ui| {
                if self.mode.needs_value() {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.value_text)
                            .desired_width(120.0)
                            .hint_text("value"),
                    );
                }
                if self.mode.needs_two() {
                    ui.label("…");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.value2_text)
                            .desired_width(120.0)
                            .hint_text("value"),
                    );
                }
            });

            ui.horizontal(|ui| {
                if ui.button("First scan").clicked() {
                    self.do_first_scan();
                }
                ui.add_enabled_ui(self.session.is_some(), |ui| {
                    if ui.button("Next scan").clicked() {
                        self.do_next_scan();
                    }
                });
                if ui.button("Reset").clicked() {
                    self.session = None;
                    self.mode = ScanMode::Exact;
                    self.status = "Scan reset".into();
                }
                if let Some(s) = &self.session {
                    ui.label(RichText::new(format!("{} matches", s.len())).weak());
                }
            });

            ui.separator();
            ui.strong("Results");

            let mut add_addr = None;
            let src = self.source.as_deref();
            let vt = self.value_type;
            egui::ScrollArea::vertical()
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    if let Some(session) = &self.session {
                        egui::Grid::new("results")
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                for m in session.display_matches() {
                                    ui.monospace(format!("{:#014x}", m.address));
                                    let now = src
                                        .and_then(|s| read_value(s, m.address, vt))
                                        .map(|v| v.display())
                                        .unwrap_or_else(|| "—".into());
                                    ui.label(now);
                                    if ui.small_button("＋ Table").clicked() {
                                        add_addr = Some(m.address);
                                    }
                                    ui.end_row();
                                }
                            });
                    } else {
                        ui.label(RichText::new("No scan yet.").weak());
                    }
                });

            if let Some(addr) = add_addr {
                self.add_to_table(addr);
            }
        });
    }

    fn table_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("table")
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong("Cheat table");
                    if ui.small_button("Save…").clicked() {
                        self.save_table();
                    }
                    if ui.small_button("Load…").clicked() {
                        self.load_table();
                    }
                });
                ui.label(
                    RichText::new("Saved addresses persist — no need to rescan next time.").weak(),
                );
                ui.separator();

                let src = self.source.as_deref();
                let mut remove_id = None;
                let mut apply_id = None;

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &mut self.table.entries {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut entry.label)
                                        .desired_width(120.0),
                                );
                                ui.checkbox(&mut entry.frozen, "Freeze");
                                if ui.small_button("✕").clicked() {
                                    remove_id = Some(entry.id);
                                }
                            });
                            ui.horizontal(|ui| {
                                let current = src
                                    .and_then(|s| entry.read_current(s))
                                    .map(|v| v.display())
                                    .unwrap_or_else(|| "—".into());
                                ui.label(RichText::new(format!("now: {current}")).weak());
                                ui.label("→");
                                let mut txt =
                                    entry.desired.map(|v| v.display()).unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut txt)
                                            .desired_width(90.0)
                                            .hint_text("set"),
                                    )
                                    .changed()
                                {
                                    entry.desired = ScanValue::parse(entry.value_type, &txt).ok();
                                }
                                if ui.small_button("Apply").clicked() {
                                    apply_id = Some(entry.id);
                                }
                            });
                        });
                    }
                });

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
            });
    }

    fn save_table(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MemGene table", &[memgene_core::constants::TABLE_FILE_EXT])
            .set_file_name(format!("table.{}", memgene_core::constants::TABLE_FILE_EXT))
            .save_file()
        {
            match self.table.save(&path) {
                Ok(()) => self.status = format!("Saved {}", path.display()),
                Err(e) => self.status = format!("Save failed: {e}"),
            }
        }
    }

    fn load_table(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MemGene table", &[memgene_core::constants::TABLE_FILE_EXT])
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

/// Read one typed value from a source, or `None` if unreadable.
fn read_value(src: &dyn MemorySource, addr: u64, ty: ValueType) -> Option<ScanValue> {
    let mut buf = [0u8; 8];
    let n = src.read(addr, &mut buf[..ty.size()]).ok()?;
    if n < ty.size() {
        return None;
    }
    Some(ScanValue::from_le_bytes(ty, &buf))
}
