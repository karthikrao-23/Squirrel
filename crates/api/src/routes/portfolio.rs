//! Read-only portfolio endpoints + lot rebuild (all under `/api`).
//!
//! - `GET  /api/accounts`     → linked accounts
//! - `GET  /api/holdings`     → current positions (joined w/ security)
//! - `GET  /api/transactions` → recent transactions (`?limit=`, default 200)
//! - `GET  /api/lots`         → reconstructed open tax lots
//! - `POST /api/lots/rebuild` → re-run FIFO reconstruction now
//! - `GET  /api/portfolio/history` → daily value/cost-basis snapshots

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

const DEFAULT_TXN_LIMIT: i64 = 200;
const MAX_TXN_LIMIT: i64 = 1000;

/// A lot held in an account, with its long/short-term classification. Mirrors
/// [`db::queries::tax_lots::LotByAccount`] plus a computed `term`.
#[derive(Serialize)]
struct AccountLot {
    id: uuid::Uuid,
    account_id: uuid::Uuid,
    account_name: String,
    account_subtype: Option<String>,
    security_id: uuid::Uuid,
    ticker: Option<String>,
    open_date: chrono::NaiveDate,
    term: &'static str,
    remaining_quantity: Decimal,
    cost_basis_per_share: Decimal,
    close_price: Option<Decimal>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/accounts", get(list_accounts))
        .route("/api/accounts/lots", get(list_account_lots))
        .route("/api/holdings", get(list_holdings))
        .route("/api/transactions", get(list_transactions))
        .route("/api/lots", get(list_lots))
        .route("/api/lots/rebuild", post(rebuild_lots))
        .route("/api/portfolio/history", get(portfolio_history))
}

async fn list_accounts(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let accounts = db::queries::accounts::list(&state.db, user.0.id).await?;
    Ok(Json(json!({ "accounts": accounts })))
}

async fn list_account_lots(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let lots = db::queries::tax_lots::list_open_with_account(&state.db, user.0.id).await?;
    // A lot held for under a year is short-term; the boundary is exactly one
    // year before today.
    let one_year_ago = Utc::now().date_naive() - Duration::days(365);
    let lots: Vec<AccountLot> = lots
        .into_iter()
        .map(|l| {
            let term = if l.open_date > one_year_ago {
                "short_term"
            } else {
                "long_term"
            };
            AccountLot {
                id: l.id,
                account_id: l.account_id,
                account_name: l.account_name,
                account_subtype: l.account_subtype,
                security_id: l.security_id,
                ticker: l.ticker,
                open_date: l.open_date,
                term,
                remaining_quantity: l.remaining_quantity,
                cost_basis_per_share: l.cost_basis_per_share,
                close_price: l.close_price,
            }
        })
        .collect();
    Ok(Json(json!({ "lots": lots })))
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

async fn portfolio_history(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let history = db::queries::snapshots::history(&state.db, user.0.id).await?;
    Ok(Json(json!({ "history": history })))
}
