//! Tax-lot queries. Lots are *derived* (reconstructed from transactions), so we
//! replace the whole set for a user atomically on each rebuild rather than
//! trying to patch individual rows.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// A lot to be inserted during a rebuild (always opens as `status = 'open'`).
pub struct NewLot {
    pub account_id: Uuid,
    pub security_id: Uuid,
    pub open_date: NaiveDate,
    pub original_quantity: Decimal,
    pub remaining_quantity: Decimal,
    pub cost_basis_per_share: Decimal,
    pub source_transaction_id: Option<Uuid>,
}

/// Atomically replace all of a user's lots with a freshly reconstructed set.
/// Runs in one transaction so a reader never sees a half-rebuilt state.
pub async fn replace_for_user(pool: &PgPool, user_id: Uuid, lots: &[NewLot]) -> sqlx::Result<u64> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM tax_lots WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    for lot in lots {
        sqlx::query(
            r#"
            INSERT INTO tax_lots
                (user_id, account_id, security_id, open_date, quantity,
                 remaining_quantity, cost_basis_per_share, status, source_transaction_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'open', $8)
            "#,
        )
        .bind(user_id)
        .bind(lot.account_id)
        .bind(lot.security_id)
        .bind(lot.open_date)
        .bind(lot.original_quantity)
        .bind(lot.remaining_quantity)
        .bind(lot.cost_basis_per_share)
        .bind(lot.source_transaction_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(lots.len() as u64)
}

/// A lot joined with its ticker, for the API.
#[derive(Debug, Serialize, FromRow)]
pub struct LotView {
    pub id: Uuid,
    pub account_id: Uuid,
    pub security_id: Uuid,
    pub ticker: Option<String>,
    pub open_date: NaiveDate,
    pub quantity: Decimal,
    pub remaining_quantity: Decimal,
    pub cost_basis_per_share: Decimal,
    pub status: String,
}

pub async fn list_with_security(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<LotView>> {
    sqlx::query_as::<_, LotView>(
        r#"
        SELECT l.id, l.account_id, l.security_id, s.ticker, l.open_date,
               l.quantity, l.remaining_quantity, l.cost_basis_per_share, l.status
        FROM tax_lots l
        JOIN securities s ON s.id = l.security_id
        WHERE l.user_id = $1
        ORDER BY s.ticker, l.open_date
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}
