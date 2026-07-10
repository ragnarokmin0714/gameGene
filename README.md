# GameGene

A memory scanner and value editor for **single-player games** — find a value
(HP, MP, EXP, currency, item count), pin down its address, and edit or freeze
it. Save what you find to a *cheat table* so you never have to rescan.

> **Scope & ethics.** GameGene is for single-player / offline games and your own
> processes, on your own machine. Using it against online or multiplayer games
> violates their terms of service, trips anti-cheat, and affects other players.
> Don't. See [Legal & ethical notes](#legal--ethical-notes).

---

## How it's built

GameGene is a Cargo workspace of three crates, each with one job:

| Crate | Role | Contents | OS-dependent? |
|-------|------|----------|---------------|
| **`gamegene-core`** | The brain (rules) | Value types, the scan engine (first scan → next scan), the cheat table, and the `MemorySource` trait. | No — pure logic, unit-tested on any platform. |
| **`gamegene-platform`** | The hands (touching the OS) | Implementations of `MemorySource`: Windows (`ReadProcessMemory`/`WriteProcessMemory`), Linux (`/proc`), and a mock. The right one is chosen at compile time via `cfg`. | Yes — all `unsafe` syscalls live here. |
| **`gamegene-app`** | The face (UI) | The egui desktop app (`gamegene` binary) that wires core + platform together. | Through the two crates above. |

The key idea: **`gamegene-core` never calls the operating system.** It talks to
the world only through the `MemorySource` trait. That's what makes the scan
logic testable without a real game, and what keeps the risky platform code
quarantined in one place. Adding a new OS means adding one file in
`gamegene-platform`; core doesn't change.

```
  gamegene-app        ──  egui desktop UI; the binary you launch
       │  calls
       ▼
  gamegene-core       ──  scan engine, cheat table, value types;
       ▲                 defines the MemorySource trait; no OS calls
       │  implements the trait
  gamegene-platform   ──  Windows / Linux / mock; the real syscalls
```

---

## Building

Prerequisites: a recent Rust toolchain via [rustup](https://rustup.rs).

```sh
# from the workspace root
cargo build --release        # release binary at target/release/gamegene[.exe]
cargo test  --workspace      # run the engine + cheat-table + backend tests
```

GameGene targets **64-bit** processes.

---

## Using it per platform

### Windows 11 (primary target)

1. `cargo build --release` → `target\release\gamegene.exe`.
2. **Run as Administrator.** Attaching to another process needs the
   `PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION` rights, which a
   normal user token may not grant against a game.
3. Launch your single-player game, then in GameGene: pick the process in the
   left list → **Attach**.
4. Scan → change the value in-game → **Next scan** to narrow → edit or freeze.

Caveats:

- **Windows Defender / SmartScreen may flag it as a `HackTool`/PUA.** That is
  expected for anything that reads and writes another process's memory; it is
  not a virus. You may need to allow it.
- **Anti-tamper (e.g. Denuvo) on some AAA titles** does integrity checks that
  can revert writes or make addresses hard to locate. This is a difficulty
  wall, not a legal one, and it only matters for protected big-budget games —
  simpler single-player titles behave like the examples above.

### SteamOS / Steam Deck

The important nuance: most Windows games on the Deck (e.g. FF7 Rebirth) run
through **Proton (Wine)**. From Linux's point of view that game is still just a
**normal Linux process**, and its memory lives in that process's address space —
so the Linux `/proc` backend can scan it.

1. Switch to **Desktop Mode** (KDE Plasma) — GameGene is a windowed GUI.
2. Install Rust with rustup. SteamOS's system partition is read-only, but
   rustup and Cargo install under `~/.cargo` in your home directory, which is
   writable, so no `steamos-readonly disable` is needed just to build.
   (Building the GUI may need X11/Wayland dev headers; if a build fails on
   missing system libraries, install them or build on another Linux box and
   copy the binary.)
3. `cargo build --release`, run `./target/release/gamegene`.
4. Attach to the game process. Under Proton it's usually the game's `.exe`
   running under `wine`/`pv-bwrap`; the Windows PE modules show up in
   `/proc/<pid>/maps`, so module-relative locators can still resolve.

Permissions: reading/writing another process needs ptrace access. Same-user
processes usually work; if attach fails, check `kernel.yama.ptrace_scope`
(`sysctl kernel.yama.ptrace_scope`) or run with `sudo`.

### Other Linux

Same as above, minus Proton — attach directly to a native Linux game process
via the `/proc` backend. The included `linux_selftest` integration test scans
and edits the test process's own memory to prove the backend end-to-end.

### macOS

Not supported yet: the backend returns an empty process list. Adding support
means implementing `MemorySource` with `mach_vm_read_overwrite` /
`mach_vm_write` in `gamegene-platform` (and code signing / entitlements to attach
to other processes).

---

## The cheat table (so you never rescan)

Once a scan pins down an address, click **＋ Table** to save it. A table entry
has a label, a value type, a *locator*, and an optional value to write or
freeze. Tables save/load as `.ggtable` JSON files.

A locator says *how to find the address again*:

- **Absolute** — a fixed address. Only valid for the current game run, because
  the OS relocates everything (ASLR) on restart.
- **Module + offset** — `game.exe + 0x1234`. Survives a restart as long as the
  module loads at a resolvable base.
- **Pointer chain** — `[[[game.exe+base] + o1] + o2] + o3`. The robust option:
  it re-derives the address by following pointers, surviving restarts even when
  the value's allocation moves.

Click **Pin** on a table entry to run a *pointer scan*: GameGene searches for a
pointer path from a static module base to the entry's current address and, if it
finds one, rewrites the locator as that pointer chain automatically — turning a
one-session absolute address into one that survives restarts.

So the "avoid rescanning" workflow is: find it once → **Pin** it → next session,
**Load** the table and **Apply** or **Freeze** without scanning again.

---

## Legal & ethical notes

- **Single-player / offline only.** Editing values in a game you own, running on
  your own machine, affecting only your own save, is equivalent to a built-in
  cheat code. That is GameGene's intended use.
- **Do not use it on online or multiplayer games.** It breaks their terms of
  service, trips anti-cheat (bans), and can affect the shared server state and
  other players — regardless of how harmless your intent is.
- Loading a cheat table from an untrusted source lets it write attacker-chosen
  values into the process you attach. Only load tables you trust.

## License

MIT.
