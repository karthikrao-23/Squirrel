//! Read-only portfolio endpoints + lot rebuild (all under `/api`).
//!
//! - `GET  /api/accounts`     → linked accounts
//! - `GET  /api/holdings`     → current positions (joined w/ security)
//! - `GET  /api/transactions` → recent transactions (`?limit=`, default 200)
//! - `GET  /api/lots`         → reconstructed open tax lots
//! - `POST /api/lots/rebuild` → re-run FIFO reconstruction now
//! - `GET  /api/portfolio/history` → daily value/cost-basis snapshots

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
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
/// [`db::queries::tax_lots::LotByAccount`] plus a computed `term` and the
/// account's resolved tax `kind` (+ the raw override that produced it).
#[derive(Serialize)]
struct AccountLot {
    id: uuid::Uuid,
    account_id: uuid::Uuid,
    account_name: String,
    account_subtype: Option<String>,
    /// Effective tax classification ("taxable"/"retirement") after any override.
    account_kind: &'static str,
    /// The user's raw override, or null when classified automatically. Lets the
    /// UI show whether the kind was set by hand or derived from the subtype.
    account_kind_override: Option<String>,
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
        .route("/api/accounts/{id}/kind", patch(set_account_kind))
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
    // Tag each account taxable vs retirement (derived from its Plaid subtype).
    let enriched: Vec<Value> = accounts
        .iter()
        .map(|a| {
            let mut v = serde_json::to_value(a).unwrap_or_else(|_| json!({}));
            if let Some(obj) = v.as_object_mut() {
                let kind = domain::accounts::AccountKind::resolve(
                    a.subtype.as_deref(),
                    a.kind_override.as_deref(),
                );
                obj.insert("kind".into(), json!(kind.as_str()));
            }
            v
        })
        .collect();
    Ok(Json(json!({ "accounts": enriched })))
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
            let account_kind = domain::accounts::AccountKind::resolve(
                l.account_subtype.as_deref(),
                l.account_kind_override.as_deref(),
            )
            .as_str();
            AccountLot {
                id: l.id,
                account_id: l.account_id,
                account_name: l.account_name,
                account_subtype: l.account_subtype,
                account_kind,
                account_kind_override: l.account_kind_override,
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

    // Accounts with no lots that we value from Plaid's balance (holdings
    // unavailable). Returned alongside the lots so the page can show them as
    // value-only cards rather than dropping them.
    let balance_only: Vec<Value> =
        db::queries::accounts::balance_only_accounts(&state.db, user.0.id)
            .await?
            .into_iter()
            .map(|a| {
                let kind = domain::accounts::AccountKind::resolve(
                    a.subtype.as_deref(),
                    a.kind_override.as_deref(),
                )
                .as_str();
                json!({
                    "account_id": a.account_id,
                    "name": a.name,
                    "subtype": a.subtype,
                    "kind": kind,
                    "kind_override": a.kind_override,
                    "current_balance": a.current_balance,
                })
            })
            .collect();

    Ok(Json(json!({ "lots": lots, "balance_only": balance_only })))
}

/// Body for `PATCH /api/accounts/{id}/kind`. `kind: null` clears the override
/// (revert to automatic classification from the Plaid subtype).
#[derive(Deserialize)]
struct SetKindReq {
    kind: Option<String>,
}

/// `PATCH /api/accounts/{id}/kind` — manually override an account's tax
/// classification (or clear the override). Correcting a misclassified account
/// changes what the harvest, retirement, and dashboard views count it as, so we
/// only need to persist it; the reads resolve the override on the fly.
async fn set_account_kind(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<uuid::Uuid>,
    Json(req): Json<SetKindReq>,
) -> Result<Json<Value>, AppError> {
    // Only the two pinned kinds are valid; null means "clear". Reject anything
    // else up front so we never store a value the resolver would ignore.
    match req.kind.as_deref() {
        None | Some("taxable") | Some("retirement") => {}
        Some(other) => {
            return Err(AppError::BadRequest(format!(
                "kind must be \"taxable\", \"retirement\", or null; got {other:?}"
            )))
        }
    }

    let updated =
        db::queries::accounts::set_kind_override(&state.db, user.0.id, id, req.kind.as_deref())
            .await?
            .ok_or(AppError::NotFound)?;

    let kind = domain::accounts::AccountKind::resolve(
        updated.subtype.as_deref(),
        updated.kind_override.as_deref(),
    );
    Ok(Json(json!({
        "account_id": updated.id,
        "kind": kind.as_str(),
        "kind_override": updated.kind_override,
    })))
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
    let history = db::queries::snapshots::history(&state.db, user.0.id, "total").await?;
    Ok(Json(json!({ "history": history })))
}
