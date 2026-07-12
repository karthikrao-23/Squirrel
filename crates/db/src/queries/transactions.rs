//! Transaction queries. Investment transactions are immutable history, so we
//! insert and ignore conflicts on Plaid's transaction id (idempotent re-sync).

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

/// A transaction joined with its security ticker, for the API.
#[derive(Debug, Serialize, FromRow)]
pub struct TransactionView {
    pub id: Uuid,
    pub date: NaiveDate,
    pub account_id: Uuid,
    pub ticker: Option<String>,
    pub transaction_type: Option<String>,
    pub subtype: Option<String>,
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub fees: Option<Decimal>,
    pub name: Option<String>,
}

/// Recent transactions, newest first.
pub async fn list(
    conn: &mut PgConnection,
    user_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<TransactionView>> {
    sqlx::query_as::<_, TransactionView>(
        r#"
        SELECT t.id, t.date, t.account_id, s.ticker, t.type AS transaction_type,
               t.subtype, t.quantity, t.price, t.amount, t.fees, t.name
        FROM transactions t
        LEFT JOIN securities s ON s.id = t.security_id
        WHERE t.user_id = $1
        ORDER BY t.date DESC, t.id
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(&mut *conn)
    .await
}

/// Minimal transaction fields needed to reconstruct tax lots, grouped by
/// (account, security) and ordered chronologically. Only rows with a security.
#[derive(Debug, FromRow)]
pub struct LotTxnRow {
    pub id: Uuid,
    pub account_id: Uuid,
    pub security_id: Uuid,
    pub transaction_type: Option<String>,
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub fees: Option<Decimal>,
    pub date: NaiveDate,
}

/// Security ids the user *bought* on or after `since` — used to flag wash-sale
/// risk (a purchase within 30 days of selling the same security at a loss).
pub async fn recent_buy_security_ids(
    conn: &mut PgConnection,
    user_id: Uuid,
    since: NaiveDate,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT DISTINCT security_id
        FROM transactions
        WHERE user_id = $1 AND lower(type) = 'buy'
          AND security_id IS NOT NULL AND date >= $2
        "#,
    )
    .bind(user_id)
    .bind(since)
    .fetch_all(&mut *conn)
    .await
}

pub async fn list_for_lots(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<Vec<LotTxnRow>> {
    sqlx::query_as::<_, LotTxnRow>(
        r#"
        SELECT t.id, t.account_id, t.security_id, t.type AS transaction_type,
               t.quantity, t.price, t.amount, t.fees, t.date
        FROM transactions t
        WHERE t.user_id = $1 AND t.security_id IS NOT NULL
        ORDER BY t.account_id, t.security_id, t.date, t.id
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await
}

#[allow(clippy::too_many_arguments)]
pub struct NewTransaction<'a> {
    pub user_id: Uuid,
    pub account_id: Uuid,
    pub security_id: Option<Uuid>,
    pub plaid_investment_transaction_id: &'a str,
    pub transaction_type: Option<&'a str>,
    pub subtype: Option<&'a str>,
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub fees: Option<Decimal>,
    pub date: NaiveDate,
    pub name: Option<&'a str>,
    pub currency: Option<&'a str>,
}

/// Insert a transaction, skipping it if we've already stored it. Returns whether
/// a new row was inserted (useful for sync summaries).
pub async fn insert_ignore(conn: &mut PgConnection, tx: &NewTransaction<'_>) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r#"
        INSERT INTO transactions
            (user_id, account_id, security_id, plaid_investment_transaction_id,
             type, subtype, quantity, price, amount, fees, date, name, currency)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        ON CONFLICT (user_id, plaid_investment_transaction_id) DO NOTHING
        "#,
    )
    .bind(tx.user_id)
    .bind(tx.account_id)
    .bind(tx.security_id)
    .bind(tx.plaid_investment_transaction_id)
    .bind(tx.transaction_type)
    .bind(tx.subtype)
    .bind(tx.quantity)
    .bind(tx.price)
    .bind(tx.amount)
    .bind(tx.fees)
    .bind(tx.date)
    .bind(tx.name)
    .bind(tx.currency)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() > 0)
}
