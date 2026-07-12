//! Engine tests: first scan finds candidates, next scan narrows them.

use gamegene_core::mock::MockMemory;
use gamegene_core::scan::{Compare, ScanSession};
use gamegene_core::value::{ScanValue, ValueType};

const BASE: u64 = 0x10000;

/// Build a mock with a few known i32 values planted at aligned offsets.
fn planted_mem() -> MockMemory {
    let mem = MockMemory::new(BASE, 64);
    mem.poke(BASE, &100i32.to_le_bytes()); // offset 0  -> 100
    mem.poke(BASE + 8, &100i32.to_le_bytes()); // offset 8  -> 100
    mem.poke(BASE + 16, &250i32.to_le_bytes()); // offset 16 -> 250
    mem.poke(BASE + 32, &100i32.to_le_bytes()); // offset 32 -> 100
    mem
}

#[test]
fn first_scan_exact_finds_all_occurrences() {
    let mem = planted_mem();
    let session =
        ScanSession::first_scan(&mem, ValueType::I32, Compare::Exact(ScanValue::I32(100)))
            .expect("first scan");

    let addrs: Vec<u64> = session.matches().iter().map(|m| m.address).collect();
    assert_eq!(addrs, vec![BASE, BASE + 8, BASE + 32]);
}

#[test]
fn next_scan_changed_narrows_to_the_edited_address() {
    let mem = planted_mem();
    let mut session =
        ScanSession::first_scan(&mem, ValueType::I32, Compare::Exact(ScanValue::I32(100)))
            .expect("first scan");
    assert_eq!(session.len(), 3);

    // Simulate the game changing only the value at BASE+8.
    mem.poke(BASE + 8, &175i32.to_le_bytes());

    session
        .next_scan(&mem, Compare::Changed)
        .expect("next scan");
    let addrs: Vec<u64> = session.matches().iter().map(|m| m.address).collect();
    assert_eq!(addrs, vec![BASE + 8]);
    // previous value should have been refreshed to the new reading.
    assert_eq!(session.matches()[0].previous, ScanValue::I32(175));
}

#[test]
fn next_scan_increased_and_decreased() {
    let mem = planted_mem();
    // Two sessions taken from the same baseline (previous = 100 at three spots)
    // BEFORE any mutation, so each can apply a different relative predicate.
    let mut inc =
        ScanSession::first_scan(&mem, ValueType::I32, Compare::Exact(ScanValue::I32(100))).unwrap();
    let mut dec =
        ScanSession::first_scan(&mem, ValueType::I32, Compare::Exact(ScanValue::I32(100))).unwrap();

    mem.poke(BASE, &120i32.to_le_bytes()); // increased
    mem.poke(BASE + 8, &90i32.to_le_bytes()); // decreased
                                              // BASE+32 unchanged

    inc.next_scan(&mem, Compare::Increased).unwrap();
    assert_eq!(
        inc.matches().iter().map(|m| m.address).collect::<Vec<_>>(),
        vec![BASE]
    );

    dec.next_scan(&mem, Compare::Decreased).unwrap();
    assert_eq!(
        dec.matches().iter().map(|m| m.address).collect::<Vec<_>>(),
        vec![BASE + 8]
    );
}

#[test]
fn greater_less_between_predicates() {
    let mem = planted_mem();

    let gt = ScanSession::first_scan(
        &mem,
        ValueType::I32,
        Compare::GreaterThan(ScanValue::I32(150)),
    )
    .unwrap();
    assert_eq!(
        gt.matches().iter().map(|m| m.address).collect::<Vec<_>>(),
        vec![BASE + 16] // only 250
    );

    let between = ScanSession::first_scan(
        &mem,
        ValueType::I32,
        Compare::Between(ScanValue::I32(90), ScanValue::I32(150)),
    )
    .unwrap();
    assert_eq!(
        between
            .matches()
            .iter()
            .map(|m| m.address)
            .collect::<Vec<_>>(),
        vec![BASE, BASE + 8, BASE + 32] // the three 100s
    );
}

#[test]
fn relative_compare_rejected_on_first_scan() {
    let mem = planted_mem();
    let err = ScanSession::first_scan(&mem, ValueType::I32, Compare::Changed).unwrap_err();
    assert_eq!(
        err,
        gamegene_core::error::ScanError::NeedsPrevious("changed")
    );
}

#[test]
fn float_scan_works() {
    let mem = MockMemory::new(BASE, 32);
    mem.poke(BASE, &3.5f32.to_le_bytes());
    mem.poke(BASE + 4, &9.0f32.to_le_bytes());

    let s =
        ScanSession::first_scan(&mem, ValueType::F32, Compare::Exact(ScanValue::F32(3.5))).unwrap();
    assert_eq!(
        s.matches().iter().map(|m| m.address).collect::<Vec<_>>(),
        vec![BASE]
    );
}

#[test]
fn unknown_first_scan_snapshots_then_narrows_on_change() {
    // 64 bytes = 16 aligned i32 slots, all initially zero.
    let mem = MockMemory::new(BASE, 64);
    let mut session =
        ScanSession::first_scan(&mem, ValueType::I32, Compare::Unknown).expect("unknown scan");
    // Every slot is a candidate; nothing is materialized per-address yet.
    assert_eq!(session.len(), 16);

    // Change exactly one slot, then narrow by "changed".
    mem.poke(BASE + 8, &4321i32.to_le_bytes());
    session
        .next_scan(&mem, Compare::Changed)
        .expect("next scan");

    assert_eq!(
        session.matches(),
        vec![gamegene_core::scan::Match {
            address: BASE + 8,
            previous: ScanValue::I32(4321),
        }]
    );
}

#[test]
fn unknown_scan_then_absolute_next_scan() {
    let mem = MockMemory::new(BASE, 32);
    mem.poke(BASE + 12, &777i32.to_le_bytes());
    let mut session = ScanSession::first_scan(&mem, ValueType::I32, Compare::Unknown).unwrap();

    // An absolute predicate works against a snapshot too.
    session
        .next_scan(&mem, Compare::Exact(ScanValue::I32(777)))
        .unwrap();
    assert_eq!(
        session
            .matches()
            .iter()
            .map(|m| m.address)
            .collect::<Vec<_>>(),
        vec![BASE + 12]
    );
}

#[test]
fn block_coalesced_next_scan_narrows_dense_candidates() {
    // 16 dense i32 slots all set to 100; a first Exact scan matches all of them,
    // so next_scan must read them via coalesced blocks and narrow correctly.
    let mem = MockMemory::new(BASE, 64);
    for k in 0..16u64 {
        mem.poke(BASE + k * 4, &100i32.to_le_bytes());
    }
    let mut session =
        ScanSession::first_scan(&mem, ValueType::I32, Compare::Exact(ScanValue::I32(100))).unwrap();
    assert_eq!(session.len(), 16);

    // Change two of them; "changed" should keep exactly those two.
    mem.poke(BASE + 4, &101i32.to_le_bytes());
    mem.poke(BASE + 40, &101i32.to_le_bytes());
    session.next_scan(&mem, Compare::Changed).unwrap();

    assert_eq!(
        session
            .matches()
            .iter()
            .map(|m| m.address)
            .collect::<Vec<_>>(),
        vec![BASE + 4, BASE + 40]
    );
}

#[test]
fn parallel_scan_finds_matches_across_work_items() {
    // Larger than one 16 MiB work item so the scan splits across threads and
    // the merge/sort path is exercised; plant a rare value in several items.
    let size = 40 * 1024 * 1024;
    let mem = MockMemory::new(BASE, size);
    let offsets = [0u64, 20_000_000, 33_000_000];
    for off in offsets {
        let aligned = off & !3; // keep it on an i32 slot boundary
        mem.poke(BASE + aligned, &424_242i32.to_le_bytes());
    }
    let session = ScanSession::first_scan(
        &mem,
        ValueType::I32,
        Compare::Exact(ScanValue::I32(424_242)),
    )
    .unwrap();
    let mut addrs: Vec<u64> = session.matches().iter().map(|m| m.address).collect();
    addrs.sort_unstable();
    let mut want: Vec<u64> = offsets.iter().map(|o| BASE + (o & !3)).collect();
    want.sort_unstable();
    assert_eq!(
        addrs, want,
        "results must be complete and sorted after merge"
    );
    assert!(!session.truncated());
}

#[test]
fn cancelled_scan_stops_early() {
    use gamegene_core::scan::ScanControl;
    let mem = MockMemory::new(BASE, 32 * 1024 * 1024);
    let control = ScanControl::new();
    control.request_cancel(); // cancel before it starts
    let session = ScanSession::first_scan_with(
        &mem,
        ValueType::I32,
        Compare::Exact(ScanValue::I32(0)),
        &control,
    )
    .unwrap();
    // Every i32 in a zeroed buffer equals 0, so an un-cancelled scan would
    // return millions; cancelling first must yield far fewer (ideally none).
    assert!(
        session.len() < 1000,
        "cancelled scan returned {} matches",
        session.len()
    );
}

#[test]
fn native_compare_orders_large_integers_exactly() {
    // Two i64 values just above 2^53, where the old f64 comparison path could
    // not tell them apart. GreaterThan must select only the strictly-larger one.
    let base = (1i64 << 53) + 1;
    let mem = MockMemory::new(BASE, 32);
    mem.poke(BASE, &base.to_le_bytes());
    mem.poke(BASE + 8, &(base + 1).to_le_bytes());
    let session = ScanSession::first_scan(
        &mem,
        ValueType::I64,
        Compare::GreaterThan(ScanValue::I64(base)),
    )
    .unwrap();
    let addrs: Vec<u64> = session.matches().iter().map(|m| m.address).collect();
    assert_eq!(addrs, vec![BASE + 8]);
}
