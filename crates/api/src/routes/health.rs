//! Health/readiness endpoint. `/health` is liveness; it also pings the DB so a
//! 200 means the service can actually serve requests.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    // `SELECT 1` confirms the pool can reach Postgres.
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await?;

    Ok(Json(json!({
        "status": "ok",
        "db": "up",
        "plaid_env": format!("{:?}", state.plaid.env()),
    })))
}
