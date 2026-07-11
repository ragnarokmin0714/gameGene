//! Tests for the memory-viewer byte interpretation helpers.

use gamegene_core::hexview::{ascii_char, interpret};
use gamegene_core::value::{ScanValue, ValueType};

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
