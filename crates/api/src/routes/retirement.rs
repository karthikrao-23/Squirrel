//! Retirement accounts — a performance view (not a tax view). Tax-advantaged
//! accounts (IRA/Roth/401k/…) don't harvest, so this endpoint reports how the
//! retirement holdings, **as a whole**, are performing: value, cost basis, total
//! return, money-weighted IRR (from the lots), time-weighted TWR (from daily
//! snapshots as they accrue), and the aggregate value history for charting.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use domain::accounts::AccountKind;
use domain::performance::{self, TwrPoint};

pub fn router() -> Router<AppState> {
    Router::new().route("/api/retirement", get(summary))
}

/// `GET /api/retirement`
async fn summary(State(state): State<AppState>, user: AuthUser) -> Result<Json<Value>, AppError> {
    let lots = db::queries::tax_lots::list_open_with_account(&state.db, user.0.id).await?;

    // Per-account aggregates + the IRR cash-flow series (each lot's cost is an
    // outflow on its acquisition date).
    let mut accounts: BTreeMap<String, (Option<String>, Decimal, Decimal)> = BTreeMap::new();
    let mut flows: Vec<(chrono::NaiveDate, f64)> = Vec::new();
    let mut total_mv = Decimal::ZERO;
    let mut total_cb = Decimal::ZERO;

    for lot in &lots {
        if !AccountKind::resolve(
            lot.account_subtype.as_deref(),
            lot.account_kind_override.as_deref(),
        )
        .is_retirement()
        {
            continue;
        }
        let cb = lot.remaining_quantity * lot.cost_basis_per_share;
        let mv = lot
            .close_price
            .map(|p| lot.remaining_quantity * p)
            .unwrap_or(Decimal::ZERO);
        total_mv += mv;
        total_cb += cb;
        let entry = accounts.entry(lot.account_name.clone()).or_insert((
            lot.account_subtype.clone(),
            Decimal::ZERO,
            Decimal::ZERO,
        ));
        entry.1 += mv;
        entry.2 += cb;
        if let Some(cost) = cb.to_f64() {
            if cost > 0.0 {
                flows.push((lot.open_date, -cost));
            }
        }
    }

    // Terminal inflow = current value → solve money-weighted IRR.
    let today = Utc::now().date_naive();
    if let Some(mv) = total_mv.to_f64() {
        flows.push((today, mv));
    }
    let irr = performance::xirr(&flows);

    // Time-weighted return from the retirement value history (accrues daily).
    let history = db::queries::snapshots::history(&state.db, user.0.id, "retirement").await?;
    let twr_points: Vec<TwrPoint> = history
        .iter()
        .map(|s| TwrPoint {
            value: s.market_value.to_f64().unwrap_or(0.0),
            flow: 0.0,
        })
        .collect();
    let twr = performance::twr(&twr_points);

    let simple_return = if total_cb > Decimal::ZERO {
        (total_mv - total_cb)
            .to_f64()
            .zip(total_cb.to_f64())
            .map(|(g, c)| g / c)
    } else {
        None
    };

    // Retirement accounts valued from Plaid's balance (holdings unavailable, so no
    // lots). They count toward the total *value* but not the return metrics —
    // without cost basis or a value history we can't attribute a return to them.
    let balance_only: Vec<_> = db::queries::accounts::balance_only_accounts(&state.db, user.0.id)
        .await?
        .into_iter()
        .filter(|a| {
            AccountKind::resolve(a.subtype.as_deref(), a.kind_override.as_deref()).is_retirement()
        })
        .collect();
    let balance_only_value: Decimal = balance_only.iter().map(|a| a.current_balance).sum();

    let mut account_rows: Vec<Value> = accounts
        .into_iter()
        .map(|(name, (subtype, mv, cb))| {
            json!({
                "name": name,
                "subtype": subtype,
                "market_value": mv,
                "cost_basis": cb,
                "unrealized": mv - cb,
                "balance_only": false,
            })
        })
        .collect();
    for a in &balance_only {
        account_rows.push(json!({
            "name": a.name,
            "subtype": a.subtype,
            "market_value": a.current_balance,
            "cost_basis": Value::Null,
            "unrealized": Value::Null,
            "balance_only": true,
        }));
    }

    Ok(Json(json!({
        "accounts": account_rows,
        "total": {
            // Value includes balance-only accounts; the return metrics below are
            // lot-based and exclude them (see `return_excludes`).
            "market_value": total_mv + balance_only_value,
            "cost_basis": total_cb,
            "unrealized": total_mv - total_cb,
            "simple_return": simple_return,
            "irr": irr,
            "twr": twr,
            "return_excludes": balance_only.len(),
        },
        "history": history,
    })))
}
