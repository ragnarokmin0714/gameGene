//! Array / structure dissection.
//!
//! Given the address of one record in a repeating array (an inventory slot, a
//! move-table entry, …), detect the record size ("stride") and infer each
//! field's type, so the whole array can be laid out as rows for editing.
//!
//! Stride detection is a **heuristic**: it treats the memory window as a signal
//! and looks for its period. For a repeating struct, bytes at a distance equal
//! to the record size line up (constant tags, padding, aligned fields), so the
//! byte-equality ratio at that shift is a sharp local peak. We pick the shift
//! whose peak stands out most from its neighbours — an off-by-one shift breaks
//! the alignment, so the true period is peaky while multiples are not. This is
//! a starting guess, not ground truth; the stride is always user-adjustable.

use crate::process::MemorySource;
use crate::value::ValueType;

/// Tunables for [`detect_stride_in`] / [`dissect`].
#[derive(Clone, Copy)]
pub struct StrideOptions {
    pub min_stride: usize,
    pub max_stride: usize,
    /// Bytes to read from the base address to analyse.
    pub window: usize,
    /// Require at least this many whole records to trust a stride.
    pub min_records: usize,
}

impl Default for StrideOptions {
    fn default() -> Self {
        StrideOptions {
            min_stride: 4,
            max_stride: 512,
            window: 8192,
            min_records: 6,
        }
    }
}

/// A minimum peak sharpness and byte-repeat ratio for a stride to be accepted;
/// below these the memory doesn't look convincingly periodic.
const MIN_PEAK: f64 = 0.03;
const MIN_RATIO: f64 = 0.12;

/// One field within a record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub offset: usize,
    pub ty: ValueType,
}

/// The result of dissecting an array: its record size and inferred fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Dissection {
    pub stride: usize,
    pub fields: Vec<Field>,
}

/// Byte-equality ratio of `buf` against itself shifted by `shift`.
fn repeat_ratio(buf: &[u8], shift: usize) -> f64 {
    let n = buf.len().saturating_sub(shift);
    if n == 0 {
        return 0.0;
    }
    let eq = (0..n).filter(|&i| buf[i] == buf[i + shift]).count();
    eq as f64 / n as f64
}

/// Detect the most likely record size in a raw memory window, or `None` if the
/// window doesn't look periodic. Pure and deterministic, so it is unit-tested
/// directly on byte buffers.
pub fn detect_stride_in(buf: &[u8], opts: StrideOptions) -> Option<usize> {
    let len = buf.len();
    // Largest stride that still leaves min_records whole records to compare.
    let hi = opts
        .max_stride
        .min(len / opts.min_records.max(1))
        .min(len.saturating_sub(1));
    if hi < opts.min_stride + 1 {
        return None;
    }

    // Precompute ratios up to hi+1 so each candidate has both neighbours.
    let ratios: Vec<f64> = (0..=hi + 1).map(|s| repeat_ratio(buf, s)).collect();

    let mut best: Option<(f64, f64, usize)> = None; // (peak, ratio, stride)
    for s in opts.min_stride..=hi {
        let peak = ratios[s] - 0.5 * (ratios[s - 1] + ratios[s + 1]);
        if peak <= 0.0 {
            continue;
        }
        // Strictly-greater keeps the smallest stride on ties (ascending scan).
        if best.is_none_or(|(bp, _, _)| peak > bp + 1e-9) {
            best = Some((peak, ratios[s], s));
        }
    }

    best.filter(|&(peak, ratio, _)| peak >= MIN_PEAK && ratio >= MIN_RATIO)
        .map(|(_, _, s)| s)
}

/// Do the `records` 4-byte words at this record offset look like floats?
fn looks_float(buf: &[u8], offset: usize, stride: usize, records: usize) -> bool {
    let mut floaty = 0usize;
    let mut total = 0usize;
    for r in 0..records {
        let at = r * stride + offset;
        let Some(bytes) = buf.get(at..at + 4) else {
            break;
        };
        let bits = u32::from_le_bytes(bytes.try_into().unwrap());
        if bits == 0 {
            continue; // zero reads as 0.0 for both int and float — ignore
        }
        total += 1;
        let f = f32::from_bits(bits);
        // A "real" float field: finite, sane magnitude, and not a whole number
        // (whole numbers are more likely small integers stored as ints).
        if f.is_finite() && (1e-3..=1e9).contains(&f.abs()) && f.fract() != 0.0 {
            floaty += 1;
        }
    }
    total > 0 && floaty * 2 >= total
}

/// Infer a field per aligned 4-byte word of the record. Words whose values
/// across records look like floats are typed `F32`, the rest `I32`.
pub fn infer_fields(buf: &[u8], stride: usize, records: usize) -> Vec<Field> {
    let mut fields = Vec::new();
    let mut offset = 0;
    while offset + 4 <= stride {
        let ty = if looks_float(buf, offset, stride, records) {
            ValueType::F32
        } else {
            ValueType::I32
        };
        fields.push(Field { offset, ty });
        offset += 4;
    }
    fields
}

/// Read a window at `base`, detect the stride, and infer the record's fields.
pub fn dissect(src: &dyn MemorySource, base: u64, opts: StrideOptions) -> Option<Dissection> {
    let mut buf = vec![0u8; opts.window];
    let got = src.read(base, &mut buf).ok()?;
    buf.truncate(got);
    let stride = detect_stride_in(&buf, opts)?;
    let records = buf.len() / stride;
    let fields = infer_fields(&buf, stride, records);
    Some(Dissection { stride, fields })
}
