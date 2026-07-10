//! The scan engine: first scan, then iteratively narrow.
//!
//! The workflow mirrors Cheat Engine: a first scan collects every address whose
//! value matches a predicate; each subsequent scan re-reads those addresses and
//! keeps only the ones still matching. Repeatedly changing the value in-game and
//! re-scanning collapses thousands of candidates down to the one you want.
//!
//! Two internal representations keep this efficient on multi-GB targets:
//! - a **candidate list** of explicit `(address, last value)` pairs, used after
//!   a known-value scan or any narrowing step;
//! - a **snapshot** of raw region bytes, used for an "unknown initial value"
//!   scan where every address is a candidate — storing bytes (1x memory) rather
//!   than a struct per address (which would blow up to many GB).
//!
//! Re-scanning a candidate list coalesces nearby addresses into block reads, so
//! a dense next-scan costs a handful of syscalls instead of one per address.

use crate::constants::{MAX_RESULTS_DISPLAY, NEXT_SCAN_BLOCK, SCAN_CHUNK_SIZE};
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

/// A contiguous run of raw bytes captured during an "unknown value" scan.
#[derive(Debug)]
struct Snapshot {
    base: u64,
    bytes: Vec<u8>,
}

/// How a session stores its surviving candidates.
#[derive(Debug)]
enum Results {
    /// Raw captured bytes; every aligned slot is an implicit candidate.
    Snapshot(Vec<Snapshot>),
    /// Explicit candidates, kept sorted by address.
    List(Vec<Match>),
}

/// A live scan: the chosen type plus the current candidate set.
#[derive(Debug)]
pub struct ScanSession {
    value_type: ValueType,
    results: Results,
    scan_count: u32,
}

impl ScanSession {
    /// The value type this session scans for.
    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    /// Number of candidates still alive.
    pub fn len(&self) -> usize {
        match &self.results {
            Results::List(v) => v.len(),
            Results::Snapshot(snaps) => {
                let size = self.value_type.size();
                snaps.iter().map(|s| slot_count(s.bytes.len(), size)).sum()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many scans (first + next) have run.
    pub fn scan_count(&self) -> u32 {
        self.scan_count
    }

    /// Materialize up to `MAX_RESULTS_DISPLAY` candidates for the UI.
    pub fn display_matches(&self) -> Vec<Match> {
        self.iter_matches().take(MAX_RESULTS_DISPLAY).collect()
    }

    /// Materialize every candidate. Cheap for a list; for a snapshot this
    /// expands every slot, so only call it on bounded data (e.g. in tests).
    pub fn matches(&self) -> Vec<Match> {
        self.iter_matches().collect()
    }

    fn iter_matches(&self) -> Box<dyn Iterator<Item = Match> + '_> {
        match &self.results {
            Results::List(v) => Box::new(v.iter().copied()),
            Results::Snapshot(snaps) => {
                let vt = self.value_type;
                let size = vt.size();
                Box::new(snaps.iter().flat_map(move |s| {
                    (0..slot_count(s.bytes.len(), size)).map(move |k| {
                        let i = k * size;
                        Match {
                            address: s.base + i as u64,
                            previous: ScanValue::from_le_bytes(vt, &s.bytes[i..]),
                        }
                    })
                }))
            }
        }
    }

    /// Run the first scan over every readable region of `source`.
    ///
    /// Relative predicates (`Changed`, `Increased`, …) are rejected here since
    /// there is no previous scan to compare against. `Unknown` captures a byte
    /// snapshot; every other predicate produces an explicit candidate list.
    pub fn first_scan(
        source: &dyn MemorySource,
        value_type: ValueType,
        compare: Compare,
    ) -> Result<ScanSession, ScanError> {
        if let Some(name) = compare.needs_previous() {
            return Err(ScanError::NeedsPrevious(name));
        }

        let results = if compare == Compare::Unknown {
            Results::Snapshot(snapshot_regions(source, value_type))
        } else {
            Results::List(scan_regions(source, value_type, compare))
        };

        Ok(ScanSession {
            value_type,
            results,
            scan_count: 1,
        })
    }

    /// Re-read the current candidates and keep those matching `compare`.
    ///
    /// A snapshot is compared slot-by-slot against the captured bytes and
    /// collapses into a candidate list. A candidate list is re-read with
    /// block-coalesced I/O. Either way the stored `previous` value is refreshed
    /// so the next relative scan compares against this scan.
    pub fn next_scan(
        &mut self,
        source: &dyn MemorySource,
        compare: Compare,
    ) -> Result<(), ScanError> {
        let vt = self.value_type;
        let survivors = match &mut self.results {
            Results::List(matches) => rescan_list(source, vt, compare, matches),
            Results::Snapshot(snaps) => rescan_snapshot(source, vt, compare, snaps),
        };
        self.results = Results::List(survivors);
        self.scan_count += 1;
        Ok(())
    }
}

/// Number of aligned `size`-byte slots that fit in `len` bytes.
fn slot_count(len: usize, size: usize) -> usize {
    if len >= size {
        (len - size) / size + 1
    } else {
        0
    }
}

/// First scan for a known-value predicate: collect matching aligned slots.
fn scan_regions(source: &dyn MemorySource, vt: ValueType, compare: Compare) -> Vec<Match> {
    let size = vt.size();
    let mut matches = Vec::new();
    let mut buf = vec![0u8; SCAN_CHUNK_SIZE];

    for region in source.regions() {
        let mut offset = 0u64;
        while offset < region.size {
            let want = ((region.size - offset) as usize).min(SCAN_CHUNK_SIZE);
            let read_addr = region.base + offset;
            let got = match source.read(read_addr, &mut buf[..want]) {
                Ok(n) => n,
                Err(_) => {
                    offset += want as u64;
                    continue;
                }
            };
            if got < size {
                if got == 0 {
                    offset += want as u64;
                    continue;
                }
                break;
            }
            let mut i = 0;
            while i + size <= got {
                let current = ScanValue::from_le_bytes(vt, &buf[i..]);
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
    matches
}

/// First scan for `Unknown`: capture readable regions as contiguous byte runs.
/// A run is broken wherever a read fails, so every stored byte maps linearly to
/// its address (`base + offset`).
fn snapshot_regions(source: &dyn MemorySource, vt: ValueType) -> Vec<Snapshot> {
    let size = vt.size();
    let mut snaps = Vec::new();
    let mut buf = vec![0u8; SCAN_CHUNK_SIZE];

    for region in source.regions() {
        let mut offset = 0u64;
        let mut run_base = region.base;
        let mut run: Vec<u8> = Vec::new();

        while offset < region.size {
            let want = ((region.size - offset) as usize).min(SCAN_CHUNK_SIZE);
            match source.read(region.base + offset, &mut buf[..want]) {
                Ok(got) if got > 0 => {
                    if run.is_empty() {
                        run_base = region.base + offset;
                    }
                    run.extend_from_slice(&buf[..got]);
                    offset += got as u64;
                    if got < want {
                        // Gap ahead: end this run; a new one starts after it.
                        finalize_run(&mut snaps, &mut run, run_base, size);
                    }
                }
                _ => {
                    finalize_run(&mut snaps, &mut run, run_base, size);
                    offset += want as u64;
                }
            }
        }
        finalize_run(&mut snaps, &mut run, run_base, size);
    }
    snaps
}

fn finalize_run(snaps: &mut Vec<Snapshot>, run: &mut Vec<u8>, base: u64, size: usize) {
    if run.len() >= size {
        snaps.push(Snapshot {
            base,
            bytes: std::mem::take(run),
        });
    } else {
        run.clear();
    }
}

/// Re-scan an explicit candidate list, coalescing nearby addresses into single
/// block reads (candidates are sorted first to make coalescing possible).
fn rescan_list(
    source: &dyn MemorySource,
    vt: ValueType,
    compare: Compare,
    matches: &mut [Match],
) -> Vec<Match> {
    let size = vt.size();
    matches.sort_by_key(|m| m.address);

    let mut out = Vec::new();
    let mut buf = vec![0u8; NEXT_SCAN_BLOCK + size];
    let n = matches.len();
    let mut i = 0;
    while i < n {
        let start = matches[i].address;
        // Grow the block while the far end stays within the window.
        let mut j = i;
        while j + 1 < n
            && (matches[j + 1].address + size as u64).saturating_sub(start)
                <= NEXT_SCAN_BLOCK as u64
        {
            j += 1;
        }
        let block_len = (matches[j].address + size as u64 - start) as usize;
        if buf.len() < block_len {
            buf.resize(block_len, 0);
        }
        let got = source.read(start, &mut buf[..block_len]).unwrap_or(0);

        for m in &matches[i..=j] {
            let off = (m.address - start) as usize;
            if off + size > got {
                continue; // fell in an unreadable part of the block
            }
            let current = ScanValue::from_le_bytes(vt, &buf[off..]);
            if compare.eval(current, Some(m.previous)) {
                out.push(Match {
                    address: m.address,
                    previous: current,
                });
            }
        }
        i = j + 1;
    }
    out
}

/// Re-scan a byte snapshot: read current memory for each captured run and
/// compare slot-by-slot against the snapshot, collapsing into a candidate list.
fn rescan_snapshot(
    source: &dyn MemorySource,
    vt: ValueType,
    compare: Compare,
    snaps: &[Snapshot],
) -> Vec<Match> {
    let size = vt.size();
    let mut out = Vec::new();

    for snap in snaps {
        let mut cur = vec![0u8; snap.bytes.len()];
        let valid = read_contiguous(source, snap.base, &mut cur);
        for k in 0..slot_count(valid, size) {
            let i = k * size;
            let current = ScanValue::from_le_bytes(vt, &cur[i..]);
            let previous = ScanValue::from_le_bytes(vt, &snap.bytes[i..]);
            if compare.eval(current, Some(previous)) {
                out.push(Match {
                    address: snap.base + i as u64,
                    previous: current,
                });
            }
        }
    }
    out.sort_by_key(|m| m.address);
    out
}

/// Read from `base` into `buf`, returning how many contiguous bytes from the
/// start were read (stops at the first short read or error).
fn read_contiguous(source: &dyn MemorySource, base: u64, buf: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buf.len() {
        let want = (buf.len() - filled).min(SCAN_CHUNK_SIZE);
        match source.read(base + filled as u64, &mut buf[filled..filled + want]) {
            Ok(got) if got > 0 => {
                filled += got;
                if got < want {
                    break;
                }
            }
            _ => break,
        }
    }
    filled
}
