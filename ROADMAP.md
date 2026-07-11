# Roadmap

Planned work, so a future version bump can be decided from one document.
Shipped changes are recorded in [CHANGELOG.md](CHANGELOG.md), not here.

## Next: finish the memory-editing story (item / move lists)

The memory viewer (hex view) landed first, and **structure dissection /
array-stride detection shipped in 0.7.0** (the "Array" window: detect the
record size, lay the array out as rows, infer Int32/Float fields, add cells to
the table). What remains to make bulk item/move editing practical:

- **Fill / repeat writer.** Write a pattern across a detected array in one go:
  a fixed value, an incrementing id (`start, step, count`), or copy-one-slot to
  all. This is GameGene's stand-in for Cheat Engine's Lua scripting for mass
  edits. **Must** preview the exact addresses + bytes before writing, cap the
  count, require confirmation, and back up the original bytes so it is
  reversible.

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
