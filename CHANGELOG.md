# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [Semantic Versioning](https://semver.org/) (0.x: minor bumps may include
breaking changes).

## [0.3.0]

### Added
- **Pointer scan.** "Pin" a cheat-table entry to search for a pointer path from
  a static module base to its address; the locator is rewritten as that pointer
  chain so the entry keeps working after the game restarts.

### Changed
- Faster, lighter scanning: `next_scan` on a candidate list now coalesces nearby
  addresses into block reads instead of one syscall per address, and an "unknown
  initial value" first scan stores a compact byte snapshot instead of a struct
  per address (no longer blows up to many GB on large targets).

### Fixed
- Status/attachment labels no longer show a "tofu" box for the leading check
  mark; indicators now rely on colour plus a widely-supported bullet.

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
