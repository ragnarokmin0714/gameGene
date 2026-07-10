//! Linux backend using `/proc/<pid>/maps` and `/proc/<pid>/mem`.
//!
//! Reading another process requires the same uid (and a permissive
//! `ptrace_scope`) or `CAP_SYS_PTRACE`; writing has the same constraints. This
//! backend exists mainly so the whole engine can be tested against a real OS on
//! a Linux dev box — the shipping target is Windows.

use super::ProcessInfo;
use gamegene_core::{MemError, MemoryRegion, MemorySource};
use std::fs;
use std::os::unix::fs::FileExt;
use std::path::Path;

pub fn list_processes() -> Vec<ProcessInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if !comm.is_empty() {
            out.push(ProcessInfo { pid, name: comm });
        }
    }
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

pub fn attach(pid: u32) -> Result<Box<dyn MemorySource>, MemError> {
    LinuxProcess::open(pid).map(|p| Box::new(p) as Box<dyn MemorySource>)
}

struct LinuxProcess {
    pid: u32,
    mem: fs::File,
}

impl LinuxProcess {
    fn open(pid: u32) -> Result<Self, MemError> {
        let mem = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/proc/{pid}/mem"))
            // Fall back to read-only if we lack write permission; scanning
            // still works, only edits will fail with a clear error later.
            .or_else(|_| fs::File::open(format!("/proc/{pid}/mem")))?;
        Ok(LinuxProcess { pid, mem })
    }
}

impl MemorySource for LinuxProcess {
    fn regions(&self) -> Vec<MemoryRegion> {
        let Ok(maps) = fs::read_to_string(format!("/proc/{}/maps", self.pid)) else {
            return Vec::new();
        };
        maps.lines().filter_map(parse_maps_line).collect()
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> Result<usize, MemError> {
        self.mem.read_at(buf, addr).map_err(|e| MemError::Read {
            addr,
            reason: e.to_string(),
        })
    }

    fn write(&self, addr: u64, data: &[u8]) -> Result<(), MemError> {
        self.mem
            .write_all_at(data, addr)
            .map_err(|e| MemError::Write {
                addr,
                reason: e.to_string(),
            })
    }

    fn module_base(&self, name: &str) -> Option<u64> {
        let maps = fs::read_to_string(format!("/proc/{}/maps", self.pid)).ok()?;
        for line in maps.lines() {
            let path = line.split_whitespace().nth(5).unwrap_or("");
            if Path::new(path).file_name().and_then(|n| n.to_str()) == Some(name) {
                let start = line.split('-').next()?;
                return u64::from_str_radix(start, 16).ok();
            }
        }
        None
    }
}

/// Parse one `/proc/<pid>/maps` line into a readable region, or `None` to skip.
fn parse_maps_line(line: &str) -> Option<MemoryRegion> {
    let mut parts = line.split_whitespace();
    let range = parts.next()?;
    let perms = parts.next()?;
    if !perms.starts_with('r') {
        return None; // unreadable mapping — nothing to scan
    }
    let (start, end) = range.split_once('-')?;
    let base = u64::from_str_radix(start, 16).ok()?;
    let end = u64::from_str_radix(end, 16).ok()?;
    Some(MemoryRegion {
        base,
        size: end.saturating_sub(base),
        writable: perms.as_bytes().get(1) == Some(&b'w'),
    })
}
