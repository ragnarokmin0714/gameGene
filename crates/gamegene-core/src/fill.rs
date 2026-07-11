//! Fill / repeat writer: compute the exact writes to set one field across every
//! record of an array in one go — a fixed value, or an incrementing integer.
//!
//! This module only *plans* the writes (address + little-endian bytes). The app
//! previews the plan, backs up the original bytes, and only then applies it, so
//! a bulk edit is always reversible. Nothing here touches memory.

use crate::value::{ScanValue, ValueType};

/// Hard cap on how many records one fill may touch, so a wrong stride or count
/// can't scribble across huge swaths of memory.
pub const MAX_FILL: usize = 8192;

/// One planned write: where, and the exact bytes to put there.
pub type Write = (u64, Vec<u8>);

/// Address of record `r`'s field.
fn field_addr(base: u64, stride: usize, offset: usize, r: usize) -> u64 {
    base + (r as u64) * (stride as u64) + (offset as u64)
}

/// Plan writing the same `value` into the field at `offset` of `count` records.
pub fn plan_fixed(
    base: u64,
    stride: usize,
    offset: usize,
    value: &ScanValue,
    count: usize,
) -> Vec<Write> {
    let bytes = value.to_le_bytes();
    (0..count.min(MAX_FILL))
        .map(|r| (field_addr(base, stride, offset, r), bytes.clone()))
        .collect()
}

/// Plan writing an incrementing integer (`start`, `start+step`, …) into the
/// field at `offset` of `count` records. The value is truncated to the field's
/// width, wrapping like a raw memory write.
pub fn plan_increment(
    base: u64,
    stride: usize,
    offset: usize,
    ty: ValueType,
    start: i64,
    step: i64,
    count: usize,
) -> Vec<Write> {
    let width = ty.size().min(8);
    (0..count.min(MAX_FILL))
        .map(|r| {
            let n = start.wrapping_add((r as i64).wrapping_mul(step));
            let bytes = n.to_le_bytes()[..width].to_vec();
            (field_addr(base, stride, offset, r), bytes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_writes_same_bytes_at_each_stride() {
        let plan = plan_fixed(0x1000, 16, 4, &ScanValue::I32(99), 3);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].0, 0x1004);
        assert_eq!(plan[1].0, 0x1014);
        assert_eq!(plan[2].0, 0x1024);
        assert_eq!(plan[0].1, 99i32.to_le_bytes());
        assert_eq!(plan[1].1, 99i32.to_le_bytes());
    }

    #[test]
    fn increment_steps_and_truncates_to_width() {
        // u8 field, start 10 step 5 → 10,15,20; each one byte.
        let plan = plan_increment(0x2000, 8, 0, ValueType::U8, 10, 5, 3);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0], (0x2000, vec![10]));
        assert_eq!(plan[1], (0x2008, vec![15]));
        assert_eq!(plan[2], (0x2010, vec![20]));
    }

    #[test]
    fn count_is_capped() {
        let plan = plan_fixed(0, 4, 0, &ScanValue::U8(1), MAX_FILL + 100);
        assert_eq!(plan.len(), MAX_FILL);
    }
}
