//! Transaction queries. Investment transactions are immutable history, so we
//! insert and ignore conflicts on Plaid's transaction id (idempotent re-sync).

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

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
pub async fn insert_ignore(pool: &PgPool, tx: &NewTransaction<'_>) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r#"
        INSERT INTO transactions
            (user_id, account_id, security_id, plaid_investment_transaction_id,
             type, subtype, quantity, price, amount, fees, date, name, currency)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        ON CONFLICT (plaid_investment_transaction_id) DO NOTHING
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
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
