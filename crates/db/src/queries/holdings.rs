//! Holding queries. One row per (account, security); upserts on that pair so a
//! re-sync overwrites the latest snapshot rather than duplicating.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

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
