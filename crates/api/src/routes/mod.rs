//! HTTP route definitions. New feature routers (plaid, portfolio, tax, alerts)
//! are merged in here as later milestones land.

pub mod alerts;
pub mod auth;
pub mod health;
pub mod internal;
pub mod plaid;
pub mod portfolio;
pub mod profile;
pub mod tax;

use axum::routing::any;
use axum::Router;

use crate::error::AppError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(internal::router())
        .merge(plaid::router())
        .merge(portfolio::router())
        .merge(profile::router())
        .merge(tax::router())
        .merge(alerts::router())
        // Unmatched `/api/*` returns a JSON 404 so the SPA static fallback can't
        // swallow an unknown API path into a 200 HTML page.
        .route("/api/{*rest}", any(api_not_found))
        .with_state(state)
}

async fn api_not_found() -> AppError {
    AppError::NotFound
}
