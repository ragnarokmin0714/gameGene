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

## Memory viewer enhancements

- In-grid byte editing (type over a cell), not only the write box.
- Highlight values that are valid pointers; "follow pointer" to jump there.
- Scrollable / resizable window beyond the fixed 256-byte page.

## Pointer scanning — revalidate across restarts

The multi-level pointer scanner (`pointer.rs`) already finds paths; the missing
half is Cheat Engine's *rescan* workflow that distils them down to stable ones.

- Revalidate a saved pointer-path list against a freshly restarted process:
  keep only the paths that still resolve to the target, drop the rest. Repeat
  across a few restarts until a handful of trustworthy paths remain.
- Pure `gamegene-core` logic over the `MemorySource` trait — testable against
  the mock, low risk. Good next-version candidate.

## Watchpoints — "find what accesses this address" (flagship)

Cheat Engine's most valuable discovery tool: watch an address and list the
instructions that read/write it, to find the struct base and pointer path.

Mechanism (Windows first): a hardware data breakpoint in a debug register
(DR0–DR3) raises `EXCEPTION_SINGLE_STEP`; catch it as a debugger via
`DebugActiveProcess` + a `WaitForDebugEvent` loop, and the faulting thread's
`RIP` is the accessing instruction. Most of the Win32 surface lives under the
already-enabled `Win32_System_Diagnostics_Debug`.

- **v1 (Windows-only, no disassembly):** a platform debug-event loop on its own
  thread; a `gamegene-core` watch abstraction streaming `WatchHit { rip,
  thread_id, hit_count }`; an app panel listing hits as `module+offset`. This
  alone delivers ~80% of the value.
- **v2 (CE-level):** disassemble the hit site (`iced-x86`), show operands /
  registers, and the reverse direction ("what addresses does this instruction
  touch").

Sharp edges to design for, not bolt on:

- Call `DebugSetProcessKillOnExit(FALSE)` or quitting GameGene kills the game.
- Program debug registers on every thread, including ones created later
  (`CREATE_THREAD_DEBUG_EVENT`); service *all* debug events or the target hangs.
- Breaks the "core is testable without a real game" property — this feature is
  effectively manual-test-only on a live Windows target.
- `DebugActiveProcess` is a stronger "hacking tool" signal than RPM/WPM: expect
  it to worsen the antivirus false-positive picture and to trip anti-debug
  protections (Denuvo / anti-cheat). Single-player, unprotected games only.
- Linux/Proton parity would need a separate `ptrace` + `POKEUSER` backend;
  Windows-first is an accepted asymmetry (CE is Windows-only too).

Heavy enough to be its own flagship release, not bundled with quality-of-life
work.

## Quality of life

- Global hotkeys to toggle a freeze / set a value without alt-tabbing out of the
  game (`RegisterHotKey` on Windows).
- Descriptions / comments on cheat-table entries so a saved table is
  self-documenting.

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
