//! Account queries. Upserts on Plaid's account id so re-syncs are idempotent.

use rust_decimal::Decimal;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::models::Account;

/// All accounts owned by the user, ordered by name.
pub async fn list(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<Vec<Account>> {
    sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE user_id = $1 ORDER BY name")
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await
}

/// Set (or clear) a user's manual tax-classification override for one account.
/// `kind` is `Some("taxable")` / `Some("retirement")` to pin it, or `None` to
/// revert to automatic classification. Scoped by `user_id`, so it only touches
/// the caller's own account. Returns the updated row, or `None` if no account
/// with that id belongs to the user.
pub async fn set_kind_override(
    conn: &mut PgConnection,
    user_id: Uuid,
    account_id: Uuid,
    kind: Option<&str>,
) -> sqlx::Result<Option<Account>> {
    sqlx::query_as::<_, Account>(
        r#"
        UPDATE accounts
        SET kind_override = $3, updated_at = now()
        WHERE id = $2 AND user_id = $1
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(account_id)
    .bind(kind)
    .fetch_optional(&mut *conn)
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert(
    conn: &mut PgConnection,
    user_id: Uuid,
    plaid_item_id: Uuid,
    plaid_account_id: &str,
    name: &str,
    official_name: Option<&str>,
    account_type: Option<&str>,
    subtype: Option<&str>,
    current_balance: Option<Decimal>,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO accounts (user_id, plaid_item_id, plaid_account_id, name, official_name, type, subtype, current_balance)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (user_id, plaid_account_id) DO UPDATE
        SET name = EXCLUDED.name,
            official_name = EXCLUDED.official_name,
            type = EXCLUDED.type,
            subtype = EXCLUDED.subtype,
            -- keep a previously-known balance if this sync didn't report one
            current_balance = COALESCE(EXCLUDED.current_balance, accounts.current_balance),
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
    .bind(current_balance)
    .fetch_one(&mut *conn)
    .await
}

/// Stamp every account under one item with the current time as its last
/// successful sync. Called at the end of a sync once holdings + transactions are
/// persisted and lots rebuilt, so `last_synced_at` reflects a fully successful
/// refresh (not a partial one). Scoped by `user_id` as well as `plaid_item_id`
/// for defense-in-depth alongside RLS. Returns the number of accounts stamped.
pub async fn mark_synced(
    conn: &mut PgConnection,
    user_id: Uuid,
    plaid_item_id: Uuid,
) -> sqlx::Result<u64> {
    let res = sqlx::query(
        "UPDATE accounts SET last_synced_at = now() WHERE user_id = $1 AND plaid_item_id = $2",
    )
    .bind(user_id)
    .bind(plaid_item_id)
    .execute(&mut *conn)
    .await?;
    Ok(res.rows_affected())
}

/// An account whose value we anchor to Plaid's reported balance because it has
/// no open lots (holdings unavailable, e.g. Fidelity BrokerageLink).
#[derive(Debug, Clone, FromRow)]
pub struct BalanceOnlyAccount {
    pub account_id: Uuid,
    pub name: String,
    pub subtype: Option<String>,
    pub kind_override: Option<String>,
    pub current_balance: Decimal,
}

/// Accounts that report a Plaid balance but have **no open tax lots** — their
/// value comes from the balance, not from reconstructed positions. Used to fold
/// holdings-unavailable accounts into the portfolio/retirement totals.
pub async fn balance_only_accounts(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> sqlx::Result<Vec<BalanceOnlyAccount>> {
    sqlx::query_as::<_, BalanceOnlyAccount>(
        r#"
        SELECT a.id AS account_id, a.name, a.subtype, a.kind_override, a.current_balance
        FROM accounts a
        WHERE a.user_id = $1
          AND a.current_balance IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM tax_lots l
              WHERE l.account_id = a.id AND l.status = 'open' AND l.remaining_quantity > 0
          )
        ORDER BY a.name
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await
}
