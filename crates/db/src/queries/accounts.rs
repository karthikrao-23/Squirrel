//! Account queries. Upserts on Plaid's account id so re-syncs are idempotent.

use sqlx::PgPool;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    pool: &PgPool,
    user_id: Uuid,
    plaid_item_id: Uuid,
    plaid_account_id: &str,
    name: &str,
    official_name: Option<&str>,
    account_type: Option<&str>,
    subtype: Option<&str>,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO accounts (user_id, plaid_item_id, plaid_account_id, name, official_name, type, subtype)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (plaid_account_id) DO UPDATE
        SET name = EXCLUDED.name,
            official_name = EXCLUDED.official_name,
            type = EXCLUDED.type,
            subtype = EXCLUDED.subtype,
            updated_at = now()
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(plaid_item_id)
    .bind(plaid_account_id)
    .bind(name)
    .bind(official_name)
    .bind(account_type)
    .bind(subtype)
    .fetch_one(pool)
    .await
}
