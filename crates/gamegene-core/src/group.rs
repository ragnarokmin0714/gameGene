//! Group / multi-value scan.
//!
//! Cheat Engine calls this a "group" or "commonality" scan: instead of one
//! value, you give several (e.g. an item id, a count, and a level) and it finds
//! places where they all sit close together — a good way to locate a struct
//! when you know a few of its fields.
//!
//! Approach: search each value independently (reusing [`find_pattern`]), then
//! keep the addresses of the first value that have every other value within
//! `span` bytes (in either direction). Each hit records *where* every value
//! matched, so [`group_rescan`] can later narrow the results with a fresh set
//! of numbers (change the values in game, type the new ones, rescan — the
//! before/after workflow of a single-value next scan).

use crate::find::find_pattern;
use crate::process::MemorySource;
use crate::value::ScanValue;

/// How many matches to gather per value before correlating. Values in a group
/// scan are meant to be fairly specific; very common values (0, 1) can exceed
/// this and then only the first occurrences are considered.
const PER_VALUE_CAP: usize = 200_000;

/// One group-scan hit: the anchor (the first value's address) plus the address
/// where each of the *other* values matched (nearest occurrence), in the same
/// order they were entered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupHit {
    pub anchor: u64,
    pub others: Vec<u64>,
}

/// Find up to `max_results` addresses of `values[0]` that have every other
/// value within `span` bytes. With a single value this is just a plain search.
pub fn group_scan(
    src: &dyn MemorySource,
    values: &[ScanValue],
    span: u64,
    max_results: usize,
) -> Vec<GroupHit> {
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
        let matched: Option<Vec<u64>> = others
            .iter()
            .map(|list| nearest_within(list, a, span))
            .collect();
        if let Some(others) = matched {
            out.push(GroupHit { anchor: a, others });
            if out.len() >= max_results {
                break;
            }
        }
    }
    out
}

/// Narrow previous hits with a fresh set of values ("next group scan"): a hit
/// survives only if its anchor now holds `values[0]` and each recorded address
/// holds its corresponding value. `values` must pair up with the original scan
/// (same count, same order); hits from a different shape are dropped.
pub fn group_rescan(
    src: &dyn MemorySource,
    hits: &[GroupHit],
    values: &[ScanValue],
) -> Vec<GroupHit> {
    let Some((first, rest)) = values.split_first() else {
        return Vec::new();
    };
    hits.iter()
        .filter(|h| {
            h.others.len() == rest.len()
                && reads_as(src, h.anchor, first)
                && h.others.iter().zip(rest).all(|(&a, v)| reads_as(src, a, v))
        })
        .cloned()
        .collect()
}

/// Does memory at `addr` currently hold exactly `v` (little-endian bytes)?
fn reads_as(src: &dyn MemorySource, addr: u64, v: &ScanValue) -> bool {
    let want = v.to_le_bytes();
    let mut buf = [0u8; 16];
    let got = &mut buf[..want.len()];
    matches!(src.read(addr, got), Ok(n) if n == want.len()) && *got == want[..]
}

/// The element of `sorted` within `span` of `center` that lies closest to it.
fn nearest_within(sorted: &[u64], center: u64, span: u64) -> Option<u64> {
    let lo = center.saturating_sub(span);
    let hi = center.saturating_add(span);
    let start = sorted.partition_point(|&x| x < lo);
    sorted[start..]
        .iter()
        .take_while(|&&x| x <= hi)
        .copied()
        .min_by_key(|&x| x.abs_diff(center))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockMemory;
    use crate::value::ScanValue;

    fn anchors(hits: &[GroupHit]) -> Vec<u64> {
        hits.iter().map(|h| h.anchor).collect()
    }

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
        assert_eq!(anchors(&hits), vec![base + 0x100]);
        // The hit remembers where the 50 matched.
        assert_eq!(hits[0].others, vec![base + 0x108]);
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
        assert_eq!(
            anchors(&group_scan(&mem, &values, 0x400, 100)),
            vec![base + 0x10]
        );
    }

    #[test]
    fn picks_the_nearest_occurrence_of_each_value() {
        let base = 0x30_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x100, &100i32.to_le_bytes());
        // Two 50s within the span; the closer one should be recorded.
        mem.poke(base + 0x140, &50i32.to_le_bytes());
        mem.poke(base + 0x108, &50i32.to_le_bytes());

        let values = [ScanValue::I32(100), ScanValue::I32(50)];
        let hits = group_scan(&mem, &values, 0x100, 100);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].others, vec![base + 0x108]);
    }

    #[test]
    fn rescan_narrows_to_the_group_that_followed_the_change() {
        let base = 0x40_000u64;
        let mem = MockMemory::new(base, 0x1000);
        // Two identical-looking groups: (10, 14) at 0x100 and at 0x600.
        for off in [0x100u64, 0x600] {
            mem.poke(base + off, &10i32.to_le_bytes());
            mem.poke(base + off + 4, &14i32.to_le_bytes());
        }

        let before = [ScanValue::I32(10), ScanValue::I32(14)];
        let hits = group_scan(&mem, &before, 64, 100);
        assert_eq!(hits.len(), 2);

        // The real group (0x600) changes to (27, 35); the decoy stays.
        mem.poke(base + 0x600, &27i32.to_le_bytes());
        mem.poke(base + 0x604, &35i32.to_le_bytes());

        let after = [ScanValue::I32(27), ScanValue::I32(35)];
        let narrowed = group_rescan(&mem, &hits, &after);
        assert_eq!(anchors(&narrowed), vec![base + 0x600]);
        assert_eq!(narrowed[0].others, vec![base + 0x604]);
    }

    #[test]
    fn rescan_drops_hits_when_the_value_count_differs() {
        let base = 0x50_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x10, &5i32.to_le_bytes());
        mem.poke(base + 0x18, &6i32.to_le_bytes());

        let hits = group_scan(&mem, &[ScanValue::I32(5), ScanValue::I32(6)], 64, 100);
        assert_eq!(hits.len(), 1);
        // Rescanning with three values can't pair up with a two-value hit.
        let three = [ScanValue::I32(5), ScanValue::I32(6), ScanValue::I32(7)];
        assert!(group_rescan(&mem, &hits, &three).is_empty());
    }
}
