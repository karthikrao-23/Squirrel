//! A single application error type that converts into an HTTP response. This is
//! the idiomatic Axum pattern: handlers return `Result<T, AppError>` and `?`
//! works against any error that converts into `AppError`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("plaid error: {0}")]
    Plaid(#[from] plaid::PlaidError),

    /// Caller asked for something that requires configuration we don't have.
    #[error("{0}")]
    BadRequest(String),

    /// Missing/invalid/expired session. Body is a fixed generic message so it
    /// can't be used to distinguish "no session" from "bad credentials".
    #[error("unauthorized")]
    Unauthorized,

    /// A uniqueness conflict the client can resolve (e.g. email already taken).
    #[error("{0}")]
    Conflict(String),

    /// Authenticated (or doesn't need a session) but not allowed here — e.g. a
    /// dev-only endpoint hit in production.
    #[error("{0}")]
    Forbidden(String),

    #[error("not found")]
    NotFound,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            // Plaid is an upstream dependency, so its failures are a bad gateway.
            AppError::Plaid(_) => StatusCode::BAD_GATEWAY,
            AppError::Db(_) | AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        // Log the full error server-side; return a clean message to the client.
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
        }
        // Never return internal detail to the client. The 500-class arms (`Db`,
        // `Other`) carry schema/constraint names and internal error chains, and
        // `Plaid` carries upstream internals — all are logged above/here and
        // replaced with a fixed generic body. The remaining arms (`BadRequest`,
        // `Conflict`, `Forbidden`, `Unauthorized`, `NotFound`) carry only
        // deliberate, client-actionable messages, so they pass through.
        let body = match &self {
            AppError::Plaid(e) => {
                tracing::error!(error = %e, "plaid upstream error");
                "upstream error".to_string()
            }
            AppError::Db(_) | AppError::Other(_) => "internal error".to_string(),
            other => other.to_string(),
        };
        (status, Json(json!({ "error": body }))).into_response()
    }
}
