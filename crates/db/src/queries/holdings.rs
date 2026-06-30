//! Holding queries. One row per (account, security); upserts on that pair so a
//! re-sync overwrites the latest snapshot rather than duplicating.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

/// A holding joined with its account + security, ready to serialize for the API.
#[derive(Debug, Serialize, FromRow)]
pub struct HoldingView {
    pub account_id: Uuid,
    pub account_name: String,
    pub security_id: Uuid,
    pub ticker: Option<String>,
    pub security_name: Option<String>,
    pub quantity: Decimal,
    pub institution_price: Option<Decimal>,
    pub institution_value: Option<Decimal>,
    pub cost_basis: Option<Decimal>,
    pub currency: Option<String>,
}

/// A bare position (one per account+security) — the current share count plus the
/// institution-provided cost basis and price. Used to reconcile reconstructed tax
/// lots to the actual holdings (synthesizing an opening-balance lot for shares
/// held before the available transaction history).
#[derive(Debug, FromRow)]
pub struct Position {
    pub account_id: Uuid,
    pub security_id: Uuid,
    pub quantity: Decimal,
    pub cost_basis: Option<Decimal>,
    pub institution_price: Option<Decimal>,
}

/// Every holding's current position for a user (account, security, qty, basis, price).
pub async fn positions_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<Position>> {
    sqlx::query_as::<_, Position>(
        "SELECT account_id, security_id, quantity, cost_basis, institution_price
         FROM holdings WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// All holdings for the user, most valuable first.
pub async fn list_with_security(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<HoldingView>> {
    sqlx::query_as::<_, HoldingView>(
        r#"
        SELECT h.account_id, a.name AS account_name, h.security_id,
               s.ticker, s.name AS security_name, h.quantity,
               h.institution_price, h.institution_value, h.cost_basis, h.currency
        FROM holdings h
        JOIN accounts a ON a.id = h.account_id
        JOIN securities s ON s.id = h.security_id
        WHERE h.user_id = $1
        ORDER BY h.institution_value DESC NULLS LAST
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    pool: &PgPool,
    user_id: Uuid,
    account_id: Uuid,
    security_id: Uuid,
    quantity: Decimal,
    institution_price: Option<Decimal>,
    institution_price_as_of: Option<NaiveDate>,
    institution_value: Option<Decimal>,
    cost_basis: Option<Decimal>,
    currency: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO holdings
            (user_id, account_id, security_id, quantity, institution_price,
             institution_price_as_of, institution_value, cost_basis, currency)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (account_id, security_id) DO UPDATE
        SET quantity = EXCLUDED.quantity,
            institution_price = EXCLUDED.institution_price,
            institution_price_as_of = EXCLUDED.institution_price_as_of,
            institution_value = EXCLUDED.institution_value,
            cost_basis = EXCLUDED.cost_basis,
            currency = EXCLUDED.currency,
            updated_at = now()
        "#,
    )
    .bind(user_id)
    .bind(account_id)
    .bind(security_id)
    .bind(quantity)
    .bind(institution_price)
    .bind(institution_price_as_of)
    .bind(institution_value)
    .bind(cost_basis)
    .bind(currency)
    .execute(pool)
    .await?;
    Ok(())
}
