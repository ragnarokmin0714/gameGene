//! Helpers for the memory viewer: interpret raw bytes as every value type.
//!
//! The view itself (a hex grid over a small, windowed read) lives in the app;
//! this is the pure, testable part — given the bytes under the cursor, decode
//! them as each type that fits, for the "what is this?" side panel.

use crate::value::{ScanValue, ValueType};

/// Bytes per row in the hex grid; the window start is aligned to this.
pub const HEX_ROW: u64 = 16;

/// Where the viewer should land when navigating to `addr`: the row-aligned
/// window start and the byte to select.
///
/// Both entry points into the viewer — a scan-result double-click and the
/// "Go" address box — must produce this, so navigation *always* moves the
/// window **and** re-selects. When only the window moves, a stale selection
/// (usually outside the new window) leaves the inspector stuck on the old
/// address; that was the "Go doesn't re-annotate" bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexFocus {
    /// Row-aligned start of the 256-byte window to read.
    pub window: u64,
    /// The byte the inspector reads and decodes.
    pub selected: u64,
}

/// Compute the [`HexFocus`] for jumping to `addr`.
pub fn focus_on(addr: u64) -> HexFocus {
    HexFocus {
        window: addr & !(HEX_ROW - 1),
        selected: addr,
    }
}

/// Offset of the selected byte within a window read of `got` bytes starting
/// at `window`, or `None` when the selection lies outside the read region
/// (so the inspector shows nothing rather than decoding stale/foreign bytes).
pub fn selected_offset(window: u64, selected: u64, got: usize) -> Option<usize> {
    let off = selected.wrapping_sub(window) as usize;
    (off < got).then_some(off)
}

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
