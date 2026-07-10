//! Byte-pattern and text search — a locate tool, complementary to the numeric
//! scanner.
//!
//! Numeric scanning finds a value you can edit; this finds *where something is*
//! by its bytes or text — e.g. an item's name string, to home in on the
//! inventory structure, or a known byte signature (AOB). Wildcards let a
//! signature skip bytes that vary (`4A ?? 3C 90`).
//!
//! Results are addresses; from one you add a numeric entry to the cheat table
//! and edit the value (an item ID, a count, …) as usual.

use crate::constants::SCAN_CHUNK_SIZE;
use crate::process::MemorySource;

/// One byte of a search pattern. `None` is a wildcard matching any byte.
pub type Pattern = Vec<Option<u8>>;

/// Text encoding to search for. Windows games are usually UTF-16 (little
/// endian); many cross-platform ones use UTF-8/ASCII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
}

/// Build an exact (no-wildcard) pattern from text in the given encoding.
pub fn text_pattern(text: &str, encoding: TextEncoding) -> Pattern {
    match encoding {
        TextEncoding::Utf8 => text.as_bytes().iter().map(|b| Some(*b)).collect(),
        TextEncoding::Utf16Le => text
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .map(Some)
            .collect(),
    }
}

/// Parse an "array of bytes" string like `4A ?? 3C 90` (whitespace-separated
/// hex bytes; `?` or `??` is a wildcard) into a [`Pattern`].
pub fn parse_aob(text: &str) -> Result<Pattern, String> {
    let mut pattern = Pattern::new();
    for tok in text.split_whitespace() {
        if tok == "?" || tok == "??" {
            pattern.push(None);
        } else {
            let byte = u8::from_str_radix(tok, 16)
                .map_err(|_| format!("`{tok}` is not a hex byte or wildcard"))?;
            pattern.push(Some(byte));
        }
    }
    if pattern.is_empty() {
        return Err("pattern is empty".to_string());
    }
    Ok(pattern)
}

/// Whether `pattern` matches at the start of `window` (which must be at least
/// `pattern.len()` bytes).
fn matches_at(pattern: &Pattern, window: &[u8]) -> bool {
    pattern
        .iter()
        .zip(window)
        .all(|(p, b)| p.is_none_or(|want| want == *b))
}

/// Find up to `max_results` addresses where `pattern` occurs, scanning every
/// readable region unaligned (step 1). Reads overlap by `pattern.len() - 1` so
/// a match straddling a chunk boundary is not missed.
pub fn find_pattern(source: &dyn MemorySource, pattern: &Pattern, max_results: usize) -> Vec<u64> {
    let plen = pattern.len();
    let mut hits = Vec::new();
    if plen == 0 || max_results == 0 {
        return hits;
    }
    let mut buf = vec![0u8; SCAN_CHUNK_SIZE];

    for region in source.regions() {
        let mut offset = 0u64;
        while offset < region.size {
            let want = ((region.size - offset) as usize).min(SCAN_CHUNK_SIZE);
            let read_addr = region.base + offset;
            let got = match source.read(read_addr, &mut buf[..want]) {
                Ok(n) => n,
                Err(_) => {
                    offset += want as u64;
                    continue;
                }
            };
            if got < plen {
                offset += want.max(1) as u64;
                continue;
            }
            for i in 0..=got - plen {
                if matches_at(pattern, &buf[i..i + plen]) {
                    hits.push(read_addr + i as u64);
                    if hits.len() >= max_results {
                        return hits;
                    }
                }
            }
            // Overlap the next window so boundary-spanning matches are caught,
            // unless this was a short read (a gap follows — skip past it).
            if got < want {
                offset += want as u64;
            } else {
                offset += (got - (plen - 1)) as u64;
            }
        }
    }
    hits
}
