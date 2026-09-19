pub mod schema;
pub mod shard_db;

// sqlite-wasm-rs, which the VFS registers against, builds only for wasm32
#[cfg(target_arch = "wasm32")]
pub mod ro_memory_vfs;

// sqlite-wasm-rs exposes no safe query wrapper, so the wasm32 reader steps the raw FFI
#[cfg(target_arch = "wasm32")]
pub mod ffi_conversions;

pub use schema::*;
pub use shard_db::*;
