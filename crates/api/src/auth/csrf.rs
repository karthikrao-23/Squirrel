//! CSRF guard middleware for mutating requests.
//!
//! `SameSite=Strict` already blocks most cross-site sends, but it isn't a
//! guarantee for financial mutations, so we add defense in depth:
//!   1. A required custom header (`X-Squirrel-CSRF`). Cross-site JS can't set a
//!      custom header without a CORS preflight, which we never allow.
//!   2. An `Origin`/`Referer` check against our own origin when one is present.
//!
//! Safe methods (GET/HEAD/OPTIONS/TRACE) are never challenged. Applied to all
//! mutating `/api` routes including `/api/auth/*` (login-CSRF / fixation).

use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::auth::CSRF_HEADER;
use crate::state::AppState;

pub async fn csrf_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    // Safe, non-mutating methods don't need protection.
    if matches!(
        *req.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) {
        return next.run(req).await;
    }

    // The CSRF guard protects *cookie*-authenticated mutations. These routes use
    // a different, non-cookie credential and are called by infrastructure /
    // Plaid that can't supply our header, so they're exempt:
    //   - `/api/internal/*`     — bearer token
    //   - `/api/plaid/webhook`  — Plaid's signed `Plaid-Verification` JWT
    // (A browser CSRF can forge neither credential, so skipping is safe.)
    let path = req.uri().path();
    if path.starts_with("/api/internal/") || path == "/api/plaid/webhook" {
        return next.run(req).await;
    }

    let headers = req.headers();

    // 1. Required custom header.
    if !headers.contains_key(CSRF_HEADER) {
        return reject("missing CSRF header");
    }

    // 2. Origin/Referer must match our origin when present and we know it.
    if let Some(expected) = state.config.app_origin.as_deref() {
        if let Some(origin) = headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
        {
            if origin != expected {
                return reject("cross-origin request rejected");
            }
        } else if let Some(referer) = headers
            .get(header::REFERER)
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
        {
            // Referer carries a full URL; it's same-origin iff it's under us.
            if !referer.starts_with(expected) {
                return reject("cross-origin request rejected");
            }
        }
    }

    next.run(req).await
}

fn reject(reason: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({ "error": reason }))).into_response()
}
