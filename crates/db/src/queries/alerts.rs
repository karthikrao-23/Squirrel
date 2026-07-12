//! Alert queries: dedup-aware creation, listing, marking read, and tracking
//! email delivery.

use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::models::Alert;

/// Create-or-refresh an alert for a standing condition. One live alert per
/// `(user, type, security)`: if an **unread** one already exists we refresh its
/// title/message/payload/`updated_at` in place (so the periodic cycle keeps the
/// figures + timestamp current instead of leaving a stale copy from first
/// detection); otherwise we insert a new one. Returns the row and whether it was
/// newly created (so the caller can count/email only genuinely-new alerts).
///
/// Read alerts are left untouched — the user already engaged with them.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_active(
    conn: &mut PgConnection,
    user_id: Uuid,
    alert_type: &str,
    security_id: Option<Uuid>,
    title: &str,
    message: &str,
    payload: Value,
) -> sqlx::Result<(Alert, bool)> {
    // Refresh an existing unread alert if present.
    let refreshed = sqlx::query_as::<_, Alert>(
        r#"
        UPDATE alerts
        SET title = $4, message = $5, payload = $6, updated_at = now()
        WHERE user_id = $1 AND type = $2
          AND security_id IS NOT DISTINCT FROM $3
          AND read_at IS NULL
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(alert_type)
    .bind(security_id)
    .bind(title)
    .bind(message)
    .bind(&payload)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(alert) = refreshed {
        return Ok((alert, false));
    }

    // None to refresh → insert a new one.
    let created = sqlx::query_as::<_, Alert>(
        r#"
        INSERT INTO alerts (user_id, type, security_id, title, message, payload)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(alert_type)
    .bind(security_id)
    .bind(title)
    .bind(message)
    .bind(payload)
    .fetch_one(&mut *conn)
    .await?;
    Ok((created, true))
}

/// Unread alerts of a given `type` for a user — used to reconcile standing
/// conditions against the current cycle (e.g. detect harvest opportunities that
/// have since disappeared).
pub async fn list_unread_by_type(
    conn: &mut PgConnection,
    user_id: Uuid,
    alert_type: &str,
) -> sqlx::Result<Vec<Alert>> {
    sqlx::query_as::<_, Alert>(
        "SELECT * FROM alerts WHERE user_id = $1 AND type = $2 AND read_at IS NULL",
    )
    .bind(user_id)
    .bind(alert_type)
    .fetch_all(&mut *conn)
    .await
}

/// Retype an alert to a terminal state (e.g. a harvest opportunity that lapsed
/// unacted → `missed_harvest`) with a new message/payload. Stays unread so the
/// user still sees it; `updated_at` marks when the transition happened.
pub async fn retype(
    conn: &mut PgConnection,
    id: Uuid,
    new_type: &str,
    message: &str,
    payload: Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE alerts SET type = $2, message = $3, payload = $4, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(new_type)
    .bind(message)
    .bind(payload)
    .execute(&mut *conn)
    .await?;
    Ok(())
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
