//! Database access layer: connection pool, migrations, and typed models.
//! Uses SQLx with compile-time-checked queries against PostgreSQL.

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

pub mod models;

/// Create a connection pool to PostgreSQL.
///
/// The pool is cloneable and cheap to share (it's an `Arc` internally), so the
/// Axum app holds one and hands clones to each request handler.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run all pending migrations embedded from the workspace `migrations/` dir.
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    tracing::info!("migrations applied");
    Ok(())
}
