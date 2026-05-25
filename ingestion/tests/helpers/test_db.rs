//! Shared connection pool and transaction-rollback wrapper for integration
//! tests against `eafora_test`. Tests acquire a borrow of a process-wide
//! `PgPool` (so we don't pay per-test connection setup), then run their
//! bodies inside a transaction that is rolled back at the end so each test
//! starts from the seeded baseline.

use sqlx::Postgres;
use sqlx::Transaction;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio::sync::OnceCell;

static TEST_POOL: OnceCell<PgPool> = OnceCell::const_new();

pub async fn test_pool() -> &'static PgPool {
    TEST_POOL
        .get_or_init(|| async {
            let database_url: String = std::env::var("TEST_DATABASE_URL")
                .unwrap_or_else(|_| "postgresql://localhost:5433/eafora_test".to_string());
            PgPoolOptions::new()
                .max_connections(4)
                .connect(&database_url)
                .await
                .expect("connect to eafora_test")
        })
        .await
}

pub async fn with_rollback<F, Fut, T>(pool: &PgPool, body: F) -> T
where
    F: FnOnce(Transaction<'static, Postgres>) -> Fut,
    Fut: std::future::Future<Output = (Transaction<'static, Postgres>, T)>,
{
    let transaction: Transaction<'static, Postgres> = pool.begin().await.expect("begin transaction");
    let (transaction, result): (Transaction<'static, Postgres>, T) = body(transaction).await;
    transaction.rollback().await.expect("rollback transaction");
    result
}
