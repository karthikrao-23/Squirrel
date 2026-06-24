//! Session queries. A session row stores only the SHA-256 of the opaque token
//! (`token_hash`); the raw token never touches the database. Lookups are by hash.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Session, User};

/// Persist a new session. `token_hash` is the raw 32-byte SHA-256 of the opaque
/// token; `expires_at` is the absolute expiry computed by the caller.
pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &[u8],
    expires_at: DateTime<Utc>,
) -> sqlx::Result<Session> {
    sqlx::query_as::<_, Session>(
        "INSERT INTO sessions (user_id, token_hash, expires_at) \
         VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await
}

/// Resolve a session token hash to its (session, user), but only if the session
/// has not expired. Returns `None` for an unknown or expired token.
pub async fn find_valid_by_token_hash(
    pool: &PgPool,
    token_hash: &[u8],
) -> sqlx::Result<Option<(Session, User)>> {
    // Fetch the session and its user in one round-trip. We select the columns
    // explicitly with `s.`/`u.` prefixes and map them by position into the two
    // structs (sharing a `FromRow` would collide on `id`/`created_at`).
    let row = sqlx::query_as::<_, SessionWithUser>(
        r#"
        SELECT
            s.id            AS s_id,
            s.user_id       AS s_user_id,
            s.token_hash    AS s_token_hash,
            s.created_at    AS s_created_at,
            s.last_used_at  AS s_last_used_at,
            s.expires_at    AS s_expires_at,
            u.id            AS u_id,
            u.email         AS u_email,
            u.filing_status AS u_filing_status,
            u.taxable_income AS u_taxable_income,
            u.password_hash AS u_password_hash,
            u.created_at    AS u_created_at,
            u.updated_at    AS u_updated_at
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        WHERE s.token_hash = $1 AND s.expires_at > now()
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(SessionWithUser::split))
}

/// Bump `last_used_at`, but only if it's already stale (older than 5 minutes).
/// This makes the per-request session write a no-op in steady state, removing a
/// write-amplification lever an attacker could otherwise pump.
pub async fn touch_if_stale(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE sessions SET last_used_at = now() \
         WHERE id = $1 AND last_used_at < now() - interval '5 minutes'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete a single session by its token hash (logout). Returns whether a row was
/// removed.
pub async fn delete(pool: &PgPool, token_hash: &[u8]) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete every session for a user ("log out everywhere").
pub async fn delete_all_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Reap expired sessions. Ridden on the alert engine's periodic cycle since the
/// in-process scheduler is disabled in production.
pub async fn delete_expired(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Flat join row, mapped by aliased column names, then split into the two models.
#[derive(sqlx::FromRow)]
struct SessionWithUser {
    s_id: Uuid,
    s_user_id: Uuid,
    s_token_hash: Vec<u8>,
    s_created_at: DateTime<Utc>,
    s_last_used_at: DateTime<Utc>,
    s_expires_at: DateTime<Utc>,
    u_id: Uuid,
    u_email: String,
    u_filing_status: String,
    u_taxable_income: rust_decimal::Decimal,
    u_password_hash: Option<String>,
    u_created_at: DateTime<Utc>,
    u_updated_at: DateTime<Utc>,
}

impl SessionWithUser {
    fn split(self) -> (Session, User) {
        let session = Session {
            id: self.s_id,
            user_id: self.s_user_id,
            token_hash: self.s_token_hash,
            created_at: self.s_created_at,
            last_used_at: self.s_last_used_at,
            expires_at: self.s_expires_at,
        };
        let user = User {
            id: self.u_id,
            email: self.u_email,
            filing_status: self.u_filing_status,
            taxable_income: self.u_taxable_income,
            password_hash: self.u_password_hash,
            created_at: self.u_created_at,
            updated_at: self.u_updated_at,
        };
        (session, user)
    }
}
