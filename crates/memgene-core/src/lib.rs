//! # memgene-core
//!
//! Platform-independent heart of MemGene: value types, the scan engine, and the
//! cheat table. It talks to the outside world only through the
//! [`MemorySource`](process::MemorySource) trait, so all of it is unit-testable
//! against [`mock::MockMemory`] with no real process.

pub mod constants;
pub mod error;
pub mod mock;
pub mod process;
pub mod scan;
pub mod table;
pub mod value;

pub use error::{MemError, ScanError, TableError};
pub use process::{MemoryRegion, MemorySource};
pub use scan::{Compare, Match, ScanSession};
pub use table::{CheatTable, Locator, TableEntry};
pub use value::{ScanValue, ValueType};
