//! The process list panel: refresh, filter, game detection, attach/detach.

use super::*;

impl GameGeneApp {
    pub(super) fn attach_to(&mut self, pid: u32, name: String) {
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

    pub(super) fn process_panel(&mut self, ctx: &egui::Context) {
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
}

/// Whether a process is the OS shell / system UI rather than a real app.
/// These briefly take the foreground as the user switches windows (clicking the
/// taskbar, alt-tab, the desktop), so they must not overwrite the detected game.
pub(super) fn is_shell_process(name: &str) -> bool {
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
