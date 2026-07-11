# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [Semantic Versioning](https://semver.org/) (0.x: minor bumps may include
breaking changes).

## [Unreleased]

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
