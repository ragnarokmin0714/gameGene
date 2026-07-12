//! The scan panel: single-value scan, group scan, find bytes/text, and the
//! shared results list.

use super::*;

impl GameGeneApp {
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

    pub(super) fn do_first_scan(&mut self) {
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

    pub(super) fn do_next_scan(&mut self) {
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
    /// Parse the group-scan query into typed values (at least two).
    fn parse_group_values(&mut self) -> Option<Vec<ScanValue>> {
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
                return None;
            }
        };
        if values.len() < 2 {
            self.status = "Enter at least two values, separated by spaces.".into();
            return None;
        }
        Some(values)
    }

    /// and find where they all occur within `group_span` bytes of each other.
    fn do_group_scan(&mut self) {
        let Some(values) = self.parse_group_values() else {
            return;
        };
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

    /// Narrow the previous group-scan hits with the values as they are *now*
    /// (change them in game first, then type the new numbers and rescan).
    fn do_group_rescan(&mut self) {
        let Some(values) = self.parse_group_values() else {
            return;
        };
        let Some(src) = self.source.as_deref() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        let before = self.group_results.len();
        self.group_results = group_rescan(src, &self.group_results, &values);
        self.status = format!(
            "Group rescan: {} of {before} match(es) left",
            self.group_results.len()
        );
    }

    pub(super) fn scan_panel(&mut self, ctx: &egui::Context) {
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
        let mut do_rescan = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.group_query)
                    .desired_width(200.0)
                    .hint_text(tr.group_values_hint),
            );
            ui.label(tr.group_span);
            ui.add(egui::DragValue::new(&mut self.group_span).range(4..=65536));
            if ui.button(tr.first_scan).clicked() {
                do_group = true;
            }
            // Narrow the hits after changing the values in game — the group
            // counterpart of the single-value "next scan".
            if ui
                .add_enabled(
                    !self.group_results.is_empty(),
                    egui::Button::new(tr.next_scan),
                )
                .clicked()
            {
                do_rescan = true;
            }
        });
        if do_group {
            self.do_group_scan();
        }
        if do_rescan {
            self.do_group_rescan();
        }

        ui.separator();
        ui.strong(tr.results);
        let addrs: Vec<u64> = self.group_results.iter().map(|h| h.anchor).collect();
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
}
