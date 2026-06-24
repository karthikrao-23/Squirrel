//! Authentication routes (`/api/auth/*`): signup, login, logout, logout-all, me.
//!
//! Cookies are same-origin, `HttpOnly`, `SameSite=Strict`. The CSRF guard and
//! rate limiter are applied to these routes in `lib.rs::build_app`, not here.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::{header::SET_COOKIE, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use db::models::User;
use serde::Deserialize;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tower_governor::GovernorLayer;
use unicode_normalization::UnicodeNormalization;

use crate::auth::{clear_cookie, password, session, session_cookie, AuthUser};
use crate::error::AppError;
use crate::state::AppState;

/// RFC 5321 caps an address at 254 chars; we also enforce a sane password band.
const MAX_EMAIL_LEN: usize = 254;
const MIN_PASSWORD_LEN: usize = 12;
const MAX_PASSWORD_LEN: usize = 128;

/// Rate limit on the unauthenticated argon2 routes: ~10 requests/minute/IP
/// (one token replenished every 6s, burst 10). Keyed on the real client IP from
/// `X-Forwarded-For` (set by Cloud Run's front end), not the socket peer.
const AUTH_RATE_PERIOD: Duration = Duration::from_secs(6);
const AUTH_RATE_BURST: u32 = 10;

pub fn router() -> Router<AppState> {
    // login + signup are the expensive, unauthenticated surface — rate-limit them.
    let mut builder = GovernorConfigBuilder::default().key_extractor(SmartIpKeyExtractor);
    builder.period(AUTH_RATE_PERIOD).burst_size(AUTH_RATE_BURST);
    let governor_conf = Arc::new(builder.finish().expect("valid governor config"));
    let rate_limited = Router::new()
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .layer(GovernorLayer {
            config: governor_conf,
        });

    let rest = Router::new()
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/logout-all", post(logout_all))
        .route("/api/auth/me", get(me));

    rate_limited.merge(rest)
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

// ---------- POST /api/auth/signup ----------

async fn signup(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<impl IntoResponse, AppError> {
    let email = normalize_email(&body.email)?;
    let password = validate_password(&body.password)?;

    let password_hash = password::hash_password(&password)?;
    let user = db::queries::users::create(&state.db, &email, &password_hash)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                AppError::Conflict("email already registered".into())
            }
            other => AppError::Db(other),
        })?;

    let set_cookie = issue_session(&state, user.id).await?;
    Ok(([(SET_COOKIE, set_cookie)], Json(user)))
}

// ---------- POST /api/auth/login ----------

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> Result<impl IntoResponse, AppError> {
    let email = normalize_email(&body.email)?;
    let password = normalize_password(&body.password);

    // Always run exactly one verify so an unknown email and a wrong password
    // cost the same. Against an unknown user (or a user with no hash) we verify
    // the throwaway dummy hash and discard the result.
    let found = db::queries::users::find_by_email(&state.db, &email).await?;
    let (verified, user) = match found {
        Some(u) => match &u.password_hash {
            Some(hash) => (password::verify_password(&password, hash), Some(u)),
            None => {
                password::verify_password(&password, &state.dummy_password_hash);
                (false, None)
            }
        },
        None => {
            password::verify_password(&password, &state.dummy_password_hash);
            (false, None)
        }
    };

    if !verified {
        return Err(AppError::Unauthorized);
    }
    let user = user.expect("verified implies a user was found");

    // Kill any session referenced by an inbound cookie (login-CSRF / fixation):
    // the caller must not be able to keep a pre-login session id alive.
    if let Some(raw) = jar
        .get(crate::auth::cookie_name(state.config.cookie_secure))
        .map(|c| c.value().to_string())
    {
        let _ = db::queries::sessions::delete(&state.db, &session::hash_token(&raw)).await?;
    }

    let set_cookie = issue_session(&state, user.id).await?;
    Ok(([(SET_COOKIE, set_cookie)], Json(user)))
}

// ---------- POST /api/auth/logout ----------

async fn logout(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    // Best-effort: delete the row even if the cookie is malformed, then always
    // clear the cookie with identical attributes.
    if let Some(raw) = jar
        .get(crate::auth::cookie_name(state.config.cookie_secure))
        .map(|c| c.value().to_string())
    {
        let _ = db::queries::sessions::delete(&state.db, &session::hash_token(&raw)).await;
    }
    let cleared = clear_cookie(state.config.cookie_secure);
    (StatusCode::NO_CONTENT, [(SET_COOKIE, cleared)])
}

// ---------- POST /api/auth/logout-all ----------

async fn logout_all(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<impl IntoResponse, AppError> {
    db::queries::sessions::delete_all_for_user(&state.db, user.0.id).await?;
    let cleared = clear_cookie(state.config.cookie_secure);
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cleared)]))
}

// ---------- GET /api/auth/me ----------

async fn me(user: AuthUser) -> Json<User> {
    Json(user.0)
}

// ---------- helpers ----------

/// Mint a session for `user_id`, persist its hash, and return the `Set-Cookie`.
async fn issue_session(state: &AppState, user_id: uuid::Uuid) -> Result<String, AppError> {
    let (raw, token_hash) = session::new_token();
    let expires_at = session::new_expiry(Utc::now());
    db::queries::sessions::create(&state.db, user_id, &token_hash, expires_at).await?;
    Ok(session_cookie(&raw, state.config.cookie_secure))
}

/// Normalize + validate an email: trim, NFC, lowercase, length-bound, and a
/// conservative format check. Returns the canonical form used for storage and
/// lookups (so login matches signup regardless of casing/encoding).
fn normalize_email(raw: &str) -> Result<String, AppError> {
    let email: String = raw.trim().nfc().collect::<String>().to_lowercase();
    if email.is_empty() || email.len() > MAX_EMAIL_LEN {
        return Err(AppError::BadRequest("invalid email".into()));
    }
    if !email_address::EmailAddress::is_valid(&email) {
        return Err(AppError::BadRequest("invalid email".into()));
    }
    Ok(email)
}

/// NFC-normalize a password (no length checks — used on the login path, where we
/// must not reveal anything about the stored credential).
fn normalize_password(raw: &str) -> String {
    raw.nfc().collect()
}

/// Normalize + validate a password for signup. Length is measured in characters
/// (not bytes) after NFC; the 128 cap (plus the global body limit) defuses the
/// megabyte-password argon2 DoS vector.
fn validate_password(raw: &str) -> Result<String, AppError> {
    let password = normalize_password(raw);
    let len = password.chars().count();
    if !(MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&len) {
        return Err(AppError::BadRequest(format!(
            "password must be {MIN_PASSWORD_LEN}–{MAX_PASSWORD_LEN} characters"
        )));
    }
    Ok(password)
}
