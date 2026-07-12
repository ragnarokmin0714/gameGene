//! The memory viewer window: hex grid, value inspector, and writes.

use super::*;

impl GameGeneApp {
    /// Open the memory viewer focused on `a` (row-aligned, byte selected).
    pub(super) fn open_hex_at(&mut self, a: u64) {
        self.show_hex = true;
        self.hex_addr = a & !0xF;
        self.hex_sel = Some(a);
        self.hex_addr_input = format!("{a:X}");
    }

    pub(super) fn hex_window(&mut self, ctx: &egui::Context) {
        if !self.show_hex {
            return;
        }
        let tr = self.tr();
        let mut open = true;
        let mut new_sel = None;
        let mut do_write = None;
        let mut add_addr = None;
        let mut dissect_from = None;

        // Window sizes persist in egui memory across restarts, so a size saved
        // by an old version (which auto-grew past the viewport) would otherwise
        // come back forever, with the bottom and the resize corner out of
        // reach. Clamp to the app window every frame.
        let max_size = ctx.screen_rect().shrink(24.0).size() - egui::vec2(16.0, 56.0);
        egui::Window::new(tr.mem_title)
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .default_height(480.0)
            .min_width(300.0)
            .min_height(220.0)
            .max_width(max_size.x)
            .max_height(max_size.y)
            .show(ctx, |ui| {
                // Windowed read: only the visible 256 bytes, so this is cheap.
                let mut buf = [0u8; 256];
                let got = self
                    .source
                    .as_deref()
                    .map(|s| s.read(self.hex_addr, &mut buf).unwrap_or(0))
                    .unwrap_or(0);

                // Fixed address bar; the body below scrolls independently. It
                // wraps at narrow widths so it can never poke past the frame.
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
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
                ui.separator();

                // Everything below the address bar shares one scroll area (no
                // nested panels), so the window stays a fixed viewport: the body
                // scrolls instead of spilling past the frame or jittering when a
                // live value changes width. The inspector sits at the top for
                // easy reach.
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
}
