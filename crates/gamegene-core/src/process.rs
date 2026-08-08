//! The boundary between the platform-independent engine and the OS.
//!
//! Everything above this trait (scanning, the cheat table) is pure logic and
//! runs on any platform, which is what makes it unit-testable without a real
//! game running. The platform crate provides the real Windows/Linux
//! implementations; tests use [`crate::mock::MockMemory`].

use crate::error::MemError;

/// A contiguous run of the target's address space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryRegion {
    /// Start address in the target process.
    pub base: u64,
    /// Length in bytes.
    pub size: u64,
    /// Whether the region is writable (candidate for edits).
    pub writable: bool,
}

/// A loaded module (executable / library) image in the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    pub name: String,
    pub base: u64,
    pub size: u64,
}

impl ModuleInfo {
    /// Whether `addr` falls within this module's image — i.e. is a "static"
    /// address that keeps a stable offset from the module base across restarts.
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base.saturating_add(self.size)
    }
}

/// Read/write access to another process's memory, plus module lookup.
///
/// Implementations must be safe to call from the scan engine with `&self`;
/// backends that need interior mutation use their own synchronization.
pub trait MemorySource: Send + Sync {
    /// Regions worth scanning (committed, readable, non-guarded).
    fn regions(&self) -> Vec<MemoryRegion>;

    /// Read into `buf`, returning how many bytes were actually read.
    ///
    /// A short read (fewer bytes than requested) is normal near the end of a
    /// region or across an unreadable page and must not be treated as an error.
    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<usize, MemError>;

    /// Write `data` at `addr`.
    fn write(&self, addr: u64, data: &[u8]) -> Result<(), MemError>;

    /// Base address of a loaded module (e.g. `"game.exe"`), if present.
    ///
    /// Used to turn a saved module+offset locator back into a live address
    /// after the game restarts and ASLR moves everything.
    fn module_base(&self, _name: &str) -> Option<u64> {
        None
    }

    /// All loaded modules, used by the pointer scanner to recognize static
    /// anchor addresses. Defaults to empty (no module info available).
    fn modules(&self) -> Vec<ModuleInfo> {
        Vec::new()
    }
}

/// One OS page. Reads are cut at page boundaries so an unreadable page costs a
/// page rather than the whole request.
const PAGE: u64 = 4096;

/// Fill as much of `buf` as the source will give, starting at `addr`, and return
/// how many leading bytes were filled.
///
/// `ReadProcessMemory` fails **atomically**: ask for 256 bytes 200 bytes before
/// the end of a region and it returns nothing at all, not the 200 that are
/// there. A single call is therefore the wrong shape for anything reading a
/// fixed-size window at a user-chosen address — the memory viewer showed a whole
/// page as unreadable at a region edge, and structure dissection gave up
/// entirely. Reading forward a page at a time and stopping at the first failure
/// yields the longest readable prefix instead.
///
/// Bytes past the returned length are left untouched, so callers must not read
/// them.
pub fn read_prefix(src: &dyn MemorySource, addr: u64, buf: &mut [u8]) -> usize {
    let mut filled = 0usize;
    while filled < buf.len() {
        let at = addr.saturating_add(filled as u64);
        // Stop at the next page boundary, so a bad page cannot poison a read
        // that would otherwise have succeeded.
        let to_page = PAGE - (at % PAGE);
        let want = to_page.min((buf.len() - filled) as u64) as usize;
        match src.read(at, &mut buf[filled..filled + want]) {
            Ok(got) if got > 0 => {
                filled += got;
                if got < want {
                    break; // short read: a gap starts here
                }
            }
            _ => break,
        }
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockMemory;

    #[test]
    fn read_prefix_returns_the_readable_part_at_a_region_edge() {
        // A 100-byte region: asking for 256 must yield the 100 that exist.
        let base = 0x10_000u64;
        let mem = MockMemory::new(base, 100);
        mem.poke(base + 96, &[1, 2, 3, 4]);
        let mut buf = [0u8; 256];
        let n = read_prefix(&mem, base, &mut buf);
        assert_eq!(n, 100);
        assert_eq!(&buf[96..100], &[1, 2, 3, 4]);
    }

    #[test]
    fn read_prefix_of_an_unreadable_address_is_empty() {
        let mem = MockMemory::new(0x10_000, 64);
        let mut buf = [0u8; 16];
        assert_eq!(read_prefix(&mem, 0x1_000, &mut buf), 0);
    }
}
