pub mod schema;
pub mod shard_db;

// wasm32 only: the read-only VFS that lets SQLite read a shard's in-memory bytes (native uses
// rusqlite's deserialize instead).
#[cfg(target_arch = "wasm32")]
pub mod vfs;

pub use schema::*;
pub use shard_db::*;
