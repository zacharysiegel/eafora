pub mod error;
pub mod filesystem;

pub const REVISION: &str = env!("EAFORA_REVISION");

pub use error::AppError;
pub use filesystem::*;
