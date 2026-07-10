//! Cheat-table tests: locator resolution, apply/freeze, and round-trip save.

use memgene_core::mock::MockMemory;
use memgene_core::process::MemorySource;
use memgene_core::table::{CheatTable, Locator, TableEntry};
use memgene_core::value::{ScanValue, ValueType};

const MOD_BASE: u64 = 0x400000;

fn entry(label: &str, locator: Locator, desired: Option<ScanValue>) -> TableEntry {
    TableEntry {
        id: 0,
        label: label.to_string(),
        value_type: ValueType::I32,
        locator,
        desired,
        frozen: false,
        notes: String::new(),
    }
}

#[test]
fn module_offset_resolves_and_writes() {
    let mem = MockMemory::new(MOD_BASE, 0x1000).with_module("game.exe", MOD_BASE);
    mem.poke(MOD_BASE + 0x100, &50i32.to_le_bytes());

    let e = entry(
        "HP",
        Locator::ModuleOffset {
            module: "game.exe".into(),
            offset: 0x100,
        },
        Some(ScanValue::I32(9999)),
    );

    assert_eq!(e.read_current(&mem), Some(ScanValue::I32(50)));
    e.apply_desired(&mem).unwrap();
    assert_eq!(e.read_current(&mem), Some(ScanValue::I32(9999)));
}

#[test]
fn absolute_locator_reads_back() {
    let mem = MockMemory::new(MOD_BASE, 0x1000);
    mem.poke(MOD_BASE + 0x20, &7i32.to_le_bytes());
    let e = entry("XP", Locator::Absolute(MOD_BASE + 0x20), None);
    assert_eq!(e.read_current(&mem), Some(ScanValue::I32(7)));
}

#[test]
fn pointer_chain_resolves() {
    // Layout in a module-based buffer:
    //   [MOD_BASE + 0x10]  holds a pointer -> MOD_BASE + 0x200
    //   final address = (MOD_BASE + 0x200) + 0x8 holds the target value 1234
    let mem = MockMemory::new(MOD_BASE, 0x1000).with_module("game.exe", MOD_BASE);
    mem.poke(MOD_BASE + 0x10, &(MOD_BASE + 0x200).to_le_bytes());
    mem.poke(MOD_BASE + 0x208, &1234i32.to_le_bytes());

    let e = entry(
        "Gil",
        Locator::Pointer {
            module: "game.exe".into(),
            base_offset: 0x10,
            offsets: vec![0x8],
        },
        None,
    );
    assert_eq!(e.read_current(&mem), Some(ScanValue::I32(1234)));
}

#[test]
fn missing_module_does_not_resolve() {
    let mem = MockMemory::new(MOD_BASE, 0x100); // no modules registered
    let e = entry(
        "HP",
        Locator::ModuleOffset {
            module: "ghost.exe".into(),
            offset: 0,
        },
        Some(ScanValue::I32(1)),
    );
    assert_eq!(e.read_current(&mem), None);
    assert!(e.apply_desired(&mem).is_err());
}

#[test]
fn add_assigns_incrementing_ids_and_remove_works() {
    let mut table = CheatTable::new();
    let id1 = table.add(entry("HP", Locator::Absolute(1), None));
    let id2 = table.add(entry("MP", Locator::Absolute(2), None));
    assert_ne!(id1, id2);
    assert_eq!(table.entries.len(), 2);
    assert!(table.remove(id1));
    assert!(!table.remove(id1));
    assert_eq!(table.entries.len(), 1);
}

#[test]
fn json_round_trip_preserves_entries_and_bumps_next_id() {
    let mut table = CheatTable::new();
    table.game_hint = "Some Single-Player RPG".into();
    let id = table.add(entry(
        "HP",
        Locator::ModuleOffset {
            module: "game.exe".into(),
            offset: 0x1234,
        },
        Some(ScanValue::I32(9999)),
    ));

    let json = table.to_json().unwrap();
    let mut loaded = CheatTable::from_json(&json).unwrap();

    assert_eq!(loaded.entries.len(), 1);
    assert_eq!(loaded.entries[0].label, "HP");
    assert_eq!(loaded.entries[0].id, id);

    // The rebuilt id counter must not collide with the loaded entry.
    let new_id = loaded.add(entry("MP", Locator::Absolute(0), None));
    assert!(new_id > id);
}

#[test]
fn rejects_unknown_format_version() {
    let json = r#"{"version":9999,"game_hint":"","entries":[]}"#;
    let err = CheatTable::from_json(json).unwrap_err();
    assert!(matches!(
        err,
        memgene_core::error::TableError::Version { found: 9999, .. }
    ));
}

#[test]
fn tick_frozen_only_writes_frozen_entries() {
    let mem = MockMemory::new(MOD_BASE, 0x1000).with_module("game.exe", MOD_BASE);
    mem.poke(MOD_BASE + 0x10, &1i32.to_le_bytes());
    mem.poke(MOD_BASE + 0x20, &1i32.to_le_bytes());

    let mut table = CheatTable::new();
    let frozen = TableEntry {
        frozen: true,
        ..entry(
            "frozen",
            Locator::ModuleOffset {
                module: "game.exe".into(),
                offset: 0x10,
            },
            Some(ScanValue::I32(777)),
        )
    };
    table.add(frozen);
    table.add(entry(
        "not-frozen",
        Locator::ModuleOffset {
            module: "game.exe".into(),
            offset: 0x20,
        },
        Some(ScanValue::I32(777)),
    ));

    let errs = table.tick_frozen(&mem);
    assert!(errs.is_empty());

    let read = |off: u64| {
        let mut b = [0u8; 4];
        mem.read(MOD_BASE + off, &mut b).unwrap();
        i32::from_le_bytes(b)
    };
    assert_eq!(read(0x10), 777); // frozen entry was enforced
    assert_eq!(read(0x20), 1); // untouched
}
