//! Tests for the memory-viewer byte interpretation and navigation helpers.

use gamegene_core::hexview::{ascii_char, focus_on, interpret, selected_offset, HEX_ROW};
use gamegene_core::value::{ScanValue, ValueType};

#[test]
fn focus_aligns_the_window_and_selects_the_exact_byte() {
    let f = focus_on(0x1007);
    assert_eq!(f.window, 0x1000, "window aligns down to the 16-byte row");
    assert_eq!(f.selected, 0x1007, "the exact byte stays selected");
    // An already-aligned address is its own window start.
    let f = focus_on(0x2000);
    assert_eq!(f.window, 0x2000);
    assert_eq!(f.selected, 0x2000);
}

#[test]
fn go_re_selects_into_the_new_window() {
    // Regression: jumping to a far address (the "Go" box) must land with the
    // selection *inside* the freshly read window, so the inspector follows
    // the jump instead of lingering on the old address. Before the fix Go
    // moved only the window and this offset came out as None.
    let f = focus_on(0x4_0005);
    let got = 256; // a full window read
    let off = selected_offset(f.window, f.selected, got);
    assert_eq!(
        off,
        Some(5),
        "selection resolves to a byte within the window"
    );
}

#[test]
fn selection_outside_the_read_region_is_none() {
    // This is exactly the stale-selection case the bug produced: the old
    // selection sits far from the current window, so nothing is decoded.
    assert_eq!(selected_offset(0x1000, 0x9999, 256), None);
    // Just past the end of a short read is also out.
    assert_eq!(selected_offset(0x1000, 0x1000 + 200, 128), None);
    // The last byte actually read is in.
    assert_eq!(selected_offset(0x1000, 0x1000 + 127, 128), Some(127));
    // A selection *before* the window (wrapping) never resolves.
    assert_eq!(selected_offset(0x1000, 0x0FFF, 256), None);
}

#[test]
fn hex_row_is_sixteen() {
    assert_eq!(HEX_ROW, 16);
}

#[test]
fn interprets_every_fitting_type() {
    // 100 as little-endian i32, padded to 8 bytes.
    let mut buf = [0u8; 8];
    buf[..4].copy_from_slice(&100i32.to_le_bytes());

    let got = interpret(&buf);
    // All ten types fit in 8 bytes.
    assert_eq!(got.len(), ValueType::ALL.len());
    // The i32 reading is 100.
    assert!(got.contains(&(ValueType::I32, ScanValue::I32(100))));
    // The u8 reading is the first byte (100).
    assert!(got.contains(&(ValueType::U8, ScanValue::U8(100))));
}

#[test]
fn skips_types_that_do_not_fit() {
    let buf = [1u8, 0u8]; // only 2 bytes
    let types: Vec<ValueType> = interpret(&buf).into_iter().map(|(t, _)| t).collect();
    assert!(types.contains(&ValueType::I8));
    assert!(types.contains(&ValueType::I16));
    assert!(!types.contains(&ValueType::I32)); // needs 4 bytes
    assert!(!types.contains(&ValueType::F64)); // needs 8 bytes
}

#[test]
fn ascii_gutter_maps_printable_and_control() {
    assert_eq!(ascii_char(b'A'), 'A');
    assert_eq!(ascii_char(0x00), '.');
    assert_eq!(ascii_char(0x7f), '.');
    assert_eq!(ascii_char(0xFF), '.');
}
