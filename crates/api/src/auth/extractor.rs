//! The `AuthUser` extractor: the single gate protecting authenticated routes.
//!
//! A handler that takes `AuthUser` can only run for a request carrying a valid,
//! unexpired session cookie. Any failure — missing cookie, unknown/expired
//! token — yields a uniform 401 with no detail.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use db::models::User;

use crate::auth::{cookie_name, session};
use crate::error::AppError;
use crate::state::AppState;

/// An authenticated user, resolved from the session cookie. `.0` is the row.
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let name = cookie_name(state.config.cookie_secure);
        let raw = jar
            .get(name)
            .map(|c| c.value())
            .ok_or(AppError::Unauthorized)?;

        let token_hash = session::hash_token(raw);
        let (sess, user) = db::queries::sessions::find_valid_by_token_hash(&state.db, &token_hash)
            .await?
            .ok_or(AppError::Unauthorized)?;

        // Cheap, throttled liveness bump (no-op unless stale) — see `touch_if_stale`.
        db::queries::sessions::touch_if_stale(&state.db, sess.id).await?;

        Ok(AuthUser(user))
    }
}
