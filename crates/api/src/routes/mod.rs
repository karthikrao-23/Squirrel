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

use axum::Router;

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
        .with_state(state)
}
