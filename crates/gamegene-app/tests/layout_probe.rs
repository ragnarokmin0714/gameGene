//! Headless layout regression tests for the viewer windows (0.11.0 feedback:
//! content spilling past the window frame, and the view shaking when a live
//! value fluctuates).
//!
//! `hex_window` is private to the binary, so these replicate its layout and
//! window settings against a raw `egui::Context` (no display needed) and
//! measure geometry across frames. Keep the settings here in sync with
//! `app/memview.rs` when they change.

use eframe::egui::{self};

const SERIF: &[u8] = include_bytes!("../assets/serif.ttf");

/// Mirror of `theme::CONTROL_HEIGHT` (theme is private to the binary).
const CONTROL_HEIGHT: f32 = 28.0;

/// Replicate the control sizing from `theme::apply`, which the centreline
/// guarantees depend on.
fn install_control_spacing(ctx: &egui::Context) {
    ctx.style_mut(|s| {
        s.spacing.interact_size.y = CONTROL_HEIGHT;
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
    });
}

/// Everything one simulated frame reports back.
struct Frame {
    /// The whole window rect (frame + title bar).
    window: egui::Rect,
    /// How far past the window's bottom/right edge anything painted (the
    /// constant drop shadow included).
    paint_past: egui::Vec2,
}

fn install_serif(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("serif".into(), egui::FontData::from_static(SERIF));
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "serif".into());
    ctx.set_fonts(fonts);
}

/// Run one frame of a replica of the app's memory-viewer window (same window
/// settings and layout skeleton as `hex_window` in `app/memview.rs`).
fn run_frame(ctx: &egui::Context, buf: &[u8; 256], events: Vec<egui::Event>) -> Frame {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let mut window = egui::Rect::NOTHING;
    let out = ctx.run(raw, |ctx| {
        let max_size = ctx.screen_rect().shrink(24.0).size() - egui::vec2(16.0, 56.0);
        let r = egui::Window::new("Memory viewer")
            .resizable(true)
            .default_width(560.0)
            .default_height(480.0)
            .min_width(300.0)
            .min_height(220.0)
            .max_width(max_size.x)
            .max_height(max_size.y)
            .show(ctx, |ui| {
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("0x");
                    let mut addr = String::from("00400000");
                    ui.add(
                        egui::TextEdit::singleline(&mut addr)
                            .desired_width(130.0)
                            .vertical_align(egui::Align::Center)
                            .min_size(egui::vec2(0.0, CONTROL_HEIGHT)),
                    );
                    let _ = ui.button("Go");
                    let _ = ui.button("- 256");
                    let _ = ui.button("+ 256");
                });
                ui.separator();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("hexgrid")
                            .spacing([3.0, 2.0])
                            .show(ui, |ui| {
                                for row in 0..16usize {
                                    ui.monospace(format!(
                                        "{:012X}",
                                        0x0040_0000u64 + row as u64 * 16
                                    ));
                                    for col in 0..16usize {
                                        let _ = ui.selectable_label(
                                            false,
                                            format!("{:02X}", buf[row * 16 + col]),
                                        );
                                    }
                                    let ascii: String = (0..16)
                                        .map(|col| {
                                            let b = buf[row * 16 + col];
                                            if (0x20..0x7f).contains(&b) {
                                                b as char
                                            } else {
                                                '.'
                                            }
                                        })
                                        .collect();
                                    ui.monospace(ascii);
                                    ui.end_row();
                                }
                            });
                    });
            });
        if let Some(r) = r {
            window = r.response.rect;
        }
    });

    let mut painted = egui::Rect::NOTHING;
    for cs in &out.shapes {
        let b = cs.shape.visual_bounding_rect().intersect(cs.clip_rect);
        if b.is_positive() {
            painted = painted.union(b);
        }
    }
    Frame {
        window,
        paint_past: painted.max - window.max,
    }
}

/// Press, move, release the primary button — a corner drag.
fn drag(ctx: &egui::Context, buf: &[u8; 256], from: egui::Pos2, to: egui::Pos2) {
    let press = egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    };
    let release = egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    };
    run_frame(ctx, buf, vec![egui::Event::PointerMoved(from), press]);
    run_frame(ctx, buf, vec![egui::Event::PointerMoved(to)]);
    run_frame(ctx, buf, vec![release]);
}

fn test_bytes() -> [u8; 256] {
    let mut buf = [0u8; 256];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i * 7) as u8;
    }
    buf
}

/// 0.12.0 feedback: control rows mixed full-size and small buttons and shorter
/// text inputs, so rows looked vertically ragged. Replicate one control row
/// (the memory-viewer address bar) and one array-window record row and assert
/// everything shares a height / sits on one centreline.
#[test]
fn control_rows_and_grid_rows_share_a_centreline() {
    let ctx = egui::Context::default();
    install_serif(&ctx);
    install_control_spacing(&ctx);

    let mut controls: Vec<egui::Rect> = Vec::new();
    let mut row_label = egui::Rect::NOTHING;
    let mut row_cells: Vec<egui::Rect> = Vec::new();
    // A Grid sizes its rows/columns from the previous frame's state, so run a
    // few frames and assert on the settled layout (what the user sees).
    for _frame in 0..3 {
        controls.clear();
        row_cells.clear();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // The address bar: input and buttons all at the control height.
                ui.horizontal_wrapped(|ui| {
                    ui.label("0x");
                    let mut addr = String::from("00400000");
                    let edit = ui
                        .add(
                            egui::TextEdit::singleline(&mut addr)
                                .desired_width(130.0)
                                .vertical_align(egui::Align::Center)
                                .min_size(egui::vec2(0.0, CONTROL_HEIGHT)),
                        )
                        .rect;
                    // A TextEdit response reports the inner rect; its painted frame
                    // is that plus the default margin (egui 0.28 builder.rs). The
                    // frame is what must line up with the buttons.
                    controls.push(edit.expand2(egui::vec2(4.0, 2.0)));
                    controls.push(ui.button("Go").rect);
                    controls.push(ui.button("- 256").rect);
                    controls.push(ui.button("+ 256").rect);
                });
                // One record row of the array grid: address label + value cells.
                egui::Grid::new("arr").striped(true).show(ui, |ui| {
                    row_label = ui.monospace("000000400000").rect;
                    let h = ui.spacing().interact_size.y;
                    for v in ["100", "3.500"] {
                        row_cells.push(ui.add_sized([92.0, h], egui::Button::new(v).small()).rect);
                    }
                    ui.end_row();
                });
            });
        });
    }

    let first = controls[0];
    for r in &controls {
        assert!(
            (r.height() - first.height()).abs() <= 1.0,
            "control heights differ: {controls:#?}"
        );
        assert!(
            (r.center().y - first.center().y).abs() <= 1.0,
            "controls are off the row centreline: {controls:#?}"
        );
    }
    for c in &row_cells {
        assert!(
            (row_label.center().y - c.center().y).abs() <= 1.0,
            "grid label not centred against its row's cells: label {row_label:?} cell {c:?}"
        );
    }
}

/// A fluctuating live value must not change the window size frame to frame.
#[test]
fn window_size_is_stable_while_a_value_fluctuates() {
    let ctx = egui::Context::default();
    install_serif(&ctx);
    let mut buf = test_bytes();
    let mut sizes = Vec::new();
    for frame in 0..8 {
        let v = 3.15f32 + frame as f32 * 41.7;
        buf[0x40..0x44].copy_from_slice(&v.to_le_bytes());
        sizes.push(run_frame(&ctx, &buf, vec![]).window.size());
    }
    for pair in sizes.windows(2) {
        assert_eq!(pair[0], pair[1], "window size changed between frames");
    }
}

/// Resizing must be clamped: never larger than the viewport (this is also what
/// recovers a stale oversized size persisted by an old version), never smaller
/// than the fixed control bar, and nothing may paint past the frame beyond the
/// constant drop shadow.
#[test]
fn window_stays_inside_viewport_and_frame() {
    let ctx = egui::Context::default();
    install_serif(&ctx);
    let mut buf = test_bytes();

    // Warm up and record the shadow as the painting baseline.
    run_frame(&ctx, &buf, vec![]);
    let base = run_frame(&ctx, &buf, vec![]);
    let shadow = base.paint_past;

    // Drag the resize corner far past the screen: the window must stay inside
    // the 1280x800 viewport.
    let corner = base.window.max - egui::vec2(2.0, 2.0);
    drag(&ctx, &buf, corner, corner + egui::vec2(2000.0, 1500.0));
    let huge = run_frame(&ctx, &buf, vec![]);
    assert!(
        huge.window.width() <= 1280.0 && huge.window.height() <= 800.0,
        "window escaped the viewport: {:?}",
        huge.window
    );

    // Drag the corner very small: the minimum size must hold, and even with a
    // fluctuating value nothing may paint past the frame (the bar wraps, the
    // body scrolls).
    let corner = huge.window.max - egui::vec2(2.0, 2.0);
    drag(
        &ctx,
        &buf,
        corner,
        huge.window.min + egui::vec2(100.0, 80.0),
    );
    for frame in 0..5 {
        let v = 3.15f32 + frame as f32 * 41.7;
        buf[0x40..0x44].copy_from_slice(&v.to_le_bytes());
        let f = run_frame(&ctx, &buf, vec![]);
        assert!(
            f.window.width() >= 299.0 && f.window.height() >= 219.0,
            "window shrank below its minimum: {:?}",
            f.window
        );
        assert!(
            f.paint_past.x <= shadow.x + 1.0 && f.paint_past.y <= shadow.y + 1.0,
            "content painted past the frame: {:?} (shadow baseline {:?})",
            f.paint_past,
            shadow
        );
    }
}
