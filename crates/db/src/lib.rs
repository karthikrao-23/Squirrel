//! Database access layer: connection pool, migrations, and typed models.
//! Uses SQLx with compile-time-checked queries against PostgreSQL.

use log::LevelFilter;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::ConnectOptions;
use std::str::FromStr;
use std::time::Duration;

pub mod models;
pub mod queries;

/// Create a connection pool to PostgreSQL.
///
/// The pool is cloneable and cheap to share (it's an `Arc` internally), so the
/// Axum app holds one and hands clones to each request handler.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    // Clamp statement logging to WARN so `RUST_LOG=debug` can't dump bound
    // params — which include password hashes and session/Plaid tokens — into the
    // logs. sqlx logs successful statements at DEBUG by default.
    let connect_options = PgConnectOptions::from_str(database_url)?
        .log_statements(LevelFilter::Warn)
        .log_slow_statements(LevelFilter::Warn, Duration::from_secs(1));

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(connect_options)
        .await?;
    Ok(pool)
}

/// Run all pending migrations embedded from the workspace `migrations/` dir.
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    tracing::info!("migrations applied");
    Ok(())
}
