//! Byte-pattern / text search tests.

use gamegene_core::find::{find_pattern, parse_aob, text_pattern, TextEncoding};
use gamegene_core::mock::MockMemory;

const BASE: u64 = 0x20000;

#[test]
fn finds_exact_byte_pattern() {
    let mem = MockMemory::new(BASE, 64);
    mem.poke(BASE + 10, &[0xDE, 0xAD, 0xBE, 0xEF]);
    let pat = parse_aob("DE AD BE EF").unwrap();
    assert_eq!(find_pattern(&mem, &pat, 16), vec![BASE + 10]);
}

#[test]
fn wildcards_match_any_byte() {
    let mem = MockMemory::new(BASE, 64);
    mem.poke(BASE + 4, &[0x4A, 0x11, 0x3C, 0x90]);
    let pat = parse_aob("4A ?? 3C 90").unwrap();
    assert_eq!(find_pattern(&mem, &pat, 16), vec![BASE + 4]);
}

#[test]
fn finds_utf16_text() {
    let mem = MockMemory::new(BASE, 128);
    // "Sword" as UTF-16LE, as a Windows game might store it.
    let bytes: Vec<u8> = "Sword"
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    mem.poke(BASE + 20, &bytes);
    let pat = text_pattern("Sword", TextEncoding::Utf16Le);
    assert_eq!(find_pattern(&mem, &pat, 16), vec![BASE + 20]);
}

#[test]
fn finds_utf8_text_and_respects_result_cap() {
    let mem = MockMemory::new(BASE, 128);
    mem.poke(BASE, b"HP");
    mem.poke(BASE + 40, b"HP");
    let pat = text_pattern("HP", TextEncoding::Utf8);
    // Two occurrences, but cap at 1.
    assert_eq!(find_pattern(&mem, &pat, 1), vec![BASE]);
    assert_eq!(find_pattern(&mem, &pat, 16), vec![BASE, BASE + 40]);
}

#[test]
fn rejects_bad_aob() {
    assert!(parse_aob("").is_err());
    assert!(parse_aob("ZZ").is_err());
    assert!(parse_aob("4A GG").is_err());
}
