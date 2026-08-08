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
- Show more than 16 rows at once on a tall window. The wheel now walks the
  address space by rows, so navigation is no longer page-bound; what remains
  is making the *number of visible rows* follow the window height.

## Pointer scanning — shipped, with one piece left

The scanner finds paths (`pointer.rs`) and **0.21.0 added the revalidation
workflow**: candidates open in a window, each restart drops the ones that no
longer reach the value's new address, and a survivor becomes the entry's
locator. Remaining:

- **Run the scan on a background thread.** It is still synchronous, so a large
  target freezes the UI with no progress and no cancel — more visible now that
  pinning opens a window instead of finishing silently. The `ScanJob` /
  `ScanControl` machinery the value and group scans use applies directly.
- Persist a candidate list between sessions, so narrowing can span days rather
  than one sitting. Needs a decision on where it lives: a cheat table records
  *findings*, and a list that has not survived a restart yet is not one.

## Name the fields — Mono metadata dissection

Structure dissection currently guesses: detect a stride, infer each column's type
from the numbers in it. For a Mono/Unity game the real answer is sitting on disk.
`Assembly-CSharp.dll` is ECMA-335 metadata — class names, field names, field
types, declaration order — and Mono's instance layout is computable from it. Read
it and the dissection view stops saying "field at +0x38, looks like Int32" and
starts saying `PlayerData.gold : ObscuredInt`.

This is the answer to values that cannot be scanned for. An obfuscated value
(Anti-Cheat Toolkit's `ObscuredInt` stores `hiddenValue ^ currentCryptoKey`)
never appears in memory in plain form, so no predicate finds it — but its key
sits in the adjacent field, and knowing the layout hands you both. The principle
generalizes: when the value is hidden, go after the structure.

### Decision: static metadata, not injection

Three ways to get this, and the choice matters enough to record:

- **A. Inject a DLL and call Mono's own API** — what Cheat Engine does
  (`MonoDataCollector.dll` + a named pipe, calling `mono_get_root_domain`,
  `mono_class_get_fields`, `mono_field_get_offset` in-process). Robust, because
  it uses the runtime's supported entry points. **Rejected**: process injection
  (`VirtualAllocEx` + `CreateRemoteThread`) is the textbook malware behaviour
  (ATT&CK T1055). It would take the antivirus story from "occasionally flagged
  by a static heuristic" — already documented in the README — to "blocked at
  runtime by Defender's ASR rules", and it puts OS calls at the heart of a
  feature, against the crate split.
- **B. Walk Mono's internal structures from outside via `ReadProcessMemory`** —
  no new AV surface, no injection. **Rejected**: `MonoDomain` / `MonoImage` /
  `MonoClass` are internal types with no stable ABI. Their layout shifts between
  Mono and Unity versions, so this means a per-version offset table to maintain
  forever, for a tool with no telemetry to tell us which versions are in use.
- **C. Parse the metadata statically, anchor the instance with the scanner** —
  **chosen.** Reading a file on disk adds *zero* new API calls, so the antivirus
  picture is unchanged. Parsing is pure logic with no OS calls, so it belongs in
  `gamegene-core` and is unit-testable against a checked-in fixture assembly with
  no game running — the property the whole crate split exists to protect.

The half C does not give you is "where is the instance", and that is the half
GameGene already has: scan for any field you *can* find, use it as an anchor, and
read the rest through the computed layout. In effect this is not a new feature so
much as teaching the existing dissection view the real names and types.

**Standing constraint: GameGene does not inject code into the target.** Read,
write, and (later) debug-register watchpoints are the ceiling. Anything needing
code to run inside the game is out of scope, whatever it would buy.

### Sharp edges

- **Field layout is the whole difficulty.** Mono's default is auto layout, which
  may reorder fields — declaration order from the metadata is not the memory
  order. Alignment, base-class fields coming first, `[StructLayout]` and
  `[FieldOffset]` overrides, and reference fields being pointer-sized all have to
  be reproduced exactly; one wrong offset skews the whole record.
- **Generics** instantiate per type argument and cannot be laid out from the
  metadata alone. Skipping generic classes in v1 is fine and should be explicit
  in the UI, not a silent wrong answer.
- **IL2CPP is a separate, easier path**: offsets are baked into
  `global-metadata.dat` plus the binary, so no layout rules to reproduce. Worth
  supporting, but as its own reader — not by pretending the two are one format.
- Verify against a known value before trusting the view: dissect a struct where
  one field's value is already known from a scan, and check it lands where the
  layout predicts.

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

## Save-file editing (`FileSource`)

Scan and edit a game's save file with the engine that already exists. A
`MemorySource` implementation backed by an mmap'd file — `regions()` returns the
one span, `read`/`write` hit the mapping — and first/next scan, structure
dissection, the hex viewer and the fill writer all work on a save with no change
to `gamegene-core`. This is the design's own claim ("core never calls the OS")
being cashed in, and it is the cheapest large feature on this list.

One workflow it unlocks that a live scanner cannot do at all: **diff two
saves.** Save, spend the money, save again, then run Decreased across the pair.
No animation, no timers, no per-frame scratch — the noise that makes an
unknown-value hunt in live memory so slow simply isn't there.

- Relative scans need a *pair* of files, not one: a second `MemorySource` as the
  "previous" side. That is the one real engine change, since `next_scan`
  currently re-reads the same source it scanned.
- Writes go straight to the file, so back it up first. An undo of the last
  write, like the fill writer already has, is the minimum.
- Table entries would need a locator that means "offset into this file" — a
  saved table should not confuse a file offset with a process address.

Scope this honestly: it works on saves that are **plain, uncompressed and
unchecksummed**. That covers a lot of indie games (bare JSON, bare binary) but
the tool cannot pretend to be general.

- **Checksums / hashes** are per-game; an edited save is rejected on load and
  there is no generic fix. Detecting *that* a save has a trailing hash is
  feasible; recomputing it is not.
- **Encrypted** saves (Unity ES3 in encrypted mode, custom XOR) have no
  plaintext to scan. Out of scope, and should say so rather than return junk.
- **Compressed** saves (RPG Maker's LZString, gzip) must be inflated first. A
  transparent gzip/zlib layer is plausible later; a per-engine format zoo is
  not.

The honest framing is "a hex editor with a scan engine over a save file", not a
save editor that knows any particular game.

## Quality of life

- Global hotkeys to toggle a freeze / set a value without alt-tabbing out of the
  game. **`RegisterHotKey` is the ceiling — if it is not enough, drop the
  feature.** A low-level keyboard hook (`SetWindowsHookEx(WH_KEYBOARD_LL)`) is
  the obvious next step when `RegisterHotKey` turns out to be too limited (few
  usable combinations, loses to a game holding exclusive input), and it is also
  the textbook keylogger API. Combined with what this tool already does —
  reading and writing another process's memory — it completes a malware
  profile that no amount of explaining will talk an antivirus out of. The
  limitation is the point, not an obstacle to route around.
- Descriptions / comments on cheat-table entries so a saved table is
  self-documenting. **Smaller than it looks: `TableEntry.notes` already exists
  and is serialized — only the UI to edit and show it is missing.**

## Performance

- **Vectorize the compare loop (SIMD).** The scan is already parallel across
  cores (`parallel_collect`, 0.14.0) and monomorphized per value type, so the
  remaining win is per-core throughput. The byte/text finder already gets this
  from `memchr`; the value scan does not.
- **Page-granular retry in the *scan* loop.** `read_prefix` (0.21.1) fixed the
  windowed readers and `find_near` (0.18.0) the group rescan, but
  `parallel_collect` still skips a whole 4 MiB chunk when a read fails, so a
  single bad page can cost every candidate behind it.

## Platform / robustness

- macOS backend (`mach_vm_read_overwrite` / `mach_vm_write`) behind the
  existing `MemorySource` trait.

## Branding

- Finalize the logo (candidates under `assets/options/`) and wire the chosen
  mark into the window icon and README.

## Non-goals

Things GameGene is deliberately not going to be. Recorded so the question does
not get reopened once per feature.

### Not a mod loader (no BepInEx / MelonLoader equivalent)

A runtime inspector — browse the live scene graph, edit fields by reflection,
call methods, click the debug panel the developer left in the build — is real
value that GameGene will not deliver, because every bit of it requires **code
running inside the game**:

- BepInEx gets in via Unity Doorstop: a native DLL beside the executable that
  the loader picks up by DLL search order, hooking `mono_jit_init_version` to
  load a managed assembly into the runtime. It is not remote injection
  (`CreateRemoteThread`), but DLL search-order hijacking (ATT&CK T1574.001) is
  just as hostile to antivirus heuristics, and it means writing files into the
  game's install directory.
- The inspector itself is a C# plugin. Building one means a second language and
  toolchain in a Rust project.
- It only attaches at launch. GameGene's attach-to-a-running-game model would
  not survive it.

The hard boundary this draws: **calling a method in the target is permanently
out of scope.** Reading and writing its memory is not.

Halfway is still worth having, and is on this list already — see *Name the
fields*, which gives field names and real types over plain RPM/WPM, no
injection. That covers the data half of what an inspector does.

BepInEx is mature, is the community standard, and is complementary rather than
competing: it needs a readable `Assembly-CSharp.dll`, so it covers Mono games
and nothing else. GameGene's ground is native C++ targets, IL2CPP, and
emulators — exactly where a mod loader has nothing to offer. Reimplementing it
would trade that ground for a weaker copy of someone else's tool.

### Not an anti-cheat bypass

Single-player, unprotected games only, as the README already states. Defeating
kernel-mode anti-cheat, anti-tamper, or anti-debug protection is not a feature
gap to be closed — it is outside what this tool is for, and the reason the
watchpoint entry accepts tripping those protections rather than working around
them.
