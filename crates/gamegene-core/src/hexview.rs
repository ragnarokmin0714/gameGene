//! Helpers for the memory viewer: interpret raw bytes as every value type.
//!
//! The view itself (a hex grid over a small, windowed read) lives in the app;
//! this is the pure, testable part — given the bytes under the cursor, decode
//! them as each type that fits, for the "what is this?" side panel.

use crate::value::{ScanValue, ValueType};

/// Decode the value at the start of `buf` as every [`ValueType`] that fits,
/// in display order. Types wider than `buf` are skipped.
pub fn interpret(buf: &[u8]) -> Vec<(ValueType, ScanValue)> {
    ValueType::ALL
        .iter()
        .filter(|ty| buf.len() >= ty.size())
        .map(|&ty| (ty, ScanValue::from_le_bytes(ty, buf)))
        .collect()
}

/// The printable ASCII character for a byte, or `.` for non-printable — used
/// for the ASCII gutter of the hex grid.
pub fn ascii_char(b: u8) -> char {
    if (0x20..0x7f).contains(&b) {
        b as char
    } else {
        '.'
    }
}
