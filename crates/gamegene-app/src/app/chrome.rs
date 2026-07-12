//! App chrome: the top bar, the settings window, and keyboard shortcuts.

use super::*;

impl GameGeneApp {
    /// Capture a key when re-binding, otherwise fire any matching shortcut.
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
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

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
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

    pub(super) fn top_bar(&mut self, ctx: &egui::Context) {
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
                            Lang::Ja => "日本語",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.lang, Lang::En, "English");
                            ui.selectable_value(&mut self.lang, Lang::ZhHant, "繁體中文");
                            ui.selectable_value(&mut self.lang, Lang::Ja, "日本語");
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
