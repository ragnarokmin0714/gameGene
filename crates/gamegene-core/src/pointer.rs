//! Pointer scanning: find a stable pointer path to a found address.
//!
//! A raw address is only valid for one game run (ASLR moves everything on
//! restart). A pointer *path* — `[[[module+base] + o1] + …] + oN` — re-derives
//! the address by following pointers from a static module base, so it survives
//! restarts. This is what lets a saved cheat-table entry keep working next time.
//!
//! The scanner collects every pointer-looking value in memory, then searches
//! backward from the target: find a pointer whose value is within `max_offset`
//! below the target, step to where that pointer is *stored*, and repeat until
//! the storage location lands inside a module (a static anchor). Every emitted
//! chain is validated by actually resolving it, so results are never bogus.

use crate::constants::{POINTER_SIZE, SCAN_CHUNK_SIZE};
use crate::process::{MemorySource, ModuleInfo};
use crate::table::Locator;
use std::collections::HashSet;

/// Bounds for a pointer scan. Deeper/wider searches find more paths but cost
/// much more time and memory.
#[derive(Debug, Clone, Copy)]
pub struct PointerScanOptions {
    /// Maximum number of offsets in a chain (pointer hops).
    pub max_depth: usize,
    /// Maximum positive offset added at each hop.
    pub max_offset: u64,
    /// Stop once this many valid chains have been found.
    pub max_results: usize,
}

impl Default for PointerScanOptions {
    fn default() -> Self {
        PointerScanOptions {
            max_depth: 4,
            max_offset: 0x400,
            max_results: 16,
        }
    }
}

/// Find pointer paths that resolve to `target`. Returns validated
/// [`Locator::Pointer`] chains (shortest found first), or empty if the process
/// exposes no modules or no path was found within the given bounds.
pub fn pointer_scan(
    source: &dyn MemorySource,
    target: u64,
    opts: PointerScanOptions,
) -> Vec<Locator> {
    let modules = source.modules();
    if modules.is_empty() {
        return Vec::new();
    }
    let records = collect_pointers(source);
    if records.is_empty() {
        return Vec::new();
    }

    let mut ctx = Ctx {
        records: &records,
        modules: &modules,
        opts,
        source,
        target,
        results: Vec::new(),
        visited: HashSet::new(),
    };
    let mut offsets = Vec::new();
    ctx.search(target, &mut offsets, 0);
    ctx.results
}

struct Ctx<'a> {
    /// `(pointer value, location holding it)`, sorted by value.
    records: &'a [(u64, u64)],
    modules: &'a [ModuleInfo],
    opts: PointerScanOptions,
    source: &'a dyn MemorySource,
    target: u64,
    results: Vec<Locator>,
    visited: HashSet<u64>,
}

impl Ctx<'_> {
    /// Search backward for a chain that reaches `goal`. `offsets` holds the
    /// offsets discovered so far, in base→target order.
    fn search(&mut self, goal: u64, offsets: &mut Vec<i64>, depth: usize) {
        if self.results.len() >= self.opts.max_results || depth >= self.opts.max_depth {
            return;
        }
        // Records whose value V satisfies goal - V in [0, max_offset].
        let lo = goal.saturating_sub(self.opts.max_offset);
        let start = self.records.partition_point(|(v, _)| *v < lo);
        for &(value, location) in &self.records[start..] {
            if value > goal {
                break;
            }
            if self.results.len() >= self.opts.max_results {
                return;
            }
            let off = (goal - value) as i64;
            offsets.insert(0, off);

            if let Some(m) = self.modules.iter().find(|m| m.contains(location)) {
                // Static anchor reached — emit and validate the chain.
                let loc = Locator::Pointer {
                    module: m.name.clone(),
                    base_offset: location as i64 - m.base as i64,
                    offsets: offsets.clone(),
                };
                if loc.resolve(self.source) == Some(self.target) {
                    self.results.push(loc);
                }
            } else if self.visited.insert(location) {
                // Follow the chain: we now need to reach `location`.
                self.search(location, offsets, depth + 1);
                self.visited.remove(&location);
            }

            offsets.remove(0);
        }
    }
}

/// Collect every 8-byte-aligned value that looks like a pointer into mapped
/// memory, as `(value, location)`, sorted by value for range queries.
fn collect_pointers(source: &dyn MemorySource) -> Vec<(u64, u64)> {
    let regions = source.regions();
    let mut ranges: Vec<(u64, u64)> = regions
        .iter()
        .map(|r| (r.base, r.base.saturating_add(r.size)))
        .collect();
    ranges.sort_by_key(|(b, _)| *b);
    let is_mapped = |addr: u64| {
        let idx = ranges.partition_point(|(b, _)| *b <= addr);
        idx > 0 && addr < ranges[idx - 1].1
    };

    let mut records = Vec::new();
    let mut buf = vec![0u8; SCAN_CHUNK_SIZE];
    for region in &regions {
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
            if got < POINTER_SIZE {
                if got == 0 {
                    offset += want as u64;
                    continue;
                }
                break;
            }
            let mut i = 0;
            while i + POINTER_SIZE <= got {
                let value = u64::from_le_bytes(
                    buf[i..i + POINTER_SIZE]
                        .try_into()
                        .expect("POINTER_SIZE bytes"),
                );
                if is_mapped(value) {
                    records.push((value, read_addr + i as u64));
                }
                i += POINTER_SIZE;
            }
            offset += got as u64;
        }
    }
    records.sort_by_key(|(v, _)| *v);
    records
}
