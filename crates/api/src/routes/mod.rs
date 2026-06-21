//! HTTP route definitions. New feature routers (plaid, portfolio, tax, alerts)
//! are merged in here as later milestones land.

pub mod health;

use axum::Router;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .with_state(state)
}
