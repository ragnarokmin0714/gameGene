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
        if self.scan_job.is_some() {
            return; // a scan is already running
        }
        self.normalize_between_inputs();
        let Some(src) = self.source.clone() else {
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
        // Relative predicates need a previous scan; reject before spawning so
        // the error shows immediately rather than after a thread round-trip.
        if let Err(e) = compare_valid_for_first(compare) {
            self.status = e;
            return;
        }
        self.status = "Scanning…".into();
        self.scan_job = Some(ScanJob::first(src, self.value_type, compare));
    }

    pub(super) fn do_next_scan(&mut self) {
        if self.scan_job.is_some() {
            return;
        }
        self.normalize_between_inputs();
        let compare = match self.build_compare() {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        let Some(src) = self.source.clone() else {
            self.status = "Run a first scan before narrowing.".into();
            return;
        };
        let Some(session) = self.session.take() else {
            self.status = "Run a first scan before narrowing.".into();
            return;
        };
        self.status = "Narrowing…".into();
        self.scan_job = Some(ScanJob::next(src, Box::new(session), compare));
    }

    /// Poll a running scan; when it finishes, install the result (or discard it
    /// if the user cancelled) and clear the job. Called every frame from the
    /// update loop.
    pub(super) fn poll_scan_job(&mut self) {
        let Some(job) = self.scan_job.as_mut() else {
            return;
        };
        let cancelling = job.is_cancelling();
        let Some(done) = job.poll() else {
            return;
        };
        self.scan_job = None;
        match done {
            JobDone::First(result) => {
                if cancelling {
                    self.status = "Scan cancelled".into();
                    return;
                }
                match result {
                    Ok(s) => {
                        self.status = if s.truncated() {
                            format!(
                                "Stopped at {} matches — value too common, narrow it",
                                s.len()
                            )
                        } else {
                            format!("First scan: {} matches", s.len())
                        };
                        self.session = Some(s);
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
            JobDone::Next { session, result } => {
                // Put the session back regardless; narrowing mutates it in place.
                let session = *session;
                if cancelling {
                    self.status = "Scan cancelled".into();
                    self.session = Some(session);
                    return;
                }
                match result {
                    Ok(()) => self.status = format!("Narrowed to {} matches", session.len()),
                    Err(e) => self.status = e.to_string(),
                }
                self.session = Some(session);
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

    /// Parse the group-scan query into typed queries (at least two), reporting
    /// problems in the status line.
    fn parse_group_values(&mut self) -> Option<Vec<GroupQuery>> {
        let queries = match parse_group_query(self.value_type, &self.group_query) {
            Ok(q) => q,
            Err(e) => {
                self.status = e;
                return None;
            }
        };
        if queries.len() < 2 {
            self.status = "Enter at least two values, separated by spaces.".into();
            return None;
        }
        Some(queries)
    }

    /// Spawn a first group scan on a background thread — find where the values
    /// all occur within `group_span` bytes of each other.
    fn do_group_scan(&mut self) {
        if self.group_job.is_some() || self.group_scanned {
            return;
        }
        let Some(values) = self.parse_group_values() else {
            return;
        };
        let Some(src) = self.source.clone() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        self.status = "Scanning…".into();
        self.group_job = Some(GroupJob::first(
            src,
            values,
            self.group_span,
            gamegene_core::constants::MAX_RESULTS_DISPLAY,
        ));
    }

    /// Narrow the previous group-scan hits with the values as they are *now*
    /// (change them in game first, then type the new numbers and rescan).
    fn do_group_rescan(&mut self) {
        if self.group_job.is_some() {
            return;
        }
        let Some(values) = self.parse_group_values() else {
            return;
        };
        let Some(src) = self.source.clone() else {
            self.status = "Attach to a process first.".into();
            return;
        };
        self.status = "Narrowing…".into();
        self.group_job = Some(GroupJob::next(
            src,
            std::mem::take(&mut self.group_results),
            values,
        ));
    }

    /// Reset the group scan: drop results and re-enable first scan.
    fn reset_group_scan(&mut self) {
        self.group_results.clear();
        self.group_scanned = false;
        self.status = "Group scan reset".into();
    }

    /// Poll a running group scan; install results (or discard on cancel).
    pub(super) fn poll_group_job(&mut self) {
        let Some(job) = self.group_job.as_mut() else {
            return;
        };
        let cancelling = job.is_cancelling();
        let Some(done) = job.poll() else {
            return;
        };
        self.group_job = None;
        if cancelling {
            self.status = "Scan cancelled".into();
            // A cancelled first scan leaves nothing scanned; a cancelled rescan
            // consumed the old results, so either way there is nothing to keep.
            self.group_scanned = !self.group_results.is_empty();
            return;
        }
        match done {
            GroupDone::First(hits) => {
                self.group_results = hits;
                self.group_scanned = true;
                self.status = format!("Group scan: {} match(es)", self.group_results.len());
            }
            GroupDone::Next(hits) => {
                self.group_results = hits;
                self.status = format!("Group rescan: {} match(es) left", self.group_results.len());
            }
        }
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
                    ui.add(control_edit(&mut self.value_text, 120.0).hint_text(tr.value_hint));
                }
                if self.mode.needs_two() {
                    ui.label("…");
                    ui.add(control_edit(&mut self.value2_text, 120.0).hint_text(tr.value_hint));
                }
            });

            // While a scan runs on the worker thread, replace the buttons with a
            // progress bar and a cancel button; keep repainting so it animates.
            if let Some(job) = self.scan_job.as_mut() {
                let label = match job.kind {
                    JobKind::First => tr.scanning,
                    JobKind::Next => tr.narrowing,
                };
                ui.horizontal(|ui| {
                    let mut bar = egui::ProgressBar::new(job.fraction().unwrap_or(0.0))
                        .desired_width(220.0)
                        .text(label);
                    if job.fraction().is_none() {
                        bar = bar.animate(true);
                    }
                    ui.add(bar);
                    ui.add_enabled_ui(!job.is_cancelling(), |ui| {
                        if ui.button(tr.cancel_scan).clicked() {
                            job.request_cancel();
                        }
                    });
                });
                // Keep animating the bar while the worker runs.
                ui.ctx().request_repaint();
            } else {
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
            }

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
                    let resp =
                        ui.add(control_edit(&mut self.find_query, 180.0).hint_text(tr.find_hint));
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
        let mut do_reset = false;
        ui.horizontal(|ui| {
            ui.add(control_edit(&mut self.group_query, 200.0).hint_text(tr.group_values_hint));
            bar_label(ui, tr.group_span);
            ui.add(egui::DragValue::new(&mut self.group_span).range(4..=65536));
        });
        // While a group scan runs on the worker thread, show an (indeterminate)
        // progress bar + Cancel; group scan sweeps once per value, so there is
        // no single fraction to report.
        if let Some(job) = self.group_job.as_mut() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(0.0)
                        .desired_width(220.0)
                        .text(tr.scanning)
                        .animate(true),
                );
                ui.add_enabled_ui(!job.is_cancelling(), |ui| {
                    if ui.button(tr.cancel_scan).clicked() {
                        job.request_cancel();
                    }
                });
            });
            ui.ctx().request_repaint();
        } else {
            ui.horizontal(|ui| {
                // First scan locks after it runs (like value scan) so an
                // accidental click can't wipe narrowed results — Reset unlocks it.
                ui.add_enabled_ui(!self.group_scanned, |ui| {
                    if ui.button(tr.first_scan).clicked() {
                        do_group = true;
                    }
                });
                ui.add_enabled_ui(self.group_scanned && !self.group_results.is_empty(), |ui| {
                    if ui.button(tr.next_scan).clicked() {
                        do_rescan = true;
                    }
                });
                if ui.button(tr.reset).clicked() {
                    do_reset = true;
                }
                if self.group_scanned {
                    ui.label(
                        RichText::new(format!("{} {}", self.group_results.len(), tr.matches))
                            .weak(),
                    );
                }
            });
        }
        // Live interpretation of the query: floats become v…v+1 ranges, so
        // typing `12` immediately shows the `12…13` that will be searched.
        if let Ok(qs) = parse_group_query(self.value_type, &self.group_query) {
            if qs.iter().any(|q| matches!(q, GroupQuery::Range(..))) {
                let ranges: Vec<String> = qs.iter().map(group_query_label).collect();
                ui.label(
                    RichText::new(format!("{} {}", tr.group_range_note, ranges.join("  "))).weak(),
                );
            }
        }
        if do_group {
            self.do_group_scan();
        }
        if do_rescan {
            self.do_group_rescan();
        }
        if do_reset {
            self.reset_group_scan();
        }

        ui.separator();
        ui.strong(tr.results);
        let mut add_addr = None;
        let mut goto_addr = None;
        let src = self.source.as_deref();
        let vt = self.value_type;
        egui::ScrollArea::vertical()
            .id_source("group_results")
            .max_height(ui.available_height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("group_results_grid")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        for hit in &self.group_results {
                            let address = hit.anchor;
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
                            ui.add_sized(
                                [90.0, ui.spacing().interact_size.y],
                                egui::Label::new(&now).truncate(),
                            )
                            .on_hover_text(&now);
                            // Where the other values sit relative to the anchor
                            // — the group's layout, no Dissect needed (Dissect
                            // is for repeating arrays, not a single struct).
                            let (cells, lines) = group_offsets(src, vt, hit);
                            ui.add_sized(
                                [150.0, ui.spacing().interact_size.y],
                                egui::Label::new(RichText::new(cells).monospace()).truncate(),
                            )
                            .on_hover_text(format!("{}\n{lines}", tr.group_others_hint));
                            if ui.small_button(tr.add_table).clicked() {
                                add_addr = Some(address);
                            }
                            ui.end_row();
                        }
                    });
            });
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

/// Reject relative predicates for a first scan (they need a previous scan to
/// compare against). Mirrors the engine's own check, but done before spawning
/// the worker so the error is immediate.
fn compare_valid_for_first(c: Compare) -> Result<(), String> {
    match c {
        Compare::Changed | Compare::Unchanged | Compare::Increased | Compare::Decreased => Err(
            "That comparison needs a previous scan — first scan with a value or Unknown.".into(),
        ),
        _ => Ok(()),
    }
}

/// Parse a group-scan query string: values separated by spaces or commas;
/// brackets and parentheses are ignored, so a pasted `[ 100  50  12 ]` works.
/// Float values match a `v…v+1` range (the HUD usually shows only the integer
/// part, so the exact bits are unknowable); integers match exactly.
fn parse_group_query(ty: ValueType, text: &str) -> Result<Vec<GroupQuery>, String> {
    text.split(|c: char| matches!(c, ',' | '[' | ']' | '(' | ')') || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|tok| {
            let v = ScanValue::parse(ty, tok).map_err(|e| e.to_string())?;
            Ok(match v {
                ScanValue::F32(f) => GroupQuery::Range(v, ScanValue::F32(f + 1.0)),
                ScanValue::F64(f) => GroupQuery::Range(v, ScanValue::F64(f + 1.0)),
                _ => GroupQuery::Exact(v),
            })
        })
        .collect()
}

/// Render a group query for the interpretation line under the input:
/// `12…13` for a range, the plain value otherwise.
fn group_query_label(q: &GroupQuery) -> String {
    match q {
        GroupQuery::Exact(v) => v.display(),
        GroupQuery::Range(lo, hi) => format!("{}…{}", lo.display(), hi.display()),
    }
}

/// Format a hit's other values as signed hex offsets from the anchor, with
/// their live values: a compact cell (`+4:20.71  +8:6.02`) plus one hover line
/// per value with the full number and absolute address.
fn group_offsets(
    src: Option<&dyn MemorySource>,
    vt: ValueType,
    hit: &GroupHit,
) -> (String, String) {
    let mut cells = Vec::new();
    let mut lines = Vec::new();
    for &a in &hit.others {
        let delta = a as i128 - hit.anchor as i128;
        let off = if delta >= 0 {
            format!("+{delta:X}")
        } else {
            format!("-{:X}", -delta)
        };
        let val = src
            .and_then(|s| read_value(s, a, vt))
            .map(|v| v.display())
            .unwrap_or_else(|| "—".into());
        cells.push(format!("{off}:{}", short_value(&val, 8)));
        lines.push(format!("{off} = {val} @ {a:#014x}"));
    }
    (cells.join("  "), lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_query_ignores_brackets_and_commas() {
        let qs = parse_group_query(ValueType::I32, "[ 100,   50    12 ]").unwrap();
        assert_eq!(
            qs,
            vec![
                GroupQuery::Exact(ScanValue::I32(100)),
                GroupQuery::Exact(ScanValue::I32(50)),
                GroupQuery::Exact(ScanValue::I32(12)),
            ]
        );
    }

    #[test]
    fn group_query_floats_become_plus_one_ranges() {
        let qs = parse_group_query(ValueType::F32, "12 20.5").unwrap();
        assert_eq!(
            qs,
            vec![
                GroupQuery::Range(ScanValue::F32(12.0), ScanValue::F32(13.0)),
                GroupQuery::Range(ScanValue::F32(20.5), ScanValue::F32(21.5)),
            ]
        );
        assert_eq!(group_query_label(&qs[0]), "12.0…13.0");
    }

    #[test]
    fn group_query_rejects_junk() {
        assert!(parse_group_query(ValueType::I32, "100 abc").is_err());
    }
}
