//! Typed numeric values and their byte encodings.
//!
//! Game values live in memory as raw little-endian bytes. [`ValueType`] names a
//! width + interpretation; [`ScanValue`] is one concrete value of such a type.

use crate::error::ScanError;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// The kind of a value the user is scanning for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

impl ValueType {
    /// Every variant, in UI display order.
    pub const ALL: [ValueType; 10] = [
        ValueType::I32,
        ValueType::I64,
        ValueType::F32,
        ValueType::F64,
        ValueType::I8,
        ValueType::I16,
        ValueType::U8,
        ValueType::U16,
        ValueType::U32,
        ValueType::U64,
    ];

    /// Width in bytes.
    pub const fn size(self) -> usize {
        match self {
            ValueType::I8 | ValueType::U8 => 1,
            ValueType::I16 | ValueType::U16 => 2,
            ValueType::I32 | ValueType::U32 | ValueType::F32 => 4,
            ValueType::I64 | ValueType::U64 | ValueType::F64 => 8,
        }
    }

    /// Short human-readable label for the UI.
    pub const fn label(self) -> &'static str {
        match self {
            ValueType::I8 => "Int8",
            ValueType::I16 => "Int16",
            ValueType::I32 => "Int32",
            ValueType::I64 => "Int64",
            ValueType::U8 => "UInt8",
            ValueType::U16 => "UInt16",
            ValueType::U32 => "UInt32",
            ValueType::U64 => "UInt64",
            ValueType::F32 => "Float",
            ValueType::F64 => "Double",
        }
    }
}

/// A concrete value of a given [`ValueType`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ScanValue {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
}

impl ScanValue {
    /// The type tag of this value.
    pub fn value_type(&self) -> ValueType {
        match self {
            ScanValue::I8(_) => ValueType::I8,
            ScanValue::I16(_) => ValueType::I16,
            ScanValue::I32(_) => ValueType::I32,
            ScanValue::I64(_) => ValueType::I64,
            ScanValue::U8(_) => ValueType::U8,
            ScanValue::U16(_) => ValueType::U16,
            ScanValue::U32(_) => ValueType::U32,
            ScanValue::U64(_) => ValueType::U64,
            ScanValue::F32(_) => ValueType::F32,
            ScanValue::F64(_) => ValueType::F64,
        }
    }

    /// Little-endian byte encoding, ready to write into process memory.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            ScanValue::I8(v) => v.to_le_bytes().to_vec(),
            ScanValue::I16(v) => v.to_le_bytes().to_vec(),
            ScanValue::I32(v) => v.to_le_bytes().to_vec(),
            ScanValue::I64(v) => v.to_le_bytes().to_vec(),
            ScanValue::U8(v) => v.to_le_bytes().to_vec(),
            ScanValue::U16(v) => v.to_le_bytes().to_vec(),
            ScanValue::U32(v) => v.to_le_bytes().to_vec(),
            ScanValue::U64(v) => v.to_le_bytes().to_vec(),
            ScanValue::F32(v) => v.to_le_bytes().to_vec(),
            ScanValue::F64(v) => v.to_le_bytes().to_vec(),
        }
    }

    /// Decode a value of `ty` from the front of `buf`.
    ///
    /// `buf` must be at least `ty.size()` bytes; callers guarantee this.
    pub fn from_le_bytes(ty: ValueType, buf: &[u8]) -> ScanValue {
        debug_assert!(buf.len() >= ty.size());
        match ty {
            ValueType::I8 => ScanValue::I8(buf[0] as i8),
            ValueType::U8 => ScanValue::U8(buf[0]),
            ValueType::I16 => ScanValue::I16(i16::from_le_bytes([buf[0], buf[1]])),
            ValueType::U16 => ScanValue::U16(u16::from_le_bytes([buf[0], buf[1]])),
            ValueType::I32 => ScanValue::I32(i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])),
            ValueType::U32 => ScanValue::U32(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])),
            ValueType::F32 => ScanValue::F32(f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])),
            ValueType::I64 => ScanValue::I64(i64::from_le_bytes(
                buf[..8].try_into().expect("8 bytes checked by caller"),
            )),
            ValueType::U64 => ScanValue::U64(u64::from_le_bytes(
                buf[..8].try_into().expect("8 bytes checked by caller"),
            )),
            ValueType::F64 => ScanValue::F64(f64::from_le_bytes(
                buf[..8].try_into().expect("8 bytes checked by caller"),
            )),
        }
    }

    /// Parse user text into a value of `ty`.
    pub fn parse(ty: ValueType, text: &str) -> Result<ScanValue, ScanError> {
        let t = text.trim();
        let err = || ScanError::Parse(format!("`{text}` is not a valid {}", ty.label()));
        Ok(match ty {
            ValueType::I8 => ScanValue::I8(t.parse().map_err(|_| err())?),
            ValueType::I16 => ScanValue::I16(t.parse().map_err(|_| err())?),
            ValueType::I32 => ScanValue::I32(t.parse().map_err(|_| err())?),
            ValueType::I64 => ScanValue::I64(t.parse().map_err(|_| err())?),
            ValueType::U8 => ScanValue::U8(t.parse().map_err(|_| err())?),
            ValueType::U16 => ScanValue::U16(t.parse().map_err(|_| err())?),
            ValueType::U32 => ScanValue::U32(t.parse().map_err(|_| err())?),
            ValueType::U64 => ScanValue::U64(t.parse().map_err(|_| err())?),
            ValueType::F32 => ScanValue::F32(t.parse().map_err(|_| err())?),
            ValueType::F64 => ScanValue::F64(t.parse().map_err(|_| err())?),
        })
    }

    /// Numeric value as `f64`, for ordering and relative comparisons.
    ///
    /// Note: `i64`/`u64` magnitudes beyond 2^53 lose precision here. Exact
    /// equality (used by `Exact`/`Changed`/`Unchanged`) never goes through this
    /// path, so only ordering of very large 64-bit integers is affected.
    pub fn as_f64(&self) -> f64 {
        match self {
            ScanValue::I8(v) => *v as f64,
            ScanValue::I16(v) => *v as f64,
            ScanValue::I32(v) => *v as f64,
            ScanValue::I64(v) => *v as f64,
            ScanValue::U8(v) => *v as f64,
            ScanValue::U16(v) => *v as f64,
            ScanValue::U32(v) => *v as f64,
            ScanValue::U64(v) => *v as f64,
            ScanValue::F32(v) => *v as f64,
            ScanValue::F64(v) => *v,
        }
    }

    /// Ordering against another value, comparing numerically.
    pub fn num_cmp(&self, other: &ScanValue) -> Option<Ordering> {
        self.as_f64().partial_cmp(&other.as_f64())
    }

    /// Display string for the UI / table view.
    pub fn display(&self) -> String {
        match self {
            ScanValue::I8(v) => v.to_string(),
            ScanValue::I16(v) => v.to_string(),
            ScanValue::I32(v) => v.to_string(),
            ScanValue::I64(v) => v.to_string(),
            ScanValue::U8(v) => v.to_string(),
            ScanValue::U16(v) => v.to_string(),
            ScanValue::U32(v) => v.to_string(),
            ScanValue::U64(v) => v.to_string(),
            // Show a decimal point so floats read as floats: `100.0`, not `100`.
            ScanValue::F32(v) => float_str(v.to_string(), v.is_finite()),
            ScanValue::F64(v) => float_str(v.to_string(), v.is_finite()),
        }
    }
}

/// Append `.0` to a whole-number float so it is visibly a float. Each float
/// type formats itself first (no widening, which would expose f32 rounding).
fn float_str(s: String, finite: bool) -> String {
    if finite && !s.contains(['.', 'e', 'E']) {
        format!("{s}.0")
    } else {
        s
    }
}
