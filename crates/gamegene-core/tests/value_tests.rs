//! Value comparison tests — chiefly that ordering stays exact for integers
//! too large to survive a round trip through `f64`.

use gamegene_core::value::ScanValue;
use std::cmp::Ordering;

/// The first integer that `f64` cannot represent: 2^53 + 1 collapses onto 2^53.
const BEYOND_F64: i64 = (1i64 << 53) + 1;

#[test]
fn ordering_is_exact_past_the_f64_mantissa() {
    let big = ScanValue::I64(BEYOND_F64);
    let smaller = ScanValue::I64(1i64 << 53);
    // Both are the same `f64`; only an exact integer compare can separate them.
    assert_eq!(big.as_f64(), smaller.as_f64());
    assert_eq!(big.num_cmp(&smaller), Some(Ordering::Greater));
    assert_eq!(smaller.num_cmp(&big), Some(Ordering::Less));
    assert_eq!(big.num_cmp(&big), Some(Ordering::Equal));
}

#[test]
fn unsigned_64_bit_ordering_is_exact_at_the_top_of_the_range() {
    let max = ScanValue::U64(u64::MAX);
    let one_less = ScanValue::U64(u64::MAX - 1);
    assert_eq!(max.as_f64(), one_less.as_f64());
    assert_eq!(max.num_cmp(&one_less), Some(Ordering::Greater));
}

#[test]
fn signed_and_unsigned_compare_without_wrapping() {
    // -1 as u64 is huge; comparing the two must use their numeric values, not
    // their bit patterns.
    assert_eq!(
        ScanValue::I64(-1).num_cmp(&ScanValue::U64(1)),
        Some(Ordering::Less)
    );
    assert_eq!(
        ScanValue::U64(u64::MAX).num_cmp(&ScanValue::I64(i64::MAX)),
        Some(Ordering::Greater)
    );
}

#[test]
fn float_comparisons_still_work_and_nan_is_unordered() {
    assert_eq!(
        ScanValue::F32(1.5).num_cmp(&ScanValue::F32(2.5)),
        Some(Ordering::Less)
    );
    assert_eq!(ScanValue::F64(f64::NAN).num_cmp(&ScanValue::F64(1.0)), None);
    // Mixed integer/float falls back to f64, which is the only shared ground.
    assert_eq!(
        ScanValue::I32(2).num_cmp(&ScanValue::F32(2.5)),
        Some(Ordering::Less)
    );
}
