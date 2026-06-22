//! Alert endpoints (`/api/alerts`): list, mark read, and a manual evaluate
//! trigger (handy for testing without waiting for the scheduler).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

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
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, AppError> {
    let user = db::queries::users::ensure_default(&state.db).await?;
    let alerts = db::queries::alerts::list(&state.db, user.id, q.unread_only).await?;
    Ok(Json(json!({ "alerts": alerts })))
}

async fn mark_read(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let user = db::queries::users::ensure_default(&state.db).await?;
    let changed = db::queries::alerts::mark_read(&state.db, user.id, id).await?;
    if !changed {
        return Err(AppError::NotFound);
    }
    Ok(Json(json!({ "ok": true })))
}

/// Run the alert rules now and return how many new alerts were created (and
/// emails sent, if SMTP is configured).
async fn evaluate(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let created = crate::alert_engine::evaluate_and_store(&state).await?;
    let emailed = crate::alert_engine::send_pending_emails(&state).await?;
    Ok(Json(json!({ "created": created, "emailed": emailed })))
}
