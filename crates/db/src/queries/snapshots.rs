//! Portfolio snapshot queries. One row per (user, day): the periodic cycle
//! upserts the day's market value + cost basis, and the dashboard reads the
//! ordered history to chart value over time.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// A single day's portfolio totals.
#[derive(Debug, Serialize, FromRow)]
pub struct Snapshot {
    pub as_of: NaiveDate,
    pub market_value: Decimal,
    pub cost_basis: Decimal,
}

/// Record (or overwrite) the snapshot for `(user_id, as_of, scope)`. `scope` is
/// "total" for the whole portfolio, or "retirement"/"taxable" for the subset.
/// Idempotent per (day, scope): re-running the cycle replaces the row.
pub async fn upsert(
    pool: &PgPool,
    user_id: Uuid,
    as_of: NaiveDate,
    scope: &str,
    market_value: Decimal,
    cost_basis: Decimal,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO portfolio_snapshots (user_id, as_of, scope, market_value, cost_basis)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (user_id, as_of, scope) DO UPDATE
        SET market_value = EXCLUDED.market_value,
            cost_basis = EXCLUDED.cost_basis
        "#,
    )
    .bind(user_id)
    .bind(as_of)
    .bind(scope)
    .bind(market_value)
    .bind(cost_basis)
    .execute(pool)
    .await?;
    Ok(())
}

/// A user's snapshots for one `scope`, oldest first (chart-ready).
pub async fn history(pool: &PgPool, user_id: Uuid, scope: &str) -> sqlx::Result<Vec<Snapshot>> {
    sqlx::query_as::<_, Snapshot>(
        r#"
        SELECT as_of, market_value, cost_basis
        FROM portfolio_snapshots
        WHERE user_id = $1 AND scope = $2
        ORDER BY as_of
        "#,
    )
    .bind(user_id)
    .bind(scope)
    .fetch_all(pool)
    .await
}
