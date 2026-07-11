//! Group / multi-value scan.
//!
//! Cheat Engine calls this a "group" or "commonality" scan: instead of one
//! value, you give several (e.g. an item id, a count, and a level) and it finds
//! places where they all sit close together — a good way to locate a struct
//! when you know a few of its fields.
//!
//! Approach: search each value independently (reusing [`find_pattern`]), then
//! keep the addresses of the first value that have every other value within
//! `span` bytes (in either direction).

use crate::find::find_pattern;
use crate::process::MemorySource;
use crate::value::ScanValue;

/// How many matches to gather per value before correlating. Values in a group
/// scan are meant to be fairly specific; very common values (0, 1) can exceed
/// this and then only the first occurrences are considered.
const PER_VALUE_CAP: usize = 200_000;

/// Find up to `max_results` addresses of `values[0]` that have every other
/// value within `span` bytes. With a single value this is just a plain search.
pub fn group_scan(
    src: &dyn MemorySource,
    values: &[ScanValue],
    span: u64,
    max_results: usize,
) -> Vec<u64> {
    if values.is_empty() || max_results == 0 {
        return Vec::new();
    }

    let mut lists: Vec<Vec<u64>> = values
        .iter()
        .map(|v| {
            let pattern = v.to_le_bytes().into_iter().map(Some).collect();
            let mut hits = find_pattern(src, &pattern, PER_VALUE_CAP);
            hits.sort_unstable();
            hits
        })
        .collect();

    let anchors = std::mem::take(&mut lists[0]);
    let others = &lists[1..];

    let mut out = Vec::new();
    for a in anchors {
        if others.iter().all(|list| any_within(list, a, span)) {
            out.push(a);
            if out.len() >= max_results {
                break;
            }
        }
    }
    out
}

/// Is there any element of `sorted` within `span` of `center`?
fn any_within(sorted: &[u64], center: u64, span: u64) -> bool {
    let lo = center.saturating_sub(span);
    let hi = center.saturating_add(span);
    let i = sorted.partition_point(|&x| x < lo);
    sorted.get(i).is_some_and(|&x| x <= hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockMemory;
    use crate::value::ScanValue;

    #[test]
    fn finds_only_the_grouped_occurrence() {
        let base = 0x10_000u64;
        let mem = MockMemory::new(base, 0x1000);
        // A real group: 100 and 50 are 8 bytes apart.
        mem.poke(base + 0x100, &100i32.to_le_bytes());
        mem.poke(base + 0x108, &50i32.to_le_bytes());
        // A decoy 100 far away, with no 50 nearby.
        mem.poke(base + 0x800, &100i32.to_le_bytes());

        let values = [ScanValue::I32(100), ScanValue::I32(50)];
        let hits = group_scan(&mem, &values, 64, 100);
        assert_eq!(hits, vec![base + 0x100]);
    }

    #[test]
    fn respects_the_span() {
        let base = 0x20_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x10, &7i32.to_le_bytes());
        mem.poke(base + 0x400, &9i32.to_le_bytes()); // far from the 7

        let values = [ScanValue::I32(7), ScanValue::I32(9)];
        // Too small a span: no group.
        assert!(group_scan(&mem, &values, 16, 100).is_empty());
        // Large enough span: found.
        assert_eq!(group_scan(&mem, &values, 0x400, 100), vec![base + 0x10]);
    }
}
