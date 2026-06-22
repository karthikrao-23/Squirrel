//! User tax profile (`/api/profile`). Holds the filing status + taxable income
//! the tax engine (M4) needs. v1 is single-user, so there's no id in the path.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::error::AppError;
use crate::state::AppState;
use db::models::User;

/// Valid federal filing statuses (must match `domain::FilingStatus` serde).
const FILING_STATUSES: [&str; 4] = [
    "single",
    "married_filing_jointly",
    "married_filing_separately",
    "head_of_household",
];

pub fn router() -> Router<AppState> {
    Router::new().route("/api/profile", get(get_profile).patch(update_profile))
}

async fn get_profile(State(state): State<AppState>) -> Result<Json<User>, AppError> {
    let user = db::queries::users::ensure_default(&state.db).await?;
    Ok(Json(user))
}

#[derive(Deserialize)]
struct ProfileUpdate {
    filing_status: Option<String>,
    taxable_income: Option<Decimal>,
}

async fn update_profile(
    State(state): State<AppState>,
    Json(body): Json<ProfileUpdate>,
) -> Result<Json<User>, AppError> {
    let current = db::queries::users::ensure_default(&state.db).await?;

    // Apply provided fields over the current values (partial update).
    let filing_status = body.filing_status.unwrap_or(current.filing_status);
    if !FILING_STATUSES.contains(&filing_status.as_str()) {
        return Err(AppError::BadRequest(format!(
            "invalid filing_status '{filing_status}'; expected one of {FILING_STATUSES:?}"
        )));
    }
    let taxable_income = body.taxable_income.unwrap_or(current.taxable_income);
    if taxable_income.is_sign_negative() {
        return Err(AppError::BadRequest(
            "taxable_income must not be negative".into(),
        ));
    }

    let updated =
        db::queries::users::update_profile(&state.db, current.id, &filing_status, taxable_income)
            .await?;
    Ok(Json(updated))
}
