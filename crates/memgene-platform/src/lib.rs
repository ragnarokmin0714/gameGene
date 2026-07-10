//! OS-specific implementations of [`memgene_core::MemorySource`].
//!
//! The rest of the app is written against the trait and this crate's small
//! [`list_processes`] / [`attach`] API; the actual syscalls live in per-OS
//! modules selected by `cfg`. Windows is the primary target; a Linux
//! (`/proc`) backend exists so the engine can be exercised end-to-end on a dev
//! machine, and any other OS degrades gracefully to "no processes".

use memgene_core::{MemError, MemorySource};

/// A process the user could attach to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as backend;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;

/// List candidate target processes, most-relevant first.
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn list_processes() -> Vec<ProcessInfo> {
    backend::list_processes()
}

/// Attach to a process by pid.
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn attach(pid: u32) -> Result<Box<dyn MemorySource>, MemError> {
    backend::attach(pid)
}

/// Fallback for unsupported platforms (e.g. macOS): nothing to attach to.
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn list_processes() -> Vec<ProcessInfo> {
    Vec::new()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn attach(_pid: u32) -> Result<Box<dyn MemorySource>, MemError> {
    Err(MemError::NotAttached)
}

/// The name of the OS backend compiled in, for display in the UI.
pub const BACKEND_NAME: &str = if cfg!(target_os = "windows") {
    "Windows (ReadProcessMemory)"
} else if cfg!(target_os = "linux") {
    "Linux (/proc)"
} else {
    "unsupported"
};
