//! Plaid item queries. An "item" is one linked institution; it owns the
//! encrypted access token we use for all subsequent data pulls.

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::PlaidItem;

/// Insert a freshly linked item, or update its token if the same Plaid item is
/// re-linked. Returns the stored row (with our internal UUID).
#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    pool: &PgPool,
    user_id: Uuid,
    plaid_item_id: &str,
    access_token_encrypted: &[u8],
    institution_id: Option<&str>,
    plaid_client_id: &str,
) -> sqlx::Result<PlaidItem> {
    sqlx::query_as::<_, PlaidItem>(
        r#"
        INSERT INTO plaid_items (user_id, plaid_item_id, access_token_encrypted, institution_id, plaid_client_id, status)
        VALUES ($1, $2, $3, $4, $5, 'active')
        ON CONFLICT (user_id, plaid_item_id) DO UPDATE
        SET access_token_encrypted = EXCLUDED.access_token_encrypted,
            institution_id = COALESCE(EXCLUDED.institution_id, plaid_items.institution_id),
            plaid_client_id = EXCLUDED.plaid_client_id,
            status = 'active',
            updated_at = now()
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(plaid_item_id)
    .bind(access_token_encrypted)
    .bind(institution_id)
    .bind(plaid_client_id)
    .fetch_one(pool)
    .await
}

/// Count of **active** items per Plaid app (`plaid_client_id`), across all users
/// — Plaid's live-item cap is per app, not per user. Legacy items have a NULL
/// `plaid_client_id` (they belong to the primary app); that bucket is returned as
/// `None`. Drives capacity-based routing of new connections.
pub async fn active_counts_by_client(pool: &PgPool) -> sqlx::Result<Vec<(Option<String>, i64)>> {
    sqlx::query_as::<_, (Option<String>, i64)>(
        r#"
        SELECT plaid_client_id, COUNT(*)
        FROM plaid_items
        WHERE status = 'active'
        GROUP BY plaid_client_id
        "#,
    )
    .fetch_all(pool)
    .await
}

/// Look up every item with this Plaid item id (used when a webhook arrives).
///
/// Returns a `Vec` because `plaid_item_id` is only unique *per user* now
/// (`UNIQUE (user_id, plaid_item_id)`): in sandbox two users connecting the same
/// institution share an id, so a webhook for it must re-sync each owner's item.
/// In production a real item id belongs to one user, so this is usually one row.
pub async fn find_all_by_plaid_item_id(
    pool: &PgPool,
    plaid_item_id: &str,
) -> sqlx::Result<Vec<PlaidItem>> {
    sqlx::query_as::<_, PlaidItem>("SELECT * FROM plaid_items WHERE plaid_item_id = $1")
        .bind(plaid_item_id)
        .fetch_all(pool)
        .await
}

/// Record the institution id once we learn it from a holdings response.
pub async fn set_institution_id(pool: &PgPool, id: Uuid, institution_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE plaid_items SET institution_id = $2, updated_at = now() WHERE id = $1 AND institution_id IS NULL",
    )
    .bind(id)
    .bind(institution_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// All items linked by a user (used by the scheduler to refresh each).
pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<PlaidItem>> {
    sqlx::query_as::<_, PlaidItem>(
        "SELECT * FROM plaid_items WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// A single item by its UUID, scoped to the owning user (`None` if it isn't
/// theirs or doesn't exist). Used before removing a connection so we can pull the
/// access token to disconnect on Plaid's side.
pub async fn find_by_id(pool: &PgPool, user_id: Uuid, id: Uuid) -> sqlx::Result<Option<PlaidItem>> {
    sqlx::query_as::<_, PlaidItem>("SELECT * FROM plaid_items WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Delete a user's item; ON DELETE CASCADE removes its accounts, holdings,
/// transactions, and tax lots. Scoped by `user_id` so one user can't delete
/// another's connection. Returns the number of rows removed (0 = not found).
pub async fn delete(pool: &PgPool, user_id: Uuid, id: Uuid) -> sqlx::Result<u64> {
    let res = sqlx::query("DELETE FROM plaid_items WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}
