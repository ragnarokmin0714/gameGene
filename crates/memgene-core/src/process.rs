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
}
