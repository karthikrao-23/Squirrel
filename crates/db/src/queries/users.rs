//! User queries. The app is multi-tenant: real users are created via
//! [`create`] (signup) and looked up by email/id. [`ensure_default`] survives
//! only as a test fixture for the `db`/query-layer tests, which don't go through
//! the auth flow.

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::User;

/// Fixture-only: return the first user, creating a default one if none exists.
///
/// `email` is now `NOT NULL`, so this seeds a fixed placeholder address. It is
/// **not** used by request handlers (those use the `AuthUser` extractor); it
/// exists purely so the query-layer tests can mint a user without signing up.
pub async fn ensure_default(pool: &PgPool) -> sqlx::Result<User> {
    if let Some(user) = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at LIMIT 1")
        .fetch_optional(pool)
        .await?
    {
        return Ok(user);
    }

    sqlx::query_as::<_, User>(
        "INSERT INTO users (email, filing_status, taxable_income) \
         VALUES ('default@squirrel.test', 'single', 0) RETURNING *",
    )
    .fetch_one(pool)
    .await
}

/// Create a new user from a signup. The tax profile is hard-coded to safe
/// defaults — it is **never** taken from the signup request, so a client can't
/// seed an arbitrary `filing_status`/`taxable_income`/`id`.
pub async fn create(pool: &PgPool, email: &str, password_hash: &str) -> sqlx::Result<User> {
    sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash, filing_status, taxable_income) \
         VALUES ($1, $2, 'single', 0) RETURNING *",
    )
    .bind(email)
    .bind(password_hash)
    .fetch_one(pool)
    .await
}

/// Look up a user by their (already normalized) email.
pub async fn find_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await
}

/// Look up a user by internal id.
pub async fn find_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// All users, oldest first. Used by the alert engine to run the cycle per user.
pub async fn list_all(pool: &PgPool) -> sqlx::Result<Vec<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at")
        .fetch_all(pool)
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
