//! Tax endpoints (`/api/tax/*`): unrealized gain summary, tax-loss-harvest
//! candidates, and a specific-lot sell simulator. All numbers are estimates
//! for decision-support — **not tax advice**.

use std::collections::HashMap;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, NaiveDate, Utc};
use domain::tax::{self, Term};
use domain::FilingStatus;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

/// Wash-sale window: a purchase within this many days of a loss sale taints it.
const WASH_SALE_DAYS: i64 = 30;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tax/summary", get(summary))
        .route("/api/tax/harvest", get(harvest))
        .route("/api/tax/simulate", post(simulate))
}

fn parse_status(s: &str) -> FilingStatus {
    match s {
        "married_filing_jointly" => FilingStatus::MarriedFilingJointly,
        "married_filing_separately" => FilingStatus::MarriedFilingSeparately,
        "head_of_household" => FilingStatus::HeadOfHousehold,
        _ => FilingStatus::Single,
    }
}

fn today() -> NaiveDate {
    Utc::now().date_naive()
}

// ---------- GET /api/tax/summary ----------

#[derive(Serialize)]
struct TaxSummary {
    as_of: NaiveDate,
    total_cost_basis: Decimal,
    total_market_value: Decimal,
    unrealized_short_term: Decimal,
    unrealized_long_term: Decimal,
    total_unrealized: Decimal,
    estimated_tax_if_sold_now: tax::TaxEstimate,
    lots_valued: usize,
    lots_unpriced: usize,
}

async fn summary(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<TaxSummary>, AppError> {
    let user = auth.0;
    let status = parse_status(&user.filing_status);
    let lots = db::queries::tax_lots::list_open_with_price(&state.db, user.id).await?;
    let as_of = today();

    let (mut basis, mut value, mut st, mut lt) =
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    let mut valued = 0usize;
    let mut unpriced = 0usize;

    for lot in &lots {
        let Some(price) = lot.close_price else {
            unpriced += 1;
            continue;
        };
        let g = tax::lot_gain(
            lot.remaining_quantity,
            lot.cost_basis_per_share,
            price,
            lot.open_date,
            as_of,
        );
        basis += g.cost_basis;
        value += g.market_value;
        match g.term {
            Term::ShortTerm => st += g.gain,
            Term::LongTerm => lt += g.gain,
        }
        valued += 1;
    }

    // Fold in accounts valued from Plaid's balance (holdings unavailable, so no
    // lots). They add to market value only — we have no cost basis for them, so
    // they don't affect unrealized gain/loss or the tax estimate.
    for acct in db::queries::accounts::balance_only_accounts(&state.db, user.id).await? {
        value += acct.current_balance;
    }

    let estimate = tax::estimate_liquidation(status, user.taxable_income, st, lt);

    Ok(Json(TaxSummary {
        as_of,
        total_cost_basis: basis,
        total_market_value: value,
        unrealized_short_term: st,
        unrealized_long_term: lt,
        total_unrealized: st + lt,
        estimated_tax_if_sold_now: estimate,
        lots_valued: valued,
        lots_unpriced: unpriced,
    }))
}

// ---------- GET /api/tax/harvest ----------

#[derive(Serialize)]
struct HarvestCandidate {
    lot_id: Uuid,
    security_id: Uuid,
    account_id: Uuid,
    ticker: Option<String>,
    open_date: NaiveDate,
    term: Term,
    quantity: Decimal,
    cost_basis: Decimal,
    market_value: Decimal,
    unrealized_loss: Decimal,
    estimated_tax_saving: Decimal,
    wash_sale_warning: bool,
}

async fn harvest(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = auth.0;
    let status = parse_status(&user.filing_status);
    let as_of = today();
    let lots = db::queries::tax_lots::list_open_with_price(&state.db, user.id).await?;

    // Retirement accounts (IRA/401k/…) are tax-advantaged — harvesting there has
    // no tax benefit, so their lots are never candidates.
    let retirement: std::collections::HashSet<Uuid> =
        db::queries::accounts::list(&state.db, user.id)
            .await?
            .into_iter()
            .filter(|a| {
                domain::accounts::AccountKind::resolve(
                    a.subtype.as_deref(),
                    a.kind_override.as_deref(),
                )
                .is_retirement()
            })
            .map(|a| a.id)
            .collect();

    let since = as_of - Duration::days(WASH_SALE_DAYS);
    let recent_buys: std::collections::HashSet<Uuid> =
        db::queries::transactions::recent_buy_security_ids(&state.db, user.id, since)
            .await?
            .into_iter()
            .collect();

    let mut candidates = Vec::new();
    for lot in &lots {
        if retirement.contains(&lot.account_id) {
            continue; // no harvesting in tax-advantaged accounts
        }
        let Some(price) = lot.close_price else {
            continue;
        };
        let g = tax::lot_gain(
            lot.remaining_quantity,
            lot.cost_basis_per_share,
            price,
            lot.open_date,
            as_of,
        );
        if g.gain >= Decimal::ZERO {
            continue; // only losses are harvestable
        }
        // Realizing a loss is a saving → estimate.total is negative; flip sign.
        let estimate = tax::estimate_tax(status, user.taxable_income, g.term, g.gain);
        candidates.push(HarvestCandidate {
            lot_id: lot.id,
            security_id: lot.security_id,
            account_id: lot.account_id,
            ticker: lot.ticker.clone(),
            open_date: lot.open_date,
            term: g.term,
            quantity: lot.remaining_quantity,
            cost_basis: g.cost_basis,
            market_value: g.market_value,
            unrealized_loss: g.gain,
            estimated_tax_saving: -estimate.total,
            wash_sale_warning: recent_buys.contains(&lot.security_id),
        });
    }

    Ok(Json(serde_json::json!({ "candidates": candidates })))
}

// ---------- POST /api/tax/simulate ----------

#[derive(Deserialize)]
struct SimulateReq {
    sales: Vec<SaleRequest>,
}

#[derive(Deserialize)]
struct SaleRequest {
    lot_id: Uuid,
    /// Shares to sell from this lot; defaults to the full remaining quantity.
    quantity: Option<Decimal>,
}

#[derive(Serialize)]
struct SaleResult {
    lot_id: Uuid,
    ticker: Option<String>,
    term: Term,
    quantity: Decimal,
    cost_basis: Decimal,
    proceeds: Decimal,
    gain: Decimal,
}

#[derive(Serialize)]
struct SimulateResp {
    sales: Vec<SaleResult>,
    total_proceeds: Decimal,
    total_cost_basis: Decimal,
    short_term_gain: Decimal,
    long_term_gain: Decimal,
    total_gain: Decimal,
    estimated_tax: tax::TaxEstimate,
    after_tax_proceeds: Decimal,
}

async fn simulate(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SimulateReq>,
) -> Result<Json<SimulateResp>, AppError> {
    if req.sales.is_empty() {
        return Err(AppError::BadRequest(
            "provide at least one lot to sell".into(),
        ));
    }
    let user = auth.0;
    let status = parse_status(&user.filing_status);
    let as_of = today();

    let lots = db::queries::tax_lots::list_open_with_price(&state.db, user.id).await?;
    let by_id: HashMap<Uuid, _> = lots.into_iter().map(|l| (l.id, l)).collect();

    let (mut proceeds, mut basis, mut st, mut lt) =
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    let mut results = Vec::new();

    for sale in &req.sales {
        let lot = by_id
            .get(&sale.lot_id)
            .ok_or_else(|| AppError::BadRequest(format!("unknown lot {}", sale.lot_id)))?;
        let price = lot.close_price.ok_or_else(|| {
            AppError::BadRequest(format!("lot {} has no current price", sale.lot_id))
        })?;
        // Clamp the requested quantity to what the lot actually holds.
        let qty = sale
            .quantity
            .unwrap_or(lot.remaining_quantity)
            .min(lot.remaining_quantity);
        if qty <= Decimal::ZERO {
            return Err(AppError::BadRequest(format!(
                "quantity for lot {} must be positive",
                sale.lot_id
            )));
        }

        let g = tax::lot_gain(qty, lot.cost_basis_per_share, price, lot.open_date, as_of);
        proceeds += g.market_value;
        basis += g.cost_basis;
        match g.term {
            Term::ShortTerm => st += g.gain,
            Term::LongTerm => lt += g.gain,
        }
        results.push(SaleResult {
            lot_id: lot.id,
            ticker: lot.ticker.clone(),
            term: g.term,
            quantity: qty,
            cost_basis: g.cost_basis,
            proceeds: g.market_value,
            gain: g.gain,
        });
    }

    let estimate = tax::estimate_liquidation(status, user.taxable_income, st, lt);

    Ok(Json(SimulateResp {
        sales: results,
        total_proceeds: proceeds,
        total_cost_basis: basis,
        short_term_gain: st,
        long_term_gain: lt,
        total_gain: st + lt,
        after_tax_proceeds: proceeds - estimate.total,
        estimated_tax: estimate,
    }))
}
