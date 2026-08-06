//! Pointer-scanner tests: find a static pointer path to a target address and
//! confirm the found chain actually resolves back to it.

use gamegene_core::mock::MockMemory;
use gamegene_core::pointer::{pointer_scan, revalidate, PointerScanOptions};
use gamegene_core::table::Locator;

const BASE: u64 = 0x40_0000;

#[test]
fn finds_single_hop_pointer_path() {
    // Static B = BASE+0x10 holds a pointer to BASE+0x100.
    // Target = *(B) + 0x20 = BASE+0x120.
    let mem = MockMemory::new(BASE, 0x1000).with_module("game.exe", BASE);
    mem.poke(BASE + 0x10, &(BASE + 0x100).to_le_bytes());
    let target = BASE + 0x120;

    let paths = pointer_scan(&mem, target, PointerScanOptions::default());
    assert!(!paths.is_empty(), "expected at least one pointer path");

    // Every returned path must resolve back to the target.
    for p in &paths {
        assert_eq!(p.resolve(&mem), Some(target));
    }
    // The one-hop path anchored at the module should be present.
    assert!(paths.iter().any(|p| matches!(
        p,
        Locator::Pointer { module, base_offset, offsets }
            if module == "game.exe" && *base_offset == 0x10 && offsets == &[0x20]
    )));
}

#[test]
fn finds_two_hop_pointer_path() {
    // Module covers only [BASE, BASE+0x100); pointers stored above it are
    // "heap" (non-static), forcing a genuine two-hop chain.
    //   B = BASE+0x10 (static) -> BASE+0x800
    //   *(BASE+0x800 + 0x8) = BASE+0x900
    //   target = BASE+0x900 + 0x30
    let mem = MockMemory::new(BASE, 0x1000).with_module_range("game.exe", BASE, 0x100);
    mem.poke(BASE + 0x10, &(BASE + 0x800).to_le_bytes());
    mem.poke(BASE + 0x808, &(BASE + 0x900).to_le_bytes());
    let target = BASE + 0x930;

    let paths = pointer_scan(&mem, target, PointerScanOptions::default());
    assert!(!paths.is_empty(), "expected a two-hop path");
    for p in &paths {
        assert_eq!(p.resolve(&mem), Some(target));
    }
    assert!(paths.iter().any(|p| matches!(
        p,
        Locator::Pointer { base_offset, offsets, .. }
            if *base_offset == 0x10 && offsets == &[0x8, 0x30]
    )));
}

#[test]
fn no_modules_means_no_paths() {
    // Without any module there is no static anchor to build a path from.
    let mem = MockMemory::new(BASE, 0x100);
    mem.poke(BASE + 0x10, &(BASE + 0x20).to_le_bytes());
    assert!(pointer_scan(&mem, BASE + 0x20, PointerScanOptions::default()).is_empty());
}

#[test]
fn revalidate_keeps_only_the_paths_that_survive_a_restart() {
    // Run 1: a real chain (static B → object → +0x20) and a coincidence — a
    // stray value that happens to land on the target this time round.
    let mem = MockMemory::new(BASE, 0x1000).with_module("game.exe", BASE);
    mem.poke(BASE + 0x10, &(BASE + 0x100).to_le_bytes()); // the real base pointer
    mem.poke(BASE + 0x18, &(BASE + 0x120).to_le_bytes()); // a coincidental one
    let target = BASE + 0x120;

    let paths = pointer_scan(&mem, target, PointerScanOptions::default());
    assert!(paths.len() >= 2, "fixture should offer several candidates");
    assert!(paths.iter().all(|p| p.resolve(&mem) == Some(target)));

    // Run 2: the object moved, as it would after a restart. The real chain
    // follows it because the base pointer was updated; the coincidence did not.
    let moved = BASE + 0x300;
    mem.poke(BASE + 0x10, &moved.to_le_bytes());
    let new_target = moved + 0x20;

    let survivors = revalidate(&mem, &paths, new_target);
    assert!(!survivors.is_empty(), "the real chain must survive");
    assert!(
        survivors.len() < paths.len(),
        "the coincidence must be dropped"
    );
    assert!(survivors
        .iter()
        .all(|p| p.resolve(&mem) == Some(new_target)));
}

#[test]
fn revalidate_against_an_unreachable_target_keeps_nothing() {
    let mem = MockMemory::new(BASE, 0x1000).with_module("game.exe", BASE);
    mem.poke(BASE + 0x10, &(BASE + 0x100).to_le_bytes());
    let paths = pointer_scan(&mem, BASE + 0x120, PointerScanOptions::default());
    assert!(!paths.is_empty());
    // Nothing resolves here, so an honest result is an empty list rather than
    // the closest guess.
    assert!(revalidate(&mem, &paths, BASE + 0x900).is_empty());
}
