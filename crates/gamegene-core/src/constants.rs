//! Centralized constants and tunables.
//!
//! This is the single place to change the app's identity or scan tuning.
//! Nothing else in the codebase should hard-code these values — refer here so
//! a rename or a performance tweak is a one-line change.

/// Product name. Shown in the window title and used to derive config paths.
pub const APP_NAME: &str = "GameGene";

/// One-line tagline shown in the UI header.
pub const APP_TAGLINE: &str = "Single-player memory editor";

/// Folder name used under the OS config directory to store cheat tables.
pub const CONFIG_DIR_NAME: &str = "gamegene";

/// File extension for saved cheat tables.
pub const TABLE_FILE_EXT: &str = "ggtable";

/// Bytes read per `read` call while scanning a region. Larger = fewer syscalls
/// but more transient memory. 4 MiB is a good balance for desktop games.
pub const SCAN_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// When re-scanning a candidate list, adjacent candidates whose addresses fall
/// within this many bytes are coalesced into a single read instead of one read
/// syscall per address. Turns a dense next-scan from millions of tiny reads
/// into a handful of block reads.
pub const NEXT_SCAN_BLOCK: usize = 64 * 1024;

/// Upper bound on results the UI will render at once. The scan engine keeps all
/// matches; this only caps what the table view materializes so a huge "unknown
/// initial value" scan cannot freeze the UI.
pub const MAX_RESULTS_DISPLAY: usize = 5_000;

/// How often (milliseconds) frozen table entries are re-written to memory.
pub const FREEZE_INTERVAL_MS: u64 = 100;

/// On-disk format version for cheat tables. Bump when the schema changes so
/// older files can be migrated or rejected with a clear message.
pub const TABLE_FORMAT_VERSION: u32 = 1;

/// Pointer width assumed when dereferencing pointer chains. GameGene targets
/// 64-bit games; 32-bit support would key this off the target process.
pub const POINTER_SIZE: usize = 8;
