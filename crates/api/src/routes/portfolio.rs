//! Read-only portfolio endpoints + lot rebuild (all under `/api`).
//!
//! - `GET  /api/accounts`     → linked accounts
//! - `GET  /api/holdings`     → current positions (joined w/ security)
//! - `GET  /api/transactions` → recent transactions (`?limit=`, default 200)
//! - `GET  /api/lots`         → reconstructed open tax lots
//! - `POST /api/lots/rebuild` → re-run FIFO reconstruction now

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

const DEFAULT_TXN_LIMIT: i64 = 200;
const MAX_TXN_LIMIT: i64 = 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/accounts", get(list_accounts))
        .route("/api/holdings", get(list_holdings))
        .route("/api/transactions", get(list_transactions))
        .route("/api/lots", get(list_lots))
        .route("/api/lots/rebuild", post(rebuild_lots))
}

async fn list_accounts(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let accounts = db::queries::accounts::list(&state.db, user.0.id).await?;
    Ok(Json(json!({ "accounts": accounts })))
}

async fn list_holdings(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let holdings = db::queries::holdings::list_with_security(&state.db, user.0.id).await?;
    Ok(Json(json!({ "holdings": holdings })))
}

#[derive(Deserialize)]
struct TxnQuery {
    limit: Option<i64>,
}

async fn list_transactions(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<TxnQuery>,
) -> Result<Json<Value>, AppError> {
    let limit = q.limit.unwrap_or(DEFAULT_TXN_LIMIT).clamp(1, MAX_TXN_LIMIT);
    let transactions = db::queries::transactions::list(&state.db, user.0.id, limit).await?;
    Ok(Json(json!({ "transactions": transactions })))
}

async fn list_lots(State(state): State<AppState>, user: AuthUser) -> Result<Json<Value>, AppError> {
    let lots = db::queries::tax_lots::list_with_security(&state.db, user.0.id).await?;
    Ok(Json(json!({ "lots": lots })))
}

async fn rebuild_lots(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let count = crate::lots::rebuild_lots(&state.db, user.0.id).await?;
    Ok(Json(json!({ "rebuilt": count })))
}
