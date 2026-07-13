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

use crate::constants::{
    MAX_RESULTS_DISPLAY, MAX_SCAN_MATCHES, NEXT_SCAN_BLOCK, SCAN_CHUNK_SIZE, SCAN_WORK_ITEM,
};
use crate::error::ScanError;
use crate::process::MemorySource;
use crate::value::{ScanValue, ValueType};
use std::cmp::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};

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

    pub(crate) fn eval(&self, current: ScanValue, previous: Option<ScanValue>) -> bool {
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

/// Cooperative progress + cancellation for a running scan.
///
/// The engine reports bytes scanned and checks [`is_cancelled`](Self::is_cancelled)
/// at chunk boundaries, so a long scan on a background thread can drive a
/// progress bar and be stopped without waiting for it to finish. All counters
/// are atomic, so the same control can be read from the UI thread while worker
/// threads update it.
#[derive(Debug, Default)]
pub struct ScanControl {
    cancel: AtomicBool,
    scanned: AtomicU64,
    total: AtomicU64,
}

impl ScanControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the scan to stop as soon as it notices (at the next chunk).
    pub fn request_cancel(&self) {
        self.cancel.store(true, AtomicOrdering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(AtomicOrdering::Relaxed)
    }

    /// Set the total number of bytes the scan will cover (for the progress bar).
    pub fn set_total(&self, total: u64) {
        self.total.store(total, AtomicOrdering::Relaxed);
    }

    fn add_scanned(&self, n: u64) {
        self.scanned.fetch_add(n, AtomicOrdering::Relaxed);
    }

    /// `(bytes_scanned, bytes_total)` — total is 0 until [`set_total`](Self::set_total).
    pub fn progress(&self) -> (u64, u64) {
        (
            self.scanned.load(AtomicOrdering::Relaxed),
            self.total.load(AtomicOrdering::Relaxed),
        )
    }
}

/// One surviving candidate address and the value it held at the last scan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    pub address: u64,
    pub previous: ScanValue,
}

/// A primitive the scanner compares natively. Comparing raw typed values and
/// building a [`ScanValue`] only for the (rare) matches skips the per-slot enum
/// construction that dominated the old hot loop — and compares integers at full
/// width instead of via `f64`, so 64-bit values past 2^53 order correctly.
trait Prim: Copy + PartialOrd {
    const SIZE: usize;
    /// Decode from the front of `b` (`b.len() >= SIZE`, guaranteed by caller).
    fn from_le(b: &[u8]) -> Self;
    /// Extract from a [`ScanValue`] known to hold this exact type.
    fn of(v: ScanValue) -> Self;
}

macro_rules! impl_prim {
    ($t:ty, $variant:ident) => {
        impl Prim for $t {
            const SIZE: usize = std::mem::size_of::<$t>();
            fn from_le(b: &[u8]) -> Self {
                <$t>::from_le_bytes(b[..Self::SIZE].try_into().unwrap())
            }
            fn of(v: ScanValue) -> Self {
                match v {
                    ScanValue::$variant(x) => x,
                    _ => unreachable!("group/scan built a predicate of the wrong type"),
                }
            }
        }
    };
}
impl_prim!(i8, I8);
impl_prim!(i16, I16);
impl_prim!(i32, I32);
impl_prim!(i64, I64);
impl_prim!(u8, U8);
impl_prim!(u16, U16);
impl_prim!(u32, U32);
impl_prim!(u64, U64);
impl_prim!(f32, F32);
impl_prim!(f64, F64);

/// A [`Compare`] lowered to one native primitive type. Ordering uses the type's
/// own `PartialOrd`, so NaN/None simply doesn't match — same as the old
/// `num_cmp` path.
enum NativePred<T> {
    Exact(T),
    Gt(T),
    Lt(T),
    Between(T, T),
    Any,
    Changed,
    Unchanged,
    Increased,
    Decreased,
}

impl<T: Copy + PartialOrd> NativePred<T> {
    fn from_compare(cmp: Compare) -> Self
    where
        T: Prim,
    {
        match cmp {
            Compare::Exact(v) => NativePred::Exact(T::of(v)),
            Compare::GreaterThan(v) => NativePred::Gt(T::of(v)),
            Compare::LessThan(v) => NativePred::Lt(T::of(v)),
            Compare::Between(lo, hi) => NativePred::Between(T::of(lo), T::of(hi)),
            Compare::Unknown => NativePred::Any,
            Compare::Changed => NativePred::Changed,
            Compare::Unchanged => NativePred::Unchanged,
            Compare::Increased => NativePred::Increased,
            Compare::Decreased => NativePred::Decreased,
        }
    }

    #[inline]
    fn hit(&self, c: T, p: Option<T>) -> bool {
        match self {
            NativePred::Exact(t) => c == *t,
            NativePred::Gt(t) => c > *t,
            NativePred::Lt(t) => c < *t,
            NativePred::Between(lo, hi) => c >= *lo && c <= *hi,
            NativePred::Any => true,
            NativePred::Changed => p.is_some_and(|p| c != p),
            NativePred::Unchanged => p.is_some_and(|p| c == p),
            NativePred::Increased => p.is_some_and(|p| c > p),
            NativePred::Decreased => p.is_some_and(|p| c < p),
        }
    }
}

/// Walk aligned slots of `cur` (optionally paired with `prev` for relative
/// predicates), calling `emit(byte_offset)` for each match. Monomorphized per
/// primitive, so the comparison is a couple of native instructions per slot.
fn scan_slots<T: Prim>(
    cur: &[u8],
    prev: Option<&[u8]>,
    pred: &NativePred<T>,
    mut emit: impl FnMut(usize),
) {
    let size = T::SIZE;
    match prev {
        None => {
            let mut i = 0;
            while i + size <= cur.len() {
                if pred.hit(T::from_le(&cur[i..]), None) {
                    emit(i);
                }
                i += size;
            }
        }
        Some(pb) => {
            let n = cur.len().min(pb.len());
            let mut i = 0;
            while i + size <= n {
                if pred.hit(T::from_le(&cur[i..]), Some(T::from_le(&pb[i..]))) {
                    emit(i);
                }
                i += size;
            }
        }
    }
}

/// Dispatch [`scan_slots`] on the runtime value type.
fn for_each_match(
    vt: ValueType,
    cmp: Compare,
    cur: &[u8],
    prev: Option<&[u8]>,
    mut emit: impl FnMut(usize),
) {
    macro_rules! go {
        ($t:ty) => {
            scan_slots::<$t>(cur, prev, &NativePred::<$t>::from_compare(cmp), &mut emit)
        };
    }
    match vt {
        ValueType::I8 => go!(i8),
        ValueType::I16 => go!(i16),
        ValueType::I32 => go!(i32),
        ValueType::I64 => go!(i64),
        ValueType::U8 => go!(u8),
        ValueType::U16 => go!(u16),
        ValueType::U32 => go!(u32),
        ValueType::U64 => go!(u64),
        ValueType::F32 => go!(f32),
        ValueType::F64 => go!(f64),
    }
}

/// Split every readable region into aligned work items no larger than
/// [`SCAN_WORK_ITEM`], so the thread pool balances even across a few huge
/// regions. Each item's start is a whole number of work-item sizes from its
/// region base, keeping every item boundary on an aligned slot.
fn work_items(source: &dyn MemorySource) -> Vec<(u64, u64)> {
    let mut items = Vec::new();
    for region in source.regions() {
        let mut off = 0u64;
        while off < region.size {
            let len = SCAN_WORK_ITEM.min(region.size - off);
            items.push((region.base + off, len));
            off += len;
        }
    }
    items
}

/// Scan every readable slot with an absolute predicate, in parallel across the
/// available cores, calling `emit` for each match to build a per-thread result
/// of type `R`. Honors `control` (progress + cancel) and stops at `cap` total
/// results (setting `*truncated`). Merged results are in no particular order;
/// callers that need ordering sort afterwards.
///
/// When `set_total` is true this sweep's byte count becomes the progress total;
/// a group scan does several sweeps and sets one aggregate total itself, so it
/// passes false to keep the bar advancing across all of them.
fn parallel_collect<R, E>(
    source: &dyn MemorySource,
    control: &ScanControl,
    cap: usize,
    set_total: bool,
    emit: E,
) -> (Vec<R>, bool)
where
    R: Send,
    E: Fn(&[u8], u64, &mut Vec<R>) + Sync,
{
    let items = work_items(source);
    if set_total {
        control.set_total(items.iter().map(|(_, len)| len).sum());
    }

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(items.len().max(1));

    let cursor = AtomicUsize::new(0);
    let count = AtomicUsize::new(0);
    let truncated = AtomicBool::new(false);

    let worker = || {
        let mut local: Vec<R> = Vec::new();
        let mut counted = 0usize; // results already added to the shared tally
        let mut buf = vec![0u8; SCAN_CHUNK_SIZE];
        loop {
            if control.is_cancelled() || truncated.load(AtomicOrdering::Relaxed) {
                break;
            }
            let idx = cursor.fetch_add(1, AtomicOrdering::Relaxed);
            let Some(&(base, len)) = items.get(idx) else {
                break;
            };
            let mut off = 0u64;
            while off < len {
                if control.is_cancelled() {
                    break;
                }
                let want = ((len - off) as usize).min(SCAN_CHUNK_SIZE);
                let read_addr = base + off;
                let got = source.read(read_addr, &mut buf[..want]).unwrap_or(0);
                control.add_scanned(want as u64);
                if got == 0 {
                    off += want as u64;
                    continue;
                }
                emit(&buf[..got], read_addr, &mut local);
                off += got as u64;
            }
            // Publish this item's new results to the shared tally and stop
            // everyone once the cap is reached.
            let delta = local.len() - counted;
            counted = local.len();
            if count.fetch_add(delta, AtomicOrdering::Relaxed) + delta >= cap {
                truncated.store(true, AtomicOrdering::Relaxed);
                break;
            }
        }
        local
    };

    let mut merged: Vec<R> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads).map(|_| s.spawn(worker)).collect();
        for h in handles {
            merged.append(&mut h.join().unwrap_or_default());
        }
    });

    let truncated = truncated.load(AtomicOrdering::Relaxed) || merged.len() >= cap;
    if merged.len() > cap {
        merged.truncate(cap);
    }
    (merged, truncated)
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
    /// Whether the last scan stopped at [`MAX_SCAN_MATCHES`] before covering
    /// everything — the value was too common; narrow with a more specific one.
    truncated: bool,
}

impl ScanSession {
    /// The value type this session scans for.
    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    /// Whether the last scan hit the candidate cap and stopped early.
    pub fn truncated(&self) -> bool {
        self.truncated
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
        Self::first_scan_with(source, value_type, compare, &ScanControl::new())
    }

    /// [`first_scan`](Self::first_scan) with progress + cancellation, for
    /// running on a background thread.
    pub fn first_scan_with(
        source: &dyn MemorySource,
        value_type: ValueType,
        compare: Compare,
        control: &ScanControl,
    ) -> Result<ScanSession, ScanError> {
        if let Some(name) = compare.needs_previous() {
            return Err(ScanError::NeedsPrevious(name));
        }

        let (results, truncated) = if compare == Compare::Unknown {
            (Results::Snapshot(snapshot_regions(source, control)), false)
        } else {
            let (mut matches, truncated) =
                parallel_collect(source, control, MAX_SCAN_MATCHES, true, |buf, addr, out| {
                    for_each_match(value_type, compare, buf, None, |i| {
                        out.push(Match {
                            address: addr + i as u64,
                            previous: ScanValue::from_le_bytes(value_type, &buf[i..]),
                        });
                    });
                });
            matches.sort_unstable_by_key(|m| m.address);
            (Results::List(matches), truncated)
        };

        Ok(ScanSession {
            value_type,
            results,
            scan_count: 1,
            truncated,
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
        self.next_scan_with(source, compare, &ScanControl::new())
    }

    /// [`next_scan`](Self::next_scan) with progress + cancellation.
    pub fn next_scan_with(
        &mut self,
        source: &dyn MemorySource,
        compare: Compare,
        control: &ScanControl,
    ) -> Result<(), ScanError> {
        let vt = self.value_type;
        let survivors = match &mut self.results {
            Results::List(matches) => rescan_list(source, vt, compare, matches, control),
            Results::Snapshot(snaps) => rescan_snapshot(source, vt, compare, snaps, control),
        };
        self.results = Results::List(survivors);
        self.scan_count += 1;
        // Narrowing an existing candidate set only ever shrinks it, so it can
        // never exceed the cap; clear any earlier truncation.
        self.truncated = false;
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

/// Collect up to `cap` addresses of aligned slots matching `compare` — the
/// per-value search behind the group scan. Runs the same parallel, specialized
/// walk as the first scan, capped because a loose predicate (a float range) can
/// match millions of slots.
/// Collect up to `cap` addresses of aligned slots matching `compare`, honoring
/// a [`ScanControl`] (for a cancellable group scan on a background thread).
/// `set_total` controls whether this sweep owns the progress total — false when
/// a group scan aggregates the total across several sweeps.
pub(crate) fn collect_addresses_with(
    source: &dyn MemorySource,
    vt: ValueType,
    compare: Compare,
    cap: usize,
    control: &ScanControl,
    set_total: bool,
) -> Vec<u64> {
    let (mut addrs, _) = parallel_collect(source, control, cap, set_total, |buf, addr, out| {
        for_each_match(vt, compare, buf, None, |i| out.push(addr + i as u64));
    });
    addrs.sort_unstable();
    addrs
}

/// First scan for `Unknown`: capture readable regions as contiguous byte runs.
/// A run is broken wherever a read fails, so every stored byte maps linearly to
/// its address (`base + offset`). Serial: runs must stay contiguous and this is
/// memory-bandwidth bound, not compute bound.
fn snapshot_regions(source: &dyn MemorySource, control: &ScanControl) -> Vec<Snapshot> {
    let regions = source.regions();
    control.set_total(regions.iter().map(|r| r.size).sum());
    let mut snaps = Vec::new();
    let mut buf = vec![0u8; SCAN_CHUNK_SIZE];

    for region in regions {
        if control.is_cancelled() {
            break;
        }
        let mut offset = 0u64;
        let mut run_base = region.base;
        let mut run: Vec<u8> = Vec::new();

        while offset < region.size {
            if control.is_cancelled() {
                break;
            }
            let want = ((region.size - offset) as usize).min(SCAN_CHUNK_SIZE);
            let outcome = source.read(region.base + offset, &mut buf[..want]);
            control.add_scanned(want as u64);
            match outcome {
                Ok(got) if got > 0 => {
                    if run.is_empty() {
                        run_base = region.base + offset;
                    }
                    run.extend_from_slice(&buf[..got]);
                    offset += got as u64;
                    if got < want {
                        // Gap ahead: end this run; a new one starts after it.
                        finalize_run(&mut snaps, &mut run, run_base);
                    }
                }
                _ => {
                    finalize_run(&mut snaps, &mut run, run_base);
                    offset += want as u64;
                }
            }
        }
        finalize_run(&mut snaps, &mut run, run_base);
    }
    snaps
}

fn finalize_run(snaps: &mut Vec<Snapshot>, run: &mut Vec<u8>, base: u64) {
    if !run.is_empty() {
        snaps.push(Snapshot {
            base,
            bytes: std::mem::take(run),
        });
    }
}

/// Re-scan an explicit candidate list, coalescing nearby addresses into single
/// block reads (candidates are sorted first to make coalescing possible).
/// Serial: candidate lists are already narrowed and small, and the win here is
/// block-coalesced I/O, not compute.
fn rescan_list(
    source: &dyn MemorySource,
    vt: ValueType,
    compare: Compare,
    matches: &mut [Match],
    control: &ScanControl,
) -> Vec<Match> {
    let size = vt.size();
    matches.sort_by_key(|m| m.address);
    control.set_total(matches.len() as u64);

    let mut out = Vec::new();
    let mut buf = vec![0u8; NEXT_SCAN_BLOCK + size];
    let n = matches.len();
    let mut i = 0;
    while i < n {
        if control.is_cancelled() {
            break;
        }
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
        control.add_scanned((j - i + 1) as u64);
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
    control: &ScanControl,
) -> Vec<Match> {
    control.set_total(snaps.iter().map(|s| s.bytes.len() as u64).sum());
    let mut out = Vec::new();
    let mut cur = Vec::new();

    for snap in snaps {
        if control.is_cancelled() {
            break;
        }
        cur.clear();
        cur.resize(snap.bytes.len(), 0);
        let valid = read_contiguous(source, snap.base, &mut cur);
        for_each_match(vt, compare, &cur[..valid], Some(&snap.bytes), |i| {
            out.push(Match {
                address: snap.base + i as u64,
                previous: ScanValue::from_le_bytes(vt, &cur[i..]),
            });
        });
        control.add_scanned(snap.bytes.len() as u64);
    }
    out.sort_unstable_by_key(|m| m.address);
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
