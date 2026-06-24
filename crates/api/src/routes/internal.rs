//! Internal endpoints (`/api/internal/*`) — driven by infrastructure, not users.
//!
//! In production the in-process scheduler is off; Cloud Scheduler POSTs here on a
//! cron to run the alert cycle. The preferred auth is Cloud Run IAM/OIDC (the
//! invoker identity is enforced by the platform). As a portable fallback we also
//! accept a `Bearer` token compared in constant time against `INTERNAL_API_TOKEN`.
//!
//! These routes are cookie-free, so the CSRF guard skips them (see `csrf.rs`);
//! they still sit behind the global body-limit + timeout layers.

use axum::extract::State;
use axum::http::{header::AUTHORIZATION, HeaderMap};
use axum::routing::post;
use axum::{Json, Router};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::alert_engine::{self, CycleSummary};
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/internal/alerts/run", post(run_alerts))
}

/// `POST /api/internal/alerts/run` — run the alert cycle for every user and reap
/// expired sessions. Returns the aggregate summary.
async fn run_alerts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CycleSummary>, AppError> {
    require_internal_auth(&state, &headers)?;
    let summary = alert_engine::run_cycle_all_users(&state).await?;
    Ok(Json(summary))
}

/// Validate the `Authorization: Bearer <token>` header against the configured
/// internal token. Both sides are SHA-256'd first so the constant-time compare
/// operates on equal-length inputs and never leaks token length. If no token is
/// configured the endpoint is closed (401) rather than open.
fn require_internal_auth(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let expected = state
        .config
        .internal_api_token
        .as_deref()
        .ok_or(AppError::Unauthorized)?;

    let provided = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or(AppError::Unauthorized)?;

    let expected_hash = Sha256::digest(expected.as_bytes());
    let provided_hash = Sha256::digest(provided.as_bytes());
    if provided_hash.ct_eq(&expected_hash).into() {
        Ok(())
    } else {
        Err(AppError::Unauthorized)
    }
}
