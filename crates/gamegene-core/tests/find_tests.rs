//! Byte-pattern / text search tests.

use gamegene_core::find::{
    find_pattern, parse_aob, preview, text_pattern, PreviewStyle, TextEncoding,
};
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

#[test]
fn preview_decodes_multibyte_text_rather_than_dotting_it() {
    // A CJK game's strings are multi-byte; rendering byte-by-byte would show
    // the most useful text as a row of dots.
    let utf8 = "金錢875".as_bytes();
    assert_eq!(
        preview(utf8, PreviewStyle::Text(TextEncoding::Utf8), 40),
        "金錢875"
    );
    let utf16: Vec<u8> = "金錢875"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(
        preview(&utf16, PreviewStyle::Text(TextEncoding::Utf16Le), 40),
        "金錢875"
    );
}

#[test]
fn preview_collapses_unreadable_bytes_and_respects_the_cap() {
    // Control bytes and invalid sequences must not break the single line.
    let raw = b"ab\x00\x01\xffcd";
    let out = preview(raw, PreviewStyle::Text(TextEncoding::Utf8), 40);
    assert_eq!(out, "ab···cd");
    assert!(!out.contains('\0'));
    // The cap counts characters, not bytes.
    assert_eq!(
        preview(
            "金錢875".as_bytes(),
            PreviewStyle::Text(TextEncoding::Utf8),
            3
        ),
        "金錢8"
    );
}

#[test]
fn preview_of_a_signature_stays_hex() {
    assert_eq!(
        preview(&[0x4A, 0x00, 0x3C], PreviewStyle::Hex, 8),
        "4A 00 3C"
    );
    assert_eq!(preview(&[0x4A, 0x00, 0x3C], PreviewStyle::Hex, 2), "4A 00");
}
