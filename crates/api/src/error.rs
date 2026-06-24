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
            // Plaid is an upstream dependency, so its failures are a bad gateway.
            AppError::Plaid(_) => StatusCode::BAD_GATEWAY,
            AppError::Db(_) | AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        // Log the full error server-side; return a clean message to the client.
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
        }
        // Don't leak upstream (Plaid) internals to the client; log them above and
        // return a generic body. Other arms expose their own (safe) message.
        let body = match &self {
            AppError::Plaid(e) => {
                tracing::error!(error = %e, "plaid upstream error");
                "upstream error".to_string()
            }
            other => other.to_string(),
        };
        (status, Json(json!({ "error": body }))).into_response()
    }
}
