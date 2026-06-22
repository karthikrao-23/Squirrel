//! Plaid item queries. An "item" is one linked institution; it owns the
//! encrypted access token we use for all subsequent data pulls.

use sqlx::PgPool;
use uuid::Uuid;

use crate::models::PlaidItem;

/// Insert a freshly linked item, or update its token if the same Plaid item is
/// re-linked. Returns the stored row (with our internal UUID).
pub async fn upsert(
    pool: &PgPool,
    user_id: Uuid,
    plaid_item_id: &str,
    access_token_encrypted: &[u8],
    institution_id: Option<&str>,
) -> sqlx::Result<PlaidItem> {
    sqlx::query_as::<_, PlaidItem>(
        r#"
        INSERT INTO plaid_items (user_id, plaid_item_id, access_token_encrypted, institution_id, status)
        VALUES ($1, $2, $3, $4, 'active')
        ON CONFLICT (plaid_item_id) DO UPDATE
        SET access_token_encrypted = EXCLUDED.access_token_encrypted,
            institution_id = COALESCE(EXCLUDED.institution_id, plaid_items.institution_id),
            status = 'active',
            updated_at = now()
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(plaid_item_id)
    .bind(access_token_encrypted)
    .bind(institution_id)
    .fetch_one(pool)
    .await
}

/// Look up an item by Plaid's item id (used when a webhook arrives).
pub async fn find_by_plaid_item_id(
    pool: &PgPool,
    plaid_item_id: &str,
) -> sqlx::Result<Option<PlaidItem>> {
    sqlx::query_as::<_, PlaidItem>("SELECT * FROM plaid_items WHERE plaid_item_id = $1")
        .bind(plaid_item_id)
        .fetch_optional(pool)
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
