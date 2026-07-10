//! Error types shared across the core.

use thiserror::Error;

/// Failures raised while reading, writing, or resolving process memory.
#[derive(Error, Debug)]
pub enum MemError {
    #[error("read failed at {addr:#x}: {reason}")]
    Read { addr: u64, reason: String },

    #[error("write failed at {addr:#x}: {reason}")]
    Write { addr: u64, reason: String },

    #[error("address could not be resolved (module missing or pointer chain broke)")]
    Unresolved,

    #[error("no process is attached")]
    NotAttached,

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Failures raised while running a scan.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum ScanError {
    #[error("comparison `{0}` requires a previous scan to compare against")]
    NeedsPrevious(&'static str),

    #[error("value could not be parsed for the selected type: {0}")]
    Parse(String),
}

/// Failures raised while loading or saving a cheat table.
#[derive(Error, Debug)]
pub enum TableError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("table file is not valid JSON: {0}")]
    Format(#[from] serde_json::Error),

    #[error("unsupported table format version {found} (this build understands {supported})")]
    Version { found: u32, supported: u32 },
}
