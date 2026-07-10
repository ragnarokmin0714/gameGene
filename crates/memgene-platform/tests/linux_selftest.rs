//! End-to-end test of the real Linux backend against this very process.
//!
//! This is the closest thing to "attach to a running game and edit it" that we
//! can run in CI: plant a known value on our own heap, scan for it through
//! `/proc/self/mem`, confirm the engine finds the exact address, then write a
//! new value and confirm our own variable changed.
#![cfg(target_os = "linux")]

use memgene_core::scan::{Compare, ScanSession};
use memgene_core::value::{ScanValue, ValueType};
use memgene_platform::attach;

#[test]
fn scans_and_edits_own_process_memory() {
    // A distinctive value unlikely to occur by accident, forced onto the heap.
    let sentinel: i64 = 0x1234_5678_9ABC_DEF0u64 as i64;
    let boxed = Box::new(sentinel);
    let addr = &*boxed as *const i64 as u64;

    let src = attach(std::process::id()).expect("attach to self");

    let session = ScanSession::first_scan(
        &*src,
        ValueType::I64,
        Compare::Exact(ScanValue::I64(sentinel)),
    )
    .expect("first scan");

    assert!(
        session.matches().iter().any(|m| m.address == addr),
        "planted value at {addr:#x} not found among {} matches",
        session.len()
    );

    // Edit it through the backend; our own boxed variable must observe it.
    src.write(addr, &999i64.to_le_bytes())
        .expect("write to own memory");
    assert_eq!(*boxed, 999, "write via backend did not take effect");
}
