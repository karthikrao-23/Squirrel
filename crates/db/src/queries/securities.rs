//! Security queries. Securities are shared across users (keyed by Plaid's
//! security id), so this upsert has no `user_id`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    pool: &PgPool,
    plaid_security_id: &str,
    ticker: Option<&str>,
    name: Option<&str>,
    cusip: Option<&str>,
    security_type: Option<&str>,
    close_price: Option<Decimal>,
    close_price_as_of: Option<NaiveDate>,
    currency: Option<&str>,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO securities
            (plaid_security_id, ticker, name, cusip, type, close_price, close_price_as_of, currency)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (plaid_security_id) DO UPDATE
        SET ticker = EXCLUDED.ticker,
            name = EXCLUDED.name,
            cusip = EXCLUDED.cusip,
            type = EXCLUDED.type,
            close_price = EXCLUDED.close_price,
            close_price_as_of = EXCLUDED.close_price_as_of,
            currency = EXCLUDED.currency,
            updated_at = now()
        RETURNING id
        "#,
    )
    .bind(plaid_security_id)
    .bind(ticker)
    .bind(name)
    .bind(cusip)
    .bind(security_type)
    .bind(close_price)
    .bind(close_price_as_of)
    .bind(currency)
    .fetch_one(pool)
    .await
}
