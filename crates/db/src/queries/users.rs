//! User queries. v1 is single-user, so we resolve "the" user server-side,
//! creating a default row on first use.

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::User;

/// Return the single app user, creating a default one if none exists yet.
pub async fn ensure_default(pool: &PgPool) -> sqlx::Result<User> {
    if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at LIMIT 1")
        .fetch_optional(pool)
        .await?
    {
        return Ok(user);
    }

    sqlx::query_as::<_, User>(
        "INSERT INTO users (filing_status, taxable_income) VALUES ('single', 0) RETURNING *",
    )
    .fetch_one(pool)
    .await
}

/// Update the tax profile (filing status + taxable income) used by the tax
/// engine. Returns the updated row.
pub async fn update_profile(
    pool: &PgPool,
    id: Uuid,
    filing_status: &str,
    taxable_income: Decimal,
) -> sqlx::Result<User> {
    sqlx::query_as::<_, User>(
        "UPDATE users SET filing_status = $2, taxable_income = $3, updated_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(filing_status)
    .bind(taxable_income)
    .fetch_one(pool)
    .await
}
