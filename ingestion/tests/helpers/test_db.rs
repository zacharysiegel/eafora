//! Connection pool for integration tests against `eafora_test`. Each test
//! gets its own `PgPool` because `#[tokio::test]` creates a fresh tokio
//! runtime per test — a process-wide OnceCell-cached pool's background
//! tasks would be bound to the first runtime and time out on subsequent
//! tests. Pool creation is fast enough that per-test cost is negligible.

// The integration test binary that uses this helper imports only `test_pool`;
// future test binaries are expected to reuse it via the same `mod helpers;`
// pattern. Keep the module-level allow until a second consumer arrives.
#![allow(dead_code)]

use sqlx::postgres::{PgPool, PgPoolOptions};

pub async fn test_pool() -> PgPool {
    let database_url: String = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost:5433/eafora_test".to_string());
    PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect to eafora_test")
}
