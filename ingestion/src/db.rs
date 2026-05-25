//! PgPool bootstrap. Reads `DATABASE_URL` from the environment, builds a
//! configured pool, returns it. Symmetric in concern with
//! `tests/helpers/test_db.rs` but for the production binary.

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::error::AppError;

pub async fn build_pool() -> Result<PgPool, AppError> {
    let database_url: String = std::env::var("DATABASE_URL")
        .map_err(|err| AppError::new(&format!("db: DATABASE_URL not set: {err}")))?;
    PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .map_err(|err| AppError::new(&format!("db: connect to {database_url} failed: {err}")))
}
