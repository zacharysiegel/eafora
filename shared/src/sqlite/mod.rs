pub mod schema;

// Not for wasm32 yet: the native loader uses rusqlite; the wasm32 loader (raw sqlite-wasm-rs) lands
// in a later increment behind the same `load_shard` signature.
#[cfg(not(target_arch = "wasm32"))]
pub mod shard_db;

pub use schema::*;

#[cfg(not(target_arch = "wasm32"))]
pub use shard_db::*;
