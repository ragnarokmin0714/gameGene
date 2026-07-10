//! Engine tests: first scan finds candidates, next scan narrows them.

use memgene_core::mock::MockMemory;
use memgene_core::scan::{Compare, ScanSession};
use memgene_core::value::{ScanValue, ValueType};

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
    let session = ScanSession::first_scan(&mem, ValueType::I32, Compare::Exact(ScanValue::I32(100)))
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

    session.next_scan(&mem, Compare::Changed).expect("next scan");
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

    let gt = ScanSession::first_scan(&mem, ValueType::I32, Compare::GreaterThan(ScanValue::I32(150)))
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
        between.matches().iter().map(|m| m.address).collect::<Vec<_>>(),
        vec![BASE, BASE + 8, BASE + 32] // the three 100s
    );
}

#[test]
fn relative_compare_rejected_on_first_scan() {
    let mem = planted_mem();
    let err = ScanSession::first_scan(&mem, ValueType::I32, Compare::Changed).unwrap_err();
    assert_eq!(err, memgene_core::error::ScanError::NeedsPrevious("changed"));
}

#[test]
fn float_scan_works() {
    let mem = MockMemory::new(BASE, 32);
    mem.poke(BASE, &3.5f32.to_le_bytes());
    mem.poke(BASE + 4, &9.0f32.to_le_bytes());

    let s = ScanSession::first_scan(&mem, ValueType::F32, Compare::Exact(ScanValue::F32(3.5)))
        .unwrap();
    assert_eq!(
        s.matches().iter().map(|m| m.address).collect::<Vec<_>>(),
        vec![BASE]
    );
}
