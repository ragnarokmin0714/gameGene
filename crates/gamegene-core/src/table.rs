//! The cheat table: save found addresses so you never rescan the same value.
//!
//! Once a scan pins down an address, you promote it into a [`TableEntry`] with a
//! label and (optionally) a value to write or freeze. The table serializes to
//! JSON, so a later session just loads it and re-applies — no rescan.
//!
//! Raw addresses only stay valid for one game run (ASLR moves everything on
//! restart). To survive restarts, store the address as a [`Locator::ModuleOffset`]
//! or [`Locator::Pointer`] chain, which is re-resolved against the live process.

use crate::constants::{POINTER_SIZE, TABLE_FORMAT_VERSION};
use crate::error::{MemError, TableError};
use crate::process::MemorySource;
use crate::value::{ScanValue, ValueType};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How to locate an entry's address in the live process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Locator {
    /// A fixed address. Valid only for the current game session.
    Absolute(u64),
    /// `module_base + offset`. Survives restart as long as the module loads.
    ModuleOffset { module: String, offset: i64 },
    /// A pointer chain `[[[module+base_offset] + offsets[0]] + …]`.
    /// The last offset is added, not dereferenced — standard CE semantics.
    Pointer {
        module: String,
        base_offset: i64,
        offsets: Vec<i64>,
    },
}

impl Locator {
    /// Resolve to a concrete address against the live process, or `None` if the
    /// module is missing or a pointer in the chain could not be read.
    pub fn resolve(&self, src: &dyn MemorySource) -> Option<u64> {
        match self {
            Locator::Absolute(addr) => Some(*addr),
            Locator::ModuleOffset { module, offset } => {
                let base = src.module_base(module)?;
                Some(base.wrapping_add_signed(*offset))
            }
            Locator::Pointer {
                module,
                base_offset,
                offsets,
            } => {
                let base = src.module_base(module)?;
                let mut addr = base.wrapping_add_signed(*base_offset);
                if offsets.is_empty() {
                    return read_pointer(src, addr);
                }
                // Dereference through all but the last offset...
                addr = read_pointer(src, addr)?;
                for off in &offsets[..offsets.len() - 1] {
                    addr = read_pointer(src, addr.wrapping_add_signed(*off))?;
                }
                // ...then add the last offset without dereferencing.
                Some(addr.wrapping_add_signed(offsets[offsets.len() - 1]))
            }
        }
    }
}

fn read_pointer(src: &dyn MemorySource, addr: u64) -> Option<u64> {
    let mut buf = [0u8; POINTER_SIZE];
    let n = src.read(addr, &mut buf).ok()?;
    if n < POINTER_SIZE {
        return None;
    }
    Some(u64::from_le_bytes(buf))
}

/// One saved value: where it is, what type, and the value to enforce.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableEntry {
    /// Stable id within a table, assigned by [`CheatTable::add`].
    pub id: u64,
    /// User-facing name, e.g. "HP" or "Gil".
    pub label: String,
    pub value_type: ValueType,
    pub locator: Locator,
    /// Value to write on "apply" and to keep re-writing while frozen.
    pub desired: Option<ScanValue>,
    /// Whether this entry is currently frozen (continuously re-written).
    pub frozen: bool,
    /// Free-form notes.
    #[serde(default)]
    pub notes: String,
}

impl TableEntry {
    /// Read the entry's current in-game value.
    pub fn read_current(&self, src: &dyn MemorySource) -> Option<ScanValue> {
        let addr = self.locator.resolve(src)?;
        let size = self.value_type.size();
        let mut buf = [0u8; 8];
        let n = src.read(addr, &mut buf[..size]).ok()?;
        if n < size {
            return None;
        }
        Some(ScanValue::from_le_bytes(self.value_type, &buf))
    }

    /// Write an explicit value to the entry's address.
    pub fn write_value(&self, src: &dyn MemorySource, value: ScanValue) -> Result<(), MemError> {
        let addr = self.locator.resolve(src).ok_or(MemError::Unresolved)?;
        src.write(addr, &value.to_le_bytes())
    }

    /// Write the desired value, if one is set. No-op otherwise.
    pub fn apply_desired(&self, src: &dyn MemorySource) -> Result<(), MemError> {
        match self.desired {
            Some(v) => self.write_value(src, v),
            None => Ok(()),
        }
    }
}

/// A named collection of [`TableEntry`] values, saveable to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheatTable {
    pub version: u32,
    /// GameGene version that last wrote this file, e.g. "0.4.1". Recorded for
    /// diagnostics; not enforced on load (older files simply have it empty).
    #[serde(default)]
    pub app_version: String,
    /// Optional note about which game this table is for.
    #[serde(default)]
    pub game_hint: String,
    pub entries: Vec<TableEntry>,
    /// Monotonic id source. Skipped in JSON; recomputed on load.
    #[serde(skip)]
    next_id: u64,
}

impl Default for CheatTable {
    fn default() -> Self {
        CheatTable {
            version: TABLE_FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            game_hint: String::new(),
            entries: Vec::new(),
            next_id: 1,
        }
    }
}

impl CheatTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entry, assigning and returning its id. The `id` field of the
    /// passed entry is ignored and overwritten.
    pub fn add(&mut self, mut entry: TableEntry) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        entry.id = id;
        self.entries.push(entry);
        id
    }

    /// Remove an entry by id. Returns whether one was removed.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut TableEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Re-write every frozen entry's desired value. Errors on individual
    /// entries are collected, not fatal — a temporarily unresolvable pointer
    /// shouldn't stop the others from being enforced.
    pub fn tick_frozen(&self, src: &dyn MemorySource) -> Vec<(u64, MemError)> {
        let mut errors = Vec::new();
        for e in &self.entries {
            if e.frozen {
                if let Err(err) = e.apply_desired(src) {
                    errors.push((e.id, err));
                }
            }
        }
        errors
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> Result<String, TableError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Parse from JSON, rejecting unknown format versions.
    pub fn from_json(json: &str) -> Result<CheatTable, TableError> {
        let mut table: CheatTable = serde_json::from_str(json)?;
        if table.version != TABLE_FORMAT_VERSION {
            return Err(TableError::Version {
                found: table.version,
                supported: TABLE_FORMAT_VERSION,
            });
        }
        // Rebuild the id counter above the highest saved id.
        table.next_id = table.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        Ok(table)
    }

    /// Save to a file path.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), TableError> {
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }

    /// Load from a file path.
    pub fn load(path: impl AsRef<Path>) -> Result<CheatTable, TableError> {
        let json = std::fs::read_to_string(path)?;
        CheatTable::from_json(&json)
    }
}
