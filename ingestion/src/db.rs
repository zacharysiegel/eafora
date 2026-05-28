use std::env;

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::error::AppError;

pub async fn create_pool() -> Result<PgPool, AppError> {
    let database_url: String = env::var("DATABASE_URL")?;
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await?;
    Ok(pool)
}
