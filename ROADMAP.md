# Roadmap

Planned work, so a future version bump can be decided from one document.
Shipped changes are recorded in [CHANGELOG.md](CHANGELOG.md), not here.

## Memory-editing story (item / move lists) — shipped

The pieces for bulk item/move editing are now in: the memory viewer (hex view),
**structure dissection / array-stride detection (0.7.0)**, and the **fill /
repeat writer (0.8.0)** — fixed value or incrementing id across a detected
array, with preview, count cap, and undo. Remaining niceties:

- Copy-one-slot-to-all fill (copy a whole record's bytes to every record), on
  top of the current per-field fixed/increment fills.

## Multi-value / group scan

- Search for several values that occur close together (Cheat Engine's "group"
  or "commonality" scan), e.g. find a region holding `100`, `50`, and `12`
  within N bytes — useful for locating a struct when you know several fields.

## Memory viewer enhancements

- In-grid byte editing (type over a cell), not only the write box.
- Highlight values that are valid pointers; "follow pointer" to jump there.
- Scrollable / resizable window beyond the fixed 256-byte page.

## Performance

- Parallelize the first scan across regions (rayon) and vectorize the compare
  loop (SIMD) — the single-threaded hot loop is the main remaining speed win.
- Retry unreadable reads at page granularity instead of skipping a whole chunk.

## Platform / robustness

- Run the pointer scan on a background thread so the UI does not freeze on
  large targets.
- macOS backend (`mach_vm_read_overwrite` / `mach_vm_write`) behind the
  existing `MemorySource` trait.
- Compare integers in their native type rather than via `f64` (removes the
  precision caveat for 64-bit magnitudes above 2^53).

## Branding

- Finalize the logo (candidates under `assets/options/`) and wire the chosen
  mark into the window icon and README.
