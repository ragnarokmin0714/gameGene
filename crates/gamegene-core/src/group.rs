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

    /// Whether the value at the front of `bytes` (`bytes.len() >= size`)
    /// satisfies this query.
    fn holds_bytes(&self, bytes: &[u8]) -> bool {
        self.compare()
            .eval(ScanValue::from_le_bytes(self.value_type(), bytes), None)
    }

    /// Does memory at `addr` currently satisfy this query?
    fn holds_at(&self, src: &dyn MemorySource, addr: u64) -> bool {
        let size = self.value_type().size();
        let mut buf = [0u8; 8];
        matches!(src.read(addr, &mut buf[..size]), Ok(n) if n == size) && self.holds_bytes(&buf)
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
///
/// Drops the "some value was too common to sweep fully" flag; use
/// [`group_scan_with`] when that needs reporting.
pub fn group_scan(
    src: &dyn MemorySource,
    queries: &[GroupQuery],
    span: u64,
    max_results: usize,
) -> Vec<GroupHit> {
    group_scan_with(src, queries, span, max_results, &ScanControl::new()).0
}

/// [`group_scan`] honoring a [`ScanControl`], so it can run on a background
/// thread and be cancelled. Each value is a full memory sweep, so the progress
/// total is set to `values × bytes` up front and the bar fills across them all.
///
/// The second return value is true when at least one value matched more than
/// [`PER_VALUE_CAP`] slots, so only its lowest matches were correlated and the
/// result may be incomplete — worth telling the user, since the fix is to pick
/// a more specific value rather than to rescan and hope.
pub fn group_scan_with(
    src: &dyn MemorySource,
    queries: &[GroupQuery],
    span: u64,
    max_results: usize,
    control: &ScanControl,
) -> (Vec<GroupHit>, bool) {
    if queries.is_empty() || max_results == 0 {
        return (Vec::new(), false);
    }

    // One aggregate progress total across every value's sweep.
    let bytes: u64 = src.regions().iter().map(|r| r.size).sum();
    control.set_total(bytes * queries.len() as u64);

    let mut lists: Vec<Vec<u64>> = Vec::with_capacity(queries.len());
    let mut capped: Vec<bool> = Vec::with_capacity(queries.len());
    for q in queries {
        if control.is_cancelled() {
            return (Vec::new(), false);
        }
        let (hits, truncated) = collect_addresses_with(
            src,
            q.value_type(),
            q.compare(),
            PER_VALUE_CAP,
            control,
            false, // group owns the aggregate total set above
        );
        lists.push(hits);
        capped.push(truncated);
    }
    let truncated = capped.iter().any(|&t| t);

    let anchors = std::mem::take(&mut lists[0]);
    let others = &lists[1..];
    let others_capped = &capped[1..];

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
            .zip(others_capped)
            .zip(&queries[1..])
            .map(|((list, &list_capped), q)| {
                // A capped list holds only the lowest matches, so a partner
                // above the cut is missing from it even though it is sitting
                // right next to the anchor. Re-read around the anchor rather
                // than dropping a real group over a sweep limit.
                let hit = match nearest_within(list, a, span, &claimed) {
                    Some(hit) => hit,
                    None if list_capped => find_near(src, a, span, q, &claimed)?,
                    None => return None,
                };
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
    (out, truncated)
}

/// Narrow previous hits with a fresh set of queries ("next group scan"): a hit
/// survives only if its anchor now satisfies `queries[0]` and every other query
/// can still be found within `span` bytes of the anchor.
///
/// Crucially, this **re-searches** around each anchor rather than re-checking
/// the specific partner addresses the first scan recorded. The first scan only
/// remembers each value's *nearest* occurrence, which may be an unrelated decoy
/// sitting closer than the real field — checking that fixed address would drop
/// the real group even though the new value is right there. Re-searching keeps a
/// hit as long as the values are still grouped near the anchor, and refreshes
/// `others` to where they now sit. `queries` must pair up with the original scan
/// (same count, same order); hits from a different shape are dropped.
pub fn group_rescan(
    src: &dyn MemorySource,
    hits: &[GroupHit],
    queries: &[GroupQuery],
    span: u64,
) -> Vec<GroupHit> {
    let Some((first, rest)) = queries.split_first() else {
        return Vec::new();
    };
    hits.iter()
        .filter_map(|h| {
            if h.others.len() != rest.len() || !first.holds_at(src, h.anchor) {
                return None;
            }
            // Re-find each other value near the anchor, on distinct addresses.
            let mut claimed = vec![h.anchor];
            let mut others = Vec::with_capacity(rest.len());
            for q in rest {
                let found = find_near(src, h.anchor, span, q, &claimed)?;
                claimed.push(found);
                others.push(found);
            }
            Some(GroupHit {
                anchor: h.anchor,
                others,
            })
        })
        .collect()
}

/// One OS page. The search window is read a page at a time so that a single
/// unreadable page costs one page of the window instead of all of it.
const PAGE: u64 = 4096;

/// Nearest address within `span` of `anchor` holding `q`, size-aligned and not
/// already `claimed`. Re-read fresh each rescan, so a hit survives as long as
/// the value is *somewhere* near the anchor — not tied to whichever occurrence
/// the first scan happened to record.
///
/// Reads stop at every page boundary. `ReadProcessMemory` fails *atomically*, so
/// one window-sized read that happens to run off the end of the region returns
/// nothing at all — which silently dropped every hit anchored near a region
/// edge, and looked like the group scan intermittently losing results.
fn find_near(
    src: &dyn MemorySource,
    anchor: u64,
    span: u64,
    q: &GroupQuery,
    claimed: &[u64],
) -> Option<u64> {
    let size = q.value_type().size() as u64;
    // Window [anchor-span, anchor+span], aligned down to the value size so every
    // stepped address is size-aligned (game struct fields are).
    let start = anchor.saturating_sub(span) & !(size - 1);
    let end = anchor.saturating_add(span).saturating_add(size);

    let sz = size as usize;
    let mut buf = vec![0u8; PAGE as usize];
    let mut best: Option<u64> = None;
    let mut addr = start;
    while addr < end {
        // Never span a page boundary. `start` is size-aligned and a page is a
        // multiple of every value size, so each block length is a whole number
        // of slots and no value straddles two blocks.
        let stop = ((addr & !(PAGE - 1)) + PAGE).min(end);
        let len = (stop - addr) as usize;
        let got = src.read(addr, &mut buf[..len]).unwrap_or(0);
        let mut off = 0usize;
        while off + sz <= got {
            let at = addr + off as u64;
            if at.abs_diff(anchor) <= span
                && !claimed.contains(&at)
                && q.holds_bytes(&buf[off..off + sz])
                && best.is_none_or(|b| at.abs_diff(anchor) < b.abs_diff(anchor))
            {
                best = Some(at);
            }
            off += sz;
        }
        addr = stop;
    }
    best
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
        let narrowed = group_rescan(&mem, &hits, &after, 64);
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
        let narrowed = group_rescan(&mem, &hits, &[about(11.0), about(20.0), about(6.0)], 64);
        assert_eq!(anchors(&narrowed), vec![base + 0x200]);
        // A range the value left no longer matches.
        assert!(group_rescan(&mem, &hits, &[about(30.0), about(20.0), about(6.0)], 64).is_empty());
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
        let narrowed = group_rescan(&mem, &hits, &[exact(33), exact(33)], 64);
        assert_eq!(anchors(&narrowed), vec![base + 0x100, base + 0x108]);
    }

    #[test]
    fn mixed_values_survive_when_one_field_changes() {
        // Reported case [33 30] -> [33 34]: field A stays 33, field B 30 -> 34.
        let base = 0x90_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x200, &33i32.to_le_bytes()); // A (anchor value)
        mem.poke(base + 0x204, &30i32.to_le_bytes()); // B
        let hits = group_scan(&mem, &[exact(33), exact(30)], 64, 100);
        assert_eq!(anchors(&hits), vec![base + 0x200]);

        mem.poke(base + 0x204, &34i32.to_le_bytes()); // B: 30 -> 34
        let narrowed = group_rescan(&mem, &hits, &[exact(33), exact(34)], 64);
        assert_eq!(anchors(&narrowed), vec![base + 0x200]);
        assert_eq!(narrowed[0].others, vec![base + 0x204]);
    }

    #[test]
    fn duplicate_then_one_changes() {
        // Reported case [20 20] -> [21 20]: two equal fields, one edited.
        let base = 0xA0_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x300, &20i32.to_le_bytes()); // X
        mem.poke(base + 0x304, &20i32.to_le_bytes()); // Y
        let hits = group_scan(&mem, &[exact(20), exact(20)], 64, 100);
        assert_eq!(anchors(&hits), vec![base + 0x300, base + 0x304]);

        mem.poke(base + 0x300, &21i32.to_le_bytes()); // X: 20 -> 21
                                                      // Only the field that became 21 anchors a surviving [21 20] group.
        let narrowed = group_rescan(&mem, &hits, &[exact(21), exact(20)], 64);
        assert_eq!(anchors(&narrowed), vec![base + 0x300]);
        assert_eq!(narrowed[0].others, vec![base + 0x304]);
    }

    #[test]
    fn rescan_survives_an_anchor_near_the_region_edge() {
        // The search window around an anchor close to the start of a region runs
        // off the front of it. Reading the whole window in one go fails
        // atomically there, which used to drop the hit even though the partner
        // sits right next to the anchor.
        let base = 0xB0_000u64;
        let mem = MockMemory::new(base, 0x1000);
        let anchor = base + 0x10; // well inside `span` of the region start
        mem.poke(anchor, &42i32.to_le_bytes());
        mem.poke(anchor + 8, &7i32.to_le_bytes());

        let queries = [exact(42), exact(7)];
        let span = 512;
        let hits = group_scan(&mem, &queries, span, 100);
        assert_eq!(anchors(&hits), vec![anchor]);

        let narrowed = group_rescan(&mem, &hits, &queries, span);
        assert_eq!(
            anchors(&narrowed),
            vec![anchor],
            "the partner is 8 bytes away; an unreadable page before the region \
             must not discard the whole window"
        );
        assert_eq!(narrowed[0].others, vec![anchor + 8]);
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
        assert!(group_rescan(&mem, &hits, &three, 64).is_empty());
    }
    #[test]
    fn repro_decoy_partner_intermittent() {
        // anchor 33, a REAL partner 30 that will become 34, and a DECOY 30
        // sitting nearer the anchor. First scan records the nearest (decoy);
        // rescan then drops the real group even though a 34 IS within span.
        let base = 0x80_000u64;
        let mem = MockMemory::new(base, 0x1000);
        mem.poke(base + 0x100, &33i32.to_le_bytes()); // anchor
        mem.poke(base + 0x108, &30i32.to_le_bytes()); // decoy 30 (nearer)
        mem.poke(base + 0x140, &30i32.to_le_bytes()); // real partner
        let hits = group_scan(&mem, &[exact(33), exact(30)], 0x100, 100);
        assert_eq!(anchors(&hits), vec![base + 0x100]);

        // The real partner changes to 34; decoy stays 30.
        mem.poke(base + 0x140, &34i32.to_le_bytes());
        let narrowed = group_rescan(&mem, &hits, &[exact(33), exact(34)], 0x100);
        assert_eq!(
            anchors(&narrowed),
            vec![base + 0x100],
            "anchor has a 34 within span; the group should survive"
        );
    }
}
