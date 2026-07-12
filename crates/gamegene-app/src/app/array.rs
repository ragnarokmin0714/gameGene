//! The array / structure window: stride detection, the record grid, and
//! fill / bulk write.

use super::*;

impl GameGeneApp {
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
    pub(super) fn struct_detect(&mut self) {
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

    pub(super) fn struct_window(&mut self, ctx: &egui::Context) {
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

        // Same viewport clamp as the memory viewer: never let a persisted or
        // grown size push the frame (and its resize corner) off the app window.
        let max_size = ctx.screen_rect().shrink(24.0).size() - egui::vec2(16.0, 56.0);
        egui::Window::new(tr.arr_title)
            .open(&mut open)
            .resizable(true)
            .default_width(660.0)
            .default_height(460.0)
            .min_width(320.0)
            .min_height(240.0)
            .max_width(max_size.x)
            .max_height(max_size.y)
            .show(ctx, |ui| {
                // Fixed control bar; the body below scrolls independently. It
                // wraps at narrow widths so it can never poke past the frame.
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("0x");
                    ui.add(
                        control_edit(&mut self.struct_base_input, 120.0)
                            .hint_text(tr.mem_addr_hint),
                    );
                    if ui.button(tr.arr_detect).clicked() {
                        detect = true;
                    }
                    ui.separator();
                    ui.label(tr.arr_stride);
                    ui.add(control_edit(&mut self.struct_stride_input, 56.0));
                    if ui.button(tr.arr_apply).clicked() {
                        apply = true;
                    }
                    ui.label(tr.arr_rows);
                    ui.add(egui::DragValue::new(&mut self.struct_rows).range(1..=256));
                });
                ui.label(RichText::new(tr.arr_hint).weak());
                ui.separator();

                // No nested panels: the window stays a fixed viewport and the
                // body scrolls, so nothing spills past the frame.
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
                                            let lbl = format!("+{:X} {}", f.offset, f.ty.label());
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
                                ui.add(control_edit(&mut self.fill_value, 90.0));
                                if self.fill_increment {
                                    ui.label(tr.fill_step);
                                    ui.add(control_edit(&mut self.fill_step, 50.0));
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
                                            let hex: String =
                                                bytes.iter().map(|b| format!("{b:02X} ")).collect();
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
                                    let resp =
                                        ui.add_sized([92.0, h], egui::Button::new(shown).small());
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
}
