//! The cheat table panel: entries, freeze/apply, pinning, save/load.

use super::*;

impl GameGeneApp {
    pub(super) fn add_to_table(&mut self, address: u64, value_type: ValueType) {
        self.entry_counter += 1;
        let label = format!("Value {}", self.entry_counter);
        self.add_to_table_labeled(address, value_type, label);
    }

    /// Add an entry under a specific label (used by the array-cell confirmation).
    pub(super) fn add_to_table_labeled(
        &mut self,
        address: u64,
        value_type: ValueType,
        label: String,
    ) {
        let desired = self
            .source
            .as_deref()
            .and_then(|s| read_value(s, address, value_type));
        let label = if label.trim().is_empty() {
            self.entry_counter += 1;
            format!("Value {}", self.entry_counter)
        } else {
            label
        };
        self.table.add(TableEntry {
            id: 0,
            label,
            value_type,
            locator: Locator::Absolute(address),
            desired,
            frozen: false,
            notes: String::new(),
        });
        self.status = format!("Added {address:#x} to the table");
    }

    /// The array cell-add confirmation. Shown when a cell was clicked; lets the
    /// user name the entry (or cancel) before it lands in the table.
    pub(super) fn confirm_add_window(&mut self, ctx: &egui::Context) {
        let Some((addr, ty)) = self.pending_add else {
            return;
        };
        let tr = self.tr();
        let value = self
            .source
            .as_deref()
            .and_then(|s| read_value(s, addr, ty))
            .map(|v| v.display())
            .unwrap_or_else(|| "—".into());

        let mut open = true;
        let mut do_add = false;
        let mut cancel = false;
        egui::Window::new(tr.add_confirm_title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.monospace(format!("{addr:#014X}  {}  = {value}", ty.label()));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    bar_label(ui, tr.add_confirm_label);
                    let resp = ui.add(control_edit(&mut self.pending_add_label, 180.0));
                    // Enter in the name field confirms.
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        do_add = true;
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(tr.add_confirm_ok).clicked() {
                        do_add = true;
                    }
                    if ui.button(tr.cancel_scan).clicked() {
                        cancel = true;
                    }
                });
            });

        if do_add {
            let label = self.pending_add_label.clone();
            self.add_to_table_labeled(addr, ty, label);
            self.pending_add = None;
        } else if cancel || !open {
            self.pending_add = None;
        }
    }

    pub(super) fn table_panel(&mut self, ctx: &egui::Context) {
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
                                // Fixed-width cell so a live value changing
                                // length (a fluctuating float) does not reflow
                                // the row and shake the panel; full value on
                                // hover, like the memory-viewer inspector.
                                ui.allocate_ui_with_layout(
                                    egui::vec2(130.0, ui.spacing().interact_size.y),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "{}{current}",
                                                    tr.now_prefix
                                                ))
                                                .weak(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&current);
                                    },
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

    pub(super) fn save_table(&mut self) {
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

    pub(super) fn load_table(&mut self) {
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
