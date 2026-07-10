//! The scan engine: first scan, then iteratively narrow.
//!
//! The workflow mirrors Cheat Engine: a first scan collects every address whose
//! value matches a predicate; each subsequent scan re-reads those addresses and
//! keeps only the ones still matching. Repeatedly changing the value in-game and
//! re-scanning collapses thousands of candidates down to the one you want.

use crate::constants::{MAX_RESULTS_DISPLAY, SCAN_CHUNK_SIZE};
use crate::error::ScanError;
use crate::process::MemorySource;
use crate::value::{ScanValue, ValueType};
use std::cmp::Ordering;

/// A comparison predicate applied to each candidate address.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Compare {
    /// Equal to a specific value.
    Exact(ScanValue),
    /// Strictly greater than a value.
    GreaterThan(ScanValue),
    /// Strictly less than a value.
    LessThan(ScanValue),
    /// Within an inclusive `[lo, hi]` range.
    Between(ScanValue, ScanValue),
    /// Keep everything (used to snapshot an "unknown initial value").
    Unknown,
    /// Changed since the previous scan.
    Changed,
    /// Unchanged since the previous scan.
    Unchanged,
    /// Increased since the previous scan.
    Increased,
    /// Decreased since the previous scan.
    Decreased,
}

impl Compare {
    /// Whether this predicate references the previous scan's value and so
    /// cannot be used for a first scan.
    fn needs_previous(&self) -> Option<&'static str> {
        match self {
            Compare::Changed => Some("changed"),
            Compare::Unchanged => Some("unchanged"),
            Compare::Increased => Some("increased"),
            Compare::Decreased => Some("decreased"),
            _ => None,
        }
    }

    fn eval(&self, current: ScanValue, previous: Option<ScanValue>) -> bool {
        match self {
            Compare::Unknown => true,
            Compare::Exact(t) => current == *t,
            Compare::GreaterThan(t) => current.num_cmp(t) == Some(Ordering::Greater),
            Compare::LessThan(t) => current.num_cmp(t) == Some(Ordering::Less),
            Compare::Between(lo, hi) => {
                !matches!(current.num_cmp(lo), Some(Ordering::Less) | None)
                    && !matches!(current.num_cmp(hi), Some(Ordering::Greater) | None)
            }
            Compare::Changed => previous.is_some_and(|p| current != p),
            Compare::Unchanged => previous.is_some_and(|p| current == p),
            Compare::Increased => {
                previous.and_then(|p| current.num_cmp(&p)) == Some(Ordering::Greater)
            }
            Compare::Decreased => {
                previous.and_then(|p| current.num_cmp(&p)) == Some(Ordering::Less)
            }
        }
    }
}

/// One surviving candidate address and the value it held at the last scan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    pub address: u64,
    pub previous: ScanValue,
}

/// A live scan: the chosen type plus the current candidate set.
#[derive(Debug)]
pub struct ScanSession {
    value_type: ValueType,
    matches: Vec<Match>,
    scan_count: u32,
}

impl ScanSession {
    /// The value type this session scans for.
    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    /// Current surviving candidates.
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// Number of candidates still alive.
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// How many scans (first + next) have run.
    pub fn scan_count(&self) -> u32 {
        self.scan_count
    }

    /// A capped slice for display, so a huge candidate set can't stall the UI.
    pub fn display_matches(&self) -> &[Match] {
        let n = self.matches.len().min(MAX_RESULTS_DISPLAY);
        &self.matches[..n]
    }

    /// Run the first scan over every readable region of `source`.
    ///
    /// Relative predicates (`Changed`, `Increased`, …) are rejected here since
    /// there is no previous scan to compare against.
    pub fn first_scan(
        source: &dyn MemorySource,
        value_type: ValueType,
        compare: Compare,
    ) -> Result<ScanSession, ScanError> {
        if let Some(name) = compare.needs_previous() {
            return Err(ScanError::NeedsPrevious(name));
        }

        let size = value_type.size();
        let mut matches = Vec::new();
        let mut buf = vec![0u8; SCAN_CHUNK_SIZE];

        for region in source.regions() {
            let mut offset = 0u64;
            while offset < region.size {
                let want = ((region.size - offset) as usize).min(SCAN_CHUNK_SIZE);
                let read_addr = region.base + offset;
                let got = match source.read(read_addr, &mut buf[..want]) {
                    Ok(n) => n,
                    // Unreadable page: skip this chunk, keep going.
                    Err(_) => {
                        offset += want as u64;
                        continue;
                    }
                };
                if got < size {
                    break;
                }
                // Aligned scan: step by the value's width. This matches Cheat
                // Engine's default "fast scan" and is what real values use.
                let mut i = 0;
                while i + size <= got {
                    let current = ScanValue::from_le_bytes(value_type, &buf[i..]);
                    if compare.eval(current, None) {
                        matches.push(Match {
                            address: read_addr + i as u64,
                            previous: current,
                        });
                    }
                    i += size;
                }
                offset += got as u64;
            }
        }

        Ok(ScanSession {
            value_type,
            matches,
            scan_count: 1,
        })
    }

    /// Re-read every surviving candidate and keep those matching `compare`.
    ///
    /// The stored `previous` value is refreshed to the freshly read value, so
    /// the next relative scan compares against this scan, not the first one.
    pub fn next_scan(
        &mut self,
        source: &dyn MemorySource,
        compare: Compare,
    ) -> Result<(), ScanError> {
        let size = self.value_type.size();
        let mut buf = [0u8; 8];

        self.matches.retain_mut(|m| {
            let n = match source.read(m.address, &mut buf[..size]) {
                Ok(n) => n,
                Err(_) => return false,
            };
            if n < size {
                return false;
            }
            let current = ScanValue::from_le_bytes(self.value_type, &buf);
            if compare.eval(current, Some(m.previous)) {
                m.previous = current;
                true
            } else {
                false
            }
        });

        self.scan_count += 1;
        Ok(())
    }
}
