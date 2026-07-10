//! An in-memory [`MemorySource`] for tests and the desktop demo mode.
//!
//! It models a single writable region backed by a `Vec<u8>` plus a table of
//! named module bases, so the entire engine can be exercised — first scan, next
//! scan, pointer-chain resolution, freezing — with no real process involved.

use crate::error::MemError;
use crate::process::{MemoryRegion, MemorySource, ModuleInfo};
use std::sync::RwLock;

/// A fake process whose memory is a flat buffer starting at `base`.
pub struct MockMemory {
    base: u64,
    data: RwLock<Vec<u8>>,
    writable: bool,
    modules: Vec<ModuleInfo>,
}

impl MockMemory {
    /// Create with `size` zeroed bytes mapped at `base`.
    pub fn new(base: u64, size: usize) -> Self {
        MockMemory {
            base,
            data: RwLock::new(vec![0u8; size]),
            writable: true,
            modules: Vec::new(),
        }
    }

    /// Register a named module. The module spans the whole mapped buffer, which
    /// is enough for exercising locator resolution and pointer scanning.
    pub fn with_module(mut self, name: &str, base: u64) -> Self {
        let size = self.data.read().unwrap().len() as u64;
        self.modules.push(ModuleInfo {
            name: name.to_string(),
            base,
            size,
        });
        self
    }

    /// Register a module covering only `[base, base + size)`, so addresses
    /// outside it count as non-static (needed to exercise multi-hop pointer
    /// chains, where intermediate pointers live in "heap" memory).
    pub fn with_module_range(mut self, name: &str, base: u64, size: u64) -> Self {
        self.modules.push(ModuleInfo {
            name: name.to_string(),
            base,
            size,
        });
        self
    }

    /// Convenience: overwrite bytes at an absolute address (panics if OOB).
    pub fn poke(&self, addr: u64, bytes: &[u8]) {
        let start = (addr - self.base) as usize;
        let mut data = self.data.write().unwrap();
        data[start..start + bytes.len()].copy_from_slice(bytes);
    }
}

impl MemorySource for MockMemory {
    fn regions(&self) -> Vec<MemoryRegion> {
        let size = self.data.read().unwrap().len() as u64;
        vec![MemoryRegion {
            base: self.base,
            size,
            writable: self.writable,
        }]
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<usize, MemError> {
        let data = self.data.read().unwrap();
        if addr < self.base {
            return Err(MemError::Read {
                addr,
                reason: "below mapped range".into(),
            });
        }
        let start = (addr - self.base) as usize;
        if start >= data.len() {
            return Err(MemError::Read {
                addr,
                reason: "above mapped range".into(),
            });
        }
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn write(&self, addr: u64, bytes: &[u8]) -> Result<(), MemError> {
        let mut data = self.data.write().unwrap();
        if addr < self.base {
            return Err(MemError::Write {
                addr,
                reason: "below mapped range".into(),
            });
        }
        let start = (addr - self.base) as usize;
        if start + bytes.len() > data.len() {
            return Err(MemError::Write {
                addr,
                reason: "write would overrun mapped range".into(),
            });
        }
        data[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn module_base(&self, name: &str) -> Option<u64> {
        self.modules.iter().find(|m| m.name == name).map(|m| m.base)
    }

    fn modules(&self) -> Vec<ModuleInfo> {
        self.modules.clone()
    }
}
