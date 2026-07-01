pub mod schema;
pub mod shard_db;

// wasm32 only: the read-only VFS that lets SQLite read a shard's in-memory bytes (native uses
// rusqlite's deserialize instead).
#[cfg(target_arch = "wasm32")]
pub mod ro_memory_vfs;

// wasm32 only: raw-FFI-to-Rust conversions for the wasm shard reader (native uses rusqlite).
#[cfg(target_arch = "wasm32")]
pub mod ffi_conversions;

pub use schema::*;
pub use shard_db::*;
