# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [Semantic Versioning](https://semver.org/) (0.x: minor bumps may include
breaking changes).

## [Unreleased]

### Changed
- **Save defaults to the game's name.** The save-table dialog now pre-fills
  the file name from the attached process (e.g. `eldenring.ggtable`) instead
  of the generic `GameGene.ggtable`, so tables land beside the game they
  belong to. A trailing `.exe` is dropped and awkward characters are cleaned;
  with nothing attached it falls back to the app name.

## [0.16.1]

### Fixed
- **Memory viewer "Go" now re-selects the jumped-to byte.** Before, only the
  window moved: the old selection (often outside the new window) lingered, so
  the inspector never followed the jump and the address looked "unannotated".
- **Array dissection recognizes whole-number floats.** The field-type
  heuristic required a fractional part, so a column of values like 90.0 —
  exactly what game floats look like (HP, speed, angles) — was typed Int32
  and displayed as 1119092736. Whole-number floats of sane magnitude now
  infer as Float. (Note: a float's *raw bytes* legitimately start with `00`s
  — 90.0f32 is `00 00 B4 42` little-endian; the inspector's Float row is the
  place to read it.)

## [0.16.0]

### Changed
- Group scan now shows the **same determinate progress bar** as the value scan
  — its total spans every value's sweep, so the bar fills 0→100% across the
  whole scan. The rescan stays indeterminate (it only reads a small window
  around each anchor).

### Fixed
- **Group scan with a repeated or decoy value is reliable now.** Rescan
  re-searches around each anchor instead of trusting the single nearest partner
  the first scan recorded, so a real group is no longer dropped when an
  unrelated occurrence of a value happened to sit closer. Covers `[33 30] ->
  [33 34]`, `[20 20] -> [21 20]`, and the decoy case, each with a test.
- Value and group scans **auto-reset when a scan finds 0 matches**, so First
  scan is usable again immediately without a manual Reset.
- Traditional Chinese: the fill "Step" label reads 級距 (was 步進, which is
  really only idiomatic in 步進馬達 / stepper motor).
- CI: reworded a test doc comment whose line began with `+`, which clippy 1.97
  (`doc_lazy_continuation`) parsed as a Markdown list item and rejected under
  `-D warnings` — this had broken the 0.15.0 build.

## [0.15.0]

### Added
- The array / structure view shows records on **both sides** of the base
  address — the base is centred and highlighted (accent colour + tinted row),
  so you can see the records before it, not only after.
- Clicking an array cell now opens an **"Add to table?"** confirmation with an
  editable name (Enter confirms), instead of adding to the table immediately.
- **Group scan runs on a background thread** with a progress indicator and a
  Cancel button, plus a **Reset** button; first scan locks after it runs (like
  the value scan) until Reset, so an accidental click can't wipe results.

### Fixed
- **Group scan with a repeated value** (e.g. `[30 30]`) now pairs two *distinct*
  nearby addresses instead of matching a single value with itself. HP and MP
  both at 30 are found as a group; a lone 30 with no partner nearby no longer
  produces a false hit.
- Array / structure control-bar labels (`0x` / Stride / Rows) are vertically
  centred; a bare label used to top-align against the taller inputs and
  buttons, sitting noticeably high. The same fix is applied to the
  memory-viewer bar and the new confirmation dialog.

## [0.14.0]

### Added
- **Scans run on a background thread** with a progress bar and a Cancel button.
  A first/next scan over a multi-GB game no longer freezes the window, and
  frozen table entries keep being re-written while a scan runs.
- First scan stops at a candidate cap (10M) instead of exhausting memory on a
  too-common value (e.g. scanning for `0`), reporting that the value is too
  common to narrow.

### Changed
- **Faster scanning.** The inner loop is specialized per value type and
  compares raw native values, building a `ScanValue` only for actual matches;
  regions are scanned in parallel across all cores (aligned work splitting, so
  results are identical to the old serial scan). The byte/text finder anchors
  on the first concrete byte and uses SIMD (`memchr`) to skip between
  candidates.

### Fixed
- 64-bit integer comparisons (`>`, `<`, ranges) are now exact past 2^53.
  Ordering previously went through `f64`, which could not distinguish very
  large `i64`/`u64` values.

## [0.13.0]

### Added
- **Group scan with float ranges**: when the value type is Float/Double, each
  entered value matches a `v…v+1` range — the way to find values a HUD shows
  as "12" that are really 12.37 in memory, where an exact match can never hit.
  The interpreted ranges preview live under the input as you type, and
  "Next scan" narrows with ranges too.
- **Offsets column in group results**: each hit now shows where every other
  value sits relative to the anchor, with its live value (hover for full
  numbers and absolute addresses). This is the struct layout at a glance —
  previously the other matches were found but never shown, and Dissect (built
  for repeating arrays, not a single struct) couldn't reveal them.
- Group input ignores brackets and parentheses, so a pasted `[ 100 50 12 ]`
  parses fine.
- Headless layout test asserting toolbar controls share one height and
  centreline, and grid rows centre labels against their cells.

### Changed
- One standard control height across the UI: text inputs now match button
  height, and mixed small/full-size buttons in the same row were unified
  (memory viewer address bar and inspector, process panel Detach). Small
  buttons remain only inside dense grid rows and as heading chips.
- Group-scan exact matches align to the value size (struct fields are
  aligned), reducing false positives from overlapping byte patterns.
- Windows attach asks for read-only access when full access is denied, so
  protected processes can still be scanned; edits then fail with a clear
  error, matching the Linux backend.

### Fixed
- Linux: module paths containing spaces (common under Proton, e.g.
  `.../steamapps/common/Game Name/Game.exe`) now parse correctly from
  `/proc/<pid>/maps`, so module-relative locators resolve for those games.

## [0.12.0]

### Fixed
- Memory viewer and Array / structure windows are clamped inside the app
  window every frame. Window sizes are remembered across restarts, so a size
  saved by an older version (whose layout auto-grew past the frame) used to
  come back after updating — bigger than the app window, with the bottom and
  the resize corner out of reach. Both windows also gained a minimum size, and
  their control bars wrap at narrow widths instead of poking past the frame.
- Cheat table rows no longer shake when a watched value fluctuates — the live
  "now" value sits in a fixed-width cell (full value on hover), as the scan
  results and the memory-viewer inspector already did.

### Added
- **Japanese UI** (日本語), selectable next to English and 繁體中文.
- **Group rescan**: after a group scan, change the values in game, type the new
  numbers, and press "Next scan" to narrow the hits — each hit remembers where
  every value matched, so the whole group is re-checked, not just the first
  value. This is the before/after workflow users expected from the group tab.
- Headless layout regression tests for the viewer windows: window size must
  stay stable while a value fluctuates, resizing is clamped to the viewport,
  and nothing may paint past the frame.

### Changed
- `app.rs` (2000+ lines) split into focused modules — `app/{chrome, process,
  scan, table, memview, array}.rs` — and `i18n.rs` into one file per language
  (`i18n/{en, zh_hant, ja}.rs`), so adding a language or a panel touches one
  small file. No behavior change.

## [0.11.0]

### Fixed
- Memory viewer and Array / structure windows no longer spill their content
  past the window frame and can be resized freely. They dropped the nested
  panels (which made the window auto-grow to its content); a fixed control bar
  now sits above a single scroll area, so the window is a stable viewport and
  the body scrolls within it.
- Memory viewer no longer jitters when a shown value fluctuates — the window is
  now a fixed size instead of resizing to its content every frame.

### Changed
- Group scan hint clarified: it is a single simultaneous search of the values
  as they are *now* (not a before/after scan), and results list the first
  value's address — dissect or open it in the viewer to see the whole group.

## [0.10.0]

### Changed
- Scan panel now has **tabs**: "Value scan" (the usual type + exact/greater/
  between/… scan) and "Group scan". Group scan moved out of a cramped
  collapsible into its own tab with a full results list — double-click, right-
  click, or the button to add a hit to the table, same as normal results.
- Memory viewer and Array / structure windows now keep all their content inside
  one scroll area under a fixed control bar, so the windows resize freely
  without the inspector, fill controls, or grid spilling outside the frame.

### Fixed
- Array / structure cells truncate long values (e.g. floats with many decimals)
  so a single cell can't widen the grid; the full value shows on hover.

## [0.9.0]

### Added
- **Group scan** (in the scan panel): enter several values (e.g. `100 50 12`,
  interpreted as the selected Type) and a byte span, and it finds addresses of
  the first value that have all the others within that span — Cheat Engine's
  "group / commonality" scan, useful for locating a struct from known fields.

### Changed
- Memory viewer: the value inspector and the write / +Table / Dissect controls
  now sit directly under the address bar instead of at the very bottom, so
  editing is within easy reach; the hex grid fills the space below.

## [0.8.0]

### Added
- **Fill / bulk write** in the Array window: set one field across every record at
  once — a fixed value, or an incrementing integer (start + step). It always
  **previews** the exact addresses and bytes first, **caps** the count, and backs
  up the originals so **Undo** restores them. This completes the item/move
  bulk-editing story.

### Changed
- Float values now display with a decimal point (`100.0`, not `100`) so they read
  as floats.
- Scan results truncate long values (many decimals) to keep the column steady;
  the full value shows on hover.

### Fixed
- "Between" inputs are tidied before scanning: a missing upper bound is filled
  with lower + 1 (`11…` → `11…12`), and a reversed range is swapped
  (`28…11` → `11…28`).

## [0.7.0]

### Added
- **Array / structure dissection** (new "Array" window, also reachable from the
  memory viewer's "Dissect array"). Point it at one record's address and it
  detects the record size (stride) by looking for the memory's period, lays the
  array out as one row per record, and infers each field as Int32 or Float. The
  stride is editable, and clicking any cell adds it to the cheat table. This is
  the groundwork for bulk item / move editing (the fill/repeat writer is next).

### Fixed
- Memory viewer no longer shakes left-right when a shown value fluctuates: the
  interpreted-value column is a fixed width, so a value changing length can't
  resize the window each frame.

## [0.6.0]

### Added
- **Settings persist across restarts**: the chosen theme, language, and keyboard
  shortcuts are saved and restored (via eframe storage).
- **Editable keyboard shortcuts** (new "Settings" window): Detect game, Attach,
  Save, Load, toggle Memory, First scan, Next scan, and Reset. Click "Change" to
  rebind (Esc cancels) and "Reset to defaults" to restore them. Defaults are
  Ctrl + a letter (e.g. Ctrl+S save, Ctrl+M memory, Ctrl+G detect game).

### Changed
- Claude Dark uses a warmer, lighter charcoal background (no longer near-black)
  and cleaner near-white text (no more tan tint).
- Memory viewer inspector shows the raw hex plus Int32 and Float by default;
  "More types" expands to every type.

### Fixed
- Memory viewer no longer shakes when hovering a cell — hover/active states no
  longer expand widgets, so the grid stops reflowing under the cursor.

## [0.5.1]

### Added
- The Claude theme now also uses a serif typeface (bundled Liberation Serif,
  SIL OFL 1.1 — see `crates/gamegene-app/assets/serif.LICENSE.txt`), for a
  warmer, more editorial feel closer to Claude's own look.

### Changed
- Memory viewer layout: the address bar (top) and the inspector/editor (bottom)
  are now fixed, while the hex/ASCII grid in the middle scrolls both ways — a
  narrow window shows scrollbars instead of overflowing. Long readings (e.g. an
  `f64`) are truncated with the full value shown on hover.
- Release archives now include the version, e.g.
  `gamegene-0.5.1-linux-x86_64.tar.gz`.

## [0.5.0]

### Added
- **Claude theme.** Two extra options in the theme picker — "Claude" and
  "Claude Dark" — with a warm cream / terracotta palette alongside the existing
  Apple (System / Light / Dark) skins.
- The app version is shown next to the title and in the window title bar, and is
  recorded in saved cheat-table files (`app_version` field).
- Results list is now Cheat-Engine-like to work with: **double-click** a row to
  add it to the table, or **right-click** for a menu (add to table / open in the
  memory viewer).

## [0.4.1]

### Added
- Cheat-table entries now show their current memory address (in accent colour)
  with a "Memory" button that opens the memory viewer focused on that address,
  so entries are easy to tell apart when editing more than one.

### Fixed
- The results list no longer shakes when there are many matches: the live value
  column is a fixed width and the scroll area no longer auto-shrinks, so a value
  changing length can't reflow the grid each frame.
- "First scan" is now disabled once a scan is in progress — use "Reset" to start
  over — so an accidental click can't discard the narrowed results.
- Windows taskbar / Explorer now show the GameGene icon: the icon is embedded
  into the `.exe` as a resource at build time (`build.rs` + `winresource`),
  since the runtime window icon alone does not cover those.

## [0.4.0]

### Added
- **Memory viewer** (toggle "Memory"): a hex/ASCII grid over a windowed read of
  the target, live-refreshed. Click a byte to see it decoded as every value
  type, write a value at that address, or add it to the cheat table. Reads only
  the visible 256 bytes, so it is cheap. Planned follow-ups (structure
  dissection, fill tool) are in [ROADMAP.md](ROADMAP.md).

## [0.3.0]

### Added
- **Pointer scan.** "Pin" a cheat-table entry to search for a pointer path from
  a static module base to its address; the locator is rewritten as that pointer
  chain so the entry keeps working after the game restarts.
- **Find bytes / text.** A locate tool: search for an array of bytes (with `??`
  wildcards) or text (UTF-8 or UTF-16), e.g. an item's name, then add a found
  address to the cheat table and edit it.
- Brand icon (radar + gene-helix motif) as the window/taskbar icon and README
  logo, generated from `assets/make_icon.py`.

### Changed
- Faster, lighter scanning: `next_scan` on a candidate list now coalesces nearby
  addresses into block reads instead of one syscall per address, and an "unknown
  initial value" first scan stores a compact byte snapshot instead of a struct
  per address (no longer blows up to many GB on large targets).

### Fixed
- UI symbols no longer render as "tofu" boxes: the check mark, remove (×),
  arrow, and plus glyphs are replaced with widely-supported characters, and
  attach state is shown with colour plus a plain bullet.

## [0.2.1]

### Fixed
- "Detect game" no longer locks onto the Windows shell (`explorer.exe`) or
  system UI when switching windows — it keeps the real foreground game and now
  shows the detected target (name + pid) next to the button.

### Changed
- Releases are published with the GitHub CLI instead of a third-party action,
  removing the Node-20 deprecation warning and future maintenance.

## [0.2.0]

### Added
- English / Traditional Chinese language toggle, with a system CJK font loaded
  at startup so Chinese renders (Windows 微軟正黑體, Noto CJK on Linux).
- Running-time clock (HH:MM:SS) in the header.
- "Detect game" button that locks onto the current foreground window's process
  (Windows only; no-op elsewhere).

### Changed
- **Renamed the project from MemGene to GameGene** — crates (`gamegene-*`), the
  `gamegene` binary, the config directory, and the cheat-table extension
  (`.mgtable` → `.ggtable`). Tables saved by 0.1.0 keep the old extension.
- Attaching is now explicit: click selects a process (highlighted), then connect
  via an **Attach** button or double-click. Success/failure is shown clearly.
- The cheat-table save dialog now defaults the filename to `GameGene.ggtable`.

### Fixed
- Attach result was previously easy to miss (only a faint status line); it is
  now a prominent success/error indicator.

## [0.1.0]

### Added
- Memory scan engine: first scan and iterative next scan over Int/UInt/Float
  types, with exact / greater / less / between / unknown / changed / unchanged /
  increased / decreased predicates.
- Cheat table with absolute, module+offset, and pointer-chain locators; save and
  load as JSON, apply values, and freeze (continuously re-write).
- egui desktop UI with an Apple-flavored light/dark theme.
- Windows (`ReadProcessMemory`) and Linux (`/proc`) memory backends behind a
  common `MemorySource` trait, plus a mock backend for tests.
- CI (fmt, clippy, tests on Linux; build + test on Windows) and a tag-triggered
  release workflow that builds binaries for Windows and Linux.
