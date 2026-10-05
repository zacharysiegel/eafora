#[cfg(not(target_arch = "wasm32"))] // reads the local filesystem
pub mod filesystem_fetch;
pub mod http_model;
#[cfg(not(target_arch = "wasm32"))] // reqwest opens its own sockets
pub mod reqwest_fetch;

#[cfg(not(target_arch = "wasm32"))] // reads the local filesystem
pub use filesystem_fetch::*;
pub use http_model::*;
#[cfg(not(target_arch = "wasm32"))] // reqwest opens its own sockets
pub use reqwest_fetch::*;
