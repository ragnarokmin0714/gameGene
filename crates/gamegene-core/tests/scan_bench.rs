//! Rough scan-throughput benchmark, not a correctness test.
//!
//! Ignored by default (it allocates a big buffer and is timing-dependent). Run
//! it to compare the engine before/after a change:
//!
//! ```sh
//! cargo test -p gamegene-core --release --test scan_bench -- --ignored --nocapture
//! ```
//!
//! It measures the compute side of scanning (the `MockMemory` read is a cheap
//! memcpy), so it reflects the inner-loop and parallelism work, not real
//! process-read I/O — see `linux_selftest` for a throughput number over a real
//! `/proc/self/mem`.

use gamegene_core::mock::MockMemory;
use gamegene_core::scan::ScanSession;
use gamegene_core::value::{ScanValue, ValueType};
use std::time::Instant;

const BASE: u64 = 0x1_0000_0000;

fn throughput(label: &str, bytes: usize, run: impl FnOnce() -> usize) {
    let t = Instant::now();
    let hits = run();
    let secs = t.elapsed().as_secs_f64();
    let gib = bytes as f64 / (1 << 30) as f64;
    println!(
        "{label:<28} {gib:5.2} GiB in {secs:6.3}s = {:6.2} GiB/s  ({hits} hits)",
        gib / secs
    );
}

#[test]
#[ignore = "benchmark: run with --ignored --nocapture --release"]
fn first_scan_throughput() {
    // 512 MiB of memory to sweep — enough to spread across the work-item pool.
    let size = 512 * 1024 * 1024;
    let mem = MockMemory::new(BASE, size);
    // Sprinkle a rare exact value so the result set stays small.
    for k in 0..1000 {
        mem.poke(BASE + (k * 0x40000) as u64, &777_777i32.to_le_bytes());
    }

    throughput("i32 exact (rare value)", size, || {
        ScanSession::first_scan(
            &mem,
            ValueType::I32,
            gamegene_core::Compare::Exact(ScanValue::I32(777_777)),
        )
        .unwrap()
        .len()
    });

    throughput("f32 between (range)", size, || {
        ScanSession::first_scan(
            &mem,
            ValueType::F32,
            gamegene_core::Compare::Between(ScanValue::F32(1.0), ScanValue::F32(2.0)),
        )
        .unwrap()
        .len()
    });

    throughput("unknown snapshot", size, || {
        ScanSession::first_scan(&mem, ValueType::I32, gamegene_core::Compare::Unknown)
            .unwrap()
            .len()
    });
}
