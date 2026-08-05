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

/// How to render bytes for a one-line preview of a search hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewStyle {
    /// Decode as text in the encoding that was searched for.
    Text(TextEncoding),
    /// Space-separated hex — for byte signatures, where the bytes *are* the
    /// answer and decoding them as text would only obscure them.
    Hex,
}

/// Stand-in for a byte that has no readable character. A middle dot rather than
/// `.`, so it cannot be mistaken for a period that is genuinely in the text.
const NON_PRINTABLE: char = '·';

/// Render the bytes at a search hit as a single line of at most `max_chars`
/// characters, for showing beside the address in the results list.
///
/// A list of bare addresses cannot be triaged: a text search over a running game
/// returns every place the needle occurs, and the only way to tell the
/// interesting one from a dozen string constants is to look at what surrounds
/// it. Control and unpaired characters collapse to [`NON_PRINTABLE`] so one line
/// stays one line whatever the bytes hold.
pub fn preview(bytes: &[u8], style: PreviewStyle, max_chars: usize) -> String {
    match style {
        PreviewStyle::Hex => {
            let mut s = String::new();
            for b in bytes.iter().take(max_chars) {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&format!("{b:02X}"));
            }
            s
        }
        // Decoded properly rather than byte-by-byte: a CJK game's strings are
        // multi-byte, and a Latin-1 style gutter would render all of them as
        // dots — exactly the text you most want to read.
        PreviewStyle::Text(TextEncoding::Utf8) => String::from_utf8_lossy(bytes)
            .chars()
            .take(max_chars)
            .map(printable)
            .collect(),
        PreviewStyle::Text(TextEncoding::Utf16Le) => bytes
            .chunks_exact(2)
            .take(max_chars)
            .map(|c| {
                char::from_u32(u16::from_le_bytes([c[0], c[1]]) as u32)
                    .map_or(NON_PRINTABLE, printable)
            })
            .collect(),
    }
}

/// A character as it should appear in a preview line.
fn printable(c: char) -> char {
    if c.is_control() || c == char::REPLACEMENT_CHARACTER {
        NON_PRINTABLE
    } else {
        c
    }
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
///
/// Rather than test the pattern at every byte, we anchor on the first
/// *concrete* (non-wildcard) byte and use SIMD [`memchr`] to jump straight to
/// its next occurrence, only running the full compare there. For real
/// signatures — which almost always start with a concrete byte — this skips the
/// vast majority of positions.
pub fn find_pattern(source: &dyn MemorySource, pattern: &Pattern, max_results: usize) -> Vec<u64> {
    let plen = pattern.len();
    let mut hits = Vec::new();
    if plen == 0 || max_results == 0 {
        return hits;
    }
    // Index and value of the first concrete byte to anchor the search on.
    let anchor = pattern
        .iter()
        .position(|p| p.is_some())
        .map(|i| (i, pattern[i].unwrap()));
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
            let window = &buf[..got];
            let last_start = got - plen;
            match anchor {
                // Anchored: hop between occurrences of the anchor byte. A match
                // starting at `i` has the anchor byte at `i + ai`, so candidate
                // starts are `pos - ai`.
                Some((ai, aval)) => {
                    let mut from = ai; // earliest anchor position for start 0
                    while let Some(rel) = memchr::memchr(aval, &window[from..]) {
                        let pos = from + rel;
                        if pos >= ai {
                            let start = pos - ai;
                            if start <= last_start && matches_at(pattern, &window[start..]) {
                                hits.push(read_addr + start as u64);
                                if hits.len() >= max_results {
                                    return hits;
                                }
                            }
                        }
                        from = pos + 1;
                    }
                }
                // All-wildcard pattern (matches everywhere): emit every start.
                None => {
                    for start in 0..=last_start {
                        hits.push(read_addr + start as u64);
                        if hits.len() >= max_results {
                            return hits;
                        }
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
