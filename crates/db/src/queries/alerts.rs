//! Alert queries: dedup-aware creation, listing, marking read, and tracking
//! email delivery.

use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::models::Alert;

/// Insert an alert only if there isn't already an **unread** one of the same
/// `(type, security)` for this user — prevents the scheduler from re-spamming a
/// standing condition. Returns the new row, or `None` if a duplicate suppressed it.
#[allow(clippy::too_many_arguments)]
pub async fn create_if_absent(
    conn: &mut PgConnection,
    user_id: Uuid,
    alert_type: &str,
    security_id: Option<Uuid>,
    title: &str,
    message: &str,
    payload: Value,
) -> sqlx::Result<Option<Alert>> {
    sqlx::query_as::<_, Alert>(
        r#"
        INSERT INTO alerts (user_id, type, security_id, title, message, payload)
        SELECT $1, $2, $3, $4, $5, $6
        WHERE NOT EXISTS (
            SELECT 1 FROM alerts
            WHERE user_id = $1 AND type = $2
              AND security_id IS NOT DISTINCT FROM $3
              AND read_at IS NULL
        )
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(alert_type)
    .bind(security_id)
    .bind(title)
    .bind(message)
    .bind(payload)
    .fetch_optional(&mut *conn)
    .await
}

/// List a user's alerts, newest first; optionally only the unread ones.
pub async fn list(
    conn: &mut PgConnection,
    user_id: Uuid,
    unread_only: bool,
) -> sqlx::Result<Vec<Alert>> {
    let sql = if unread_only {
        "SELECT * FROM alerts WHERE user_id = $1 AND read_at IS NULL ORDER BY created_at DESC"
    } else {
        "SELECT * FROM alerts WHERE user_id = $1 ORDER BY created_at DESC"
    };
    sqlx::query_as::<_, Alert>(sql)
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await
}

/// Mark one alert read; returns whether it changed a row (false if missing/already read).
pub async fn mark_read(conn: &mut PgConnection, user_id: Uuid, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "UPDATE alerts SET read_at = now() WHERE id = $1 AND user_id = $2 AND read_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Alerts not yet emailed (for the digest sender).
pub async fn list_unemailed(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<Vec<Alert>> {
    sqlx::query_as::<_, Alert>(
        "SELECT * FROM alerts WHERE user_id = $1 AND emailed_at IS NULL ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await
}

pub async fn mark_emailed(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE alerts SET emailed_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}
