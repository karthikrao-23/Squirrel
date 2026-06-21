//! User queries. v1 is single-user, so we resolve "the" user server-side,
//! creating a default row on first use.

use sqlx::PgPool;

use crate::models::User;

/// Return the single app user, creating a default one if none exists yet.
pub async fn ensure_default(pool: &PgPool) -> sqlx::Result<User> {
    if let Some(user) =
        sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at LIMIT 1")
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
