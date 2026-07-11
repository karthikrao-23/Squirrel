//! Alert endpoints (`/api/alerts`): list, mark read, and a manual evaluate
//! trigger (handy for testing without waiting for the scheduler).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/alerts", get(list))
        .route("/api/alerts/{id}/read", post(mark_read))
        .route("/api/alerts/evaluate", post(evaluate))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    unread_only: bool,
}

async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, AppError> {
    let mut tx = db::begin_as_user(&state.db, user.0.id).await?;
    let alerts = db::queries::alerts::list(&mut tx, user.0.id, q.unread_only).await?;
    tx.commit().await?;
    Ok(Json(json!({ "alerts": alerts })))
}

async fn mark_read(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    // `mark_read` scopes by user_id, so marking another user's alert simply
    // changes nothing → 404 (no cross-tenant write, no existence leak beyond it).
    let mut tx = db::begin_as_user(&state.db, user.0.id).await?;
    let changed = db::queries::alerts::mark_read(&mut tx, user.0.id, id).await?;
    tx.commit().await?;
    if !changed {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

/// Run the alert cycle for the calling user and return how many new alerts were
/// created (and emails sent, if SMTP is configured).
async fn evaluate(State(state): State<AppState>, user: AuthUser) -> Result<Json<Value>, AppError> {
    let summary = crate::alert_engine::run_cycle_for_user(&state, &user.0).await?;
    Ok(Json(json!({
        "created": summary.alerts_created,
        "emailed": summary.emails_sent,
    })))
}
