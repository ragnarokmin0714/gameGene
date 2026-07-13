//! Group / multi-value scan.
//!
//! Cheat Engine calls this a "group" or "commonality" scan: instead of one
//! value, you give several (e.g. an item id, a count, and a level) and it finds
//! places where they all sit close together — a good way to locate a struct
//! when you know a few of its fields.
//!
//! Approach: search each value independently over aligned slots, then keep the
//! addresses of the first value that have every other value within `span`
//! bytes (in either direction). Each hit records *where* every value matched,
//! so [`group_rescan`] can later narrow the results with a fresh set of
//! numbers (change the values in game, type the new ones, rescan — the
//! before/after workflow of a single-value next scan).
//!
//! A value can be a [`GroupQuery::Range`] instead of an exact number — the way
//! to search floats when the game only shows the integer part (a HUD "12" is
//! really 12.37 in memory, which an exact byte match can never hit).

use crate::process::MemorySource;
use crate::scan::{collect_addresses_with, Compare, ScanControl};
use crate::value::{ScanValue, ValueType};

/// One value of a group scan: match exactly, or anywhere within an inclusive
/// range (for floats whose exact bits are unknown).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupQuery {
    Exact(ScanValue),
    Range(ScanValue, ScanValue),
}

impl GroupQuery {
    fn value_type(&self) -> ValueType {
        match self {
            GroupQuery::Exact(v) | GroupQuery::Range(v, _) => v.value_type(),
        }
    }

    fn compare(&self) -> Compare {
        match *self {
            GroupQuery::Exact(v) => Compare::Exact(v),
            GroupQuery::Range(lo, hi) => Compare::Between(lo, hi),
        }
    }

    /// Does memory at `addr` currently satisfy this query?
    fn holds_at(&self, src: &dyn MemorySource, addr: u64) -> bool {
        let size = self.value_type().size();
        let mut buf = [0u8; 8];
        matches!(src.read(addr, &mut buf[..size]), Ok(n) if n == size)
            && self
                .compare()
                .eval(ScanValue::from_le_bytes(self.value_type(), &buf), None)
    }
}

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

/// Find up to `max_results` addresses of `queries[0]` that have every other
/// query within `span` bytes. With a single query this is just a plain search.
/// Matches are aligned to each query's value size (game struct fields are).
pub fn group_scan(
    src: &dyn MemorySource,
    queries: &[GroupQuery],
    span: u64,
    max_results: usize,
) -> Vec<GroupHit> {
    group_scan_with(src, queries, span, max_results, &ScanControl::new())
}

/// [`group_scan`] honoring a [`ScanControl`], so it can run on a background
/// thread and be cancelled. Progress is per-value (each value is a full sweep),
/// so the UI shows an indeterminate bar rather than a fraction.
pub fn group_scan_with(
    src: &dyn MemorySource,
    queries: &[GroupQuery],
    span: u64,
    max_results: usize,
    control: &ScanControl,
) -> Vec<GroupHit> {
    if queries.is_empty() || max_results == 0 {
        return Vec::new();
    }

    let mut lists: Vec<Vec<u64>> = Vec::with_capacity(queries.len());
    for q in queries {
        if control.is_cancelled() {
            return Vec::new();
        }
        let mut hits =
            collect_addresses_with(src, q.value_type(), q.compare(), PER_VALUE_CAP, control);
        hits.sort_unstable();
        lists.push(hits);
    }

    let anchors = std::mem::take(&mut lists[0]);
    let others = &lists[1..];

    let mut out = Vec::new();
    let mut claimed = Vec::new(); // addresses used by this anchor's hit
    for a in anchors {
        // Each query must land on a *distinct* address, so a repeated value like
        // [30 30] means "two different nearby 30s" (HP and MP both 30), not the
        // same 30 paired with itself. Claim addresses greedily, nearest first.
        claimed.clear();
        claimed.push(a);
        let matched: Option<Vec<u64>> = others
            .iter()
            .map(|list| {
                let hit = nearest_within(list, a, span, &claimed)?;
                claimed.push(hit);
                Some(hit)
            })
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

/// Narrow previous hits with a fresh set of queries ("next group scan"): a hit
/// survives only if its anchor now satisfies `queries[0]` and each recorded
/// address satisfies its corresponding query. `queries` must pair up with the
/// original scan (same count, same order); hits from a different shape are
/// dropped.
pub fn group_rescan(
    src: &dyn MemorySource,
    hits: &[GroupHit],
    queries: &[GroupQuery],
) -> Vec<GroupHit> {
    let Some((first, rest)) = queries.split_first() else {
        return Vec::new();
    };
    hits.iter()
        .filter(|h| {
            h.others.len() == rest.len()
                && first.holds_at(src, h.anchor)
                && h.others.iter().zip(rest).all(|(&a, q)| q.holds_at(src, a))
        })
        .cloned()
        .collect()
}

/// The element of `sorted` within `span` of `center` that lies closest to it,
/// skipping any address already `claimed` by another query in the same hit.
fn nearest_within(sorted: &[u64], center: u64, span: u64, claimed: &[u64]) -> Option<u64> {
    let lo = center.saturating_sub(span);
    let hi = center.saturating_add(span);
    let start = sorted.partition_point(|&x| x < lo);
    sorted[start..]
        .iter()
        .take_while(|&&x| x <= hi)
        .copied()
        .filter(|x| !claimed.contains(x))
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

    fn exact(v: i32) -> GroupQuery {
        GroupQuery::Exact(ScanValue::I32(v))
    }

    /// A float known only to its integer part: `12` → the range `[12, 13]`.
    fn about(v: f32) -> GroupQuery {
        GroupQuery::Range(ScanValue::F32(v), ScanValue::F32(v + 1.0))
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

        let queries = [exact(100), exact(50)];
        let hits = group_scan(&mem, &queries, 64, 100);
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

        let queries = [exact(7), exact(9)];
        // Too small a span: no group.
        assert!(group_scan(&mem, &queries, 16, 100).is_empty());
        // Large enough span: found.
        assert_eq!(
            anchors(&group_scan(&mem, &queries, 0x400, 100)),
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

        let queries = [exact(100), exact(50)];
        let hits = group_scan(&mem, &queries, 0x100, 100);
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

        let before = [exact(10), exact(14)];
        let hits = group_scan(&mem, &before, 64, 100);
        assert_eq!(hits.len(), 2);

        // The real group (0x600) changes to (27, 35); the decoy stays.
        mem.poke(base + 0x600, &27i32.to_le_bytes());
        mem.poke(base + 0x604, &35i32.to_le_bytes());

        let after = [exact(27), exact(35)];
        let narrowed = group_rescan(&mem, &hits, &after);
        assert_eq!(anchors(&narrowed), vec![base + 0x600]);
        assert_eq!(narrowed[0].others, vec![base + 0x604]);
    }

    #[test]
    fn float_ranges_find_values_with_unknown_decimals() {
        let base = 0x60_000u64;
        let mem = MockMemory::new(base, 0x1000);
        // A struct of three floats a HUD would show as 12, 20 and 6.
        mem.poke(base + 0x200, &12.37f32.to_le_bytes());
        mem.poke(base + 0x204, &20.71f32.to_le_bytes());
        mem.poke(base + 0x208, &6.02f32.to_le_bytes());
        // A decoy in the 12…13 range with no partners nearby.
        mem.poke(base + 0x900, &12.9f32.to_le_bytes());

        let queries = [about(12.0), about(20.0), about(6.0)];
        let hits = group_scan(&mem, &queries, 64, 100);
        assert_eq!(anchors(&hits), vec![base + 0x200]);
        assert_eq!(hits[0].others, vec![base + 0x204, base + 0x208]);

        // Rescan after the values drifted within new ranges: HP fell to 11.x.
        mem.poke(base + 0x200, &11.9f32.to_le_bytes());
        let narrowed = group_rescan(&mem, &hits, &[about(11.0), about(20.0), about(6.0)]);
        assert_eq!(anchors(&narrowed), vec![base + 0x200]);
        // A range the value left no longer matches.
        assert!(group_rescan(&mem, &hits, &[about(30.0), about(20.0), about(6.0)]).is_empty());
    }

    #[test]
    fn duplicate_values_pair_two_distinct_addresses() {
        let base = 0x70_000u64;
        let mem = MockMemory::new(base, 0x1000);
        // A real pair: HP and MP, both 30, eight bytes apart.
        mem.poke(base + 0x100, &30i32.to_le_bytes());
        mem.poke(base + 0x108, &30i32.to_le_bytes());
        // A lone 30 with no second 30 nearby — must NOT self-pair into a hit.
        mem.poke(base + 0x900, &30i32.to_le_bytes());

        let hits = group_scan(&mem, &[exact(30), exact(30)], 64, 100);
        // Only the two members of the real pair anchor a hit, each pointing at
        // the other — never at itself, and never the lone 30.
        assert_eq!(anchors(&hits), vec![base + 0x100, base + 0x108]);
        assert_eq!(hits[0].others, vec![base + 0x108]);
        assert_eq!(hits[1].others, vec![base + 0x100]);

        // Change both to 33 in game; rescan with [33 33] keeps the pair.
        mem.poke(base + 0x100, &33i32.to_le_bytes());
        mem.poke(base + 0x108, &33i32.to_le_bytes());
        let narrowed = group_rescan(&mem, &hits, &[exact(33), exact(33)]);
        assert_eq!(anchors(&narrowed), vec![base + 0x100, base + 0x108]);
    }

    #[test]
    fn rescan_drops_hits_when_the_value_count_differs() {
        let base = 0x50_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x10, &5i32.to_le_bytes());
        mem.poke(base + 0x18, &6i32.to_le_bytes());

        let hits = group_scan(&mem, &[exact(5), exact(6)], 64, 100);
        assert_eq!(hits.len(), 1);
        // Rescanning with three values can't pair up with a two-value hit.
        let three = [exact(5), exact(6), exact(7)];
        assert!(group_rescan(&mem, &hits, &three).is_empty());
    }
}
