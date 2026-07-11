//! Tests for array/structure dissection: stride detection and field inference.

use gamegene_core::mock::MockMemory;
use gamegene_core::structure::{detect_stride_in, dissect, infer_fields, StrideOptions};
use gamegene_core::value::ValueType;

/// Build a synthetic array of `records` records of `stride` bytes. Each record:
///   +0  u32 id            (increments 0,1,2,… — a varying field)
///   +4  u32 tag = 0xABCD  (constant across records)
///   +8  f32 weight        (a real float, e.g. 1.5, 2.5, …)
///   +12 u32 flags = 1     (constant)
///   rest zero-padded
fn synthetic_array(stride: usize, records: usize) -> Vec<u8> {
    assert!(stride >= 16);
    let mut buf = vec![0u8; stride * records];
    for r in 0..records {
        let base = r * stride;
        buf[base..base + 4].copy_from_slice(&(r as u32).to_le_bytes());
        buf[base + 4..base + 8].copy_from_slice(&0xABCDu32.to_le_bytes());
        let weight = 1.5f32 + r as f32;
        buf[base + 8..base + 12].copy_from_slice(&weight.to_le_bytes());
        buf[base + 12..base + 16].copy_from_slice(&1u32.to_le_bytes());
    }
    buf
}

#[test]
fn detects_the_record_size() {
    let buf = synthetic_array(32, 40);
    let stride = detect_stride_in(&buf, StrideOptions::default());
    assert_eq!(stride, Some(32));
}

#[test]
fn detects_a_different_stride() {
    let buf = synthetic_array(48, 30);
    let stride = detect_stride_in(&buf, StrideOptions::default());
    assert_eq!(stride, Some(48));
}

#[test]
fn rejects_non_periodic_memory() {
    // A simple linear ramp has no repeating record structure.
    let buf: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    // The period here (251) is outside a sensible stride range, so with a small
    // max_stride the detector should decline rather than invent a record size.
    let opts = StrideOptions {
        max_stride: 128,
        ..StrideOptions::default()
    };
    assert_eq!(detect_stride_in(&buf, opts), None);
}

#[test]
fn infers_float_and_int_fields() {
    let stride = 32;
    let buf = synthetic_array(stride, 40);
    let fields = infer_fields(&buf, stride, 40);
    // One field per 4-byte word.
    assert_eq!(fields.len(), stride / 4);
    // +8 is the float weight; +0/+4/+12 are ints.
    let at = |off: usize| fields.iter().find(|f| f.offset == off).unwrap().ty;
    assert_eq!(at(8), ValueType::F32);
    assert_eq!(at(0), ValueType::I32);
    assert_eq!(at(4), ValueType::I32);
    assert_eq!(at(12), ValueType::I32);
}

#[test]
fn dissect_reads_from_a_source() {
    let base = 0x2_0000u64;
    let buf = synthetic_array(32, 40);
    let mem = MockMemory::new(base, buf.len());
    mem.poke(base, &buf);

    let d = dissect(&mem, base, StrideOptions::default()).expect("should dissect");
    assert_eq!(d.stride, 32);
    assert_eq!(
        d.fields.iter().find(|f| f.offset == 8).unwrap().ty,
        ValueType::F32
    );
}
