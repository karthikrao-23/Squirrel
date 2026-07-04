//! HTTP-level integration tests. Build the real router over an isolated test
//! database (`#[sqlx::test]`) and drive it with in-process requests via
//! `tower::ServiceExt::oneshot` — no network, no running server.
//!
//! Auth is enforced by the `AuthUser` extractor + CSRF guard, so every test
//! signs up a user (via [`signup`]) and attaches the returned session cookie and
//! the `X-Squirrel-CSRF` header to subsequent requests. Plaid is left
//! unconfigured (those handlers return 400, covered elsewhere).

use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn test_config() -> api::config::Config {
    api::config::Config {
        app_env: api::config::AppEnv::Development,
        database_url: String::new(),
        bind_addr: "127.0.0.1:0".into(),
        port: None,
        plaid_env: plaid::PlaidEnv::Sandbox,
        plaid_client_id: String::new(),
        plaid_secret: String::new(),
        token_encryption_key: None,
        plaid_webhook_url: None,
        plaid_redirect_uri: None,
        smtp: None,
        alert_cron: "0 0 * * * *".into(),
        alert_min_tax_saving: rust_decimal::Decimal::new(50, 0),
        alert_approaching_window_days: 30,
        // Dev posture: plain cookie name `sid`, no Secure. A known app origin so
        // the CSRF guard's foreign-Origin rejection is exercisable.
        cookie_secure: false,
        app_origin: Some("http://app.test".into()),
        scheduler_enabled: false,
        internal_api_token: None,
        static_dir: None,
        run_migrations: true,
    }
}

fn app(pool: PgPool) -> Router {
    app_with(pool, test_config())
}

fn app_with(pool: PgPool, config: api::config::Config) -> Router {
    api::build_app(api::state::AppState::new(pool, config))
}

/// A request outcome: status, parsed JSON body, `Set-Cookie` headers, and a
/// lookup over response headers (lower-cased names).
struct Resp {
    status: StatusCode,
    body: Value,
    set_cookies: Vec<String>,
    headers: std::collections::HashMap<String, String>,
}

impl Resp {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// Generic request driver. `cookie` is the raw `Cookie` header value (e.g.
/// `sid=...`); `csrf` adds the custom CSRF header; `origin` sets `Origin`.
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    csrf: bool,
    origin: Option<&str>,
    body: Option<Value>,
) -> Resp {
    request_with_headers(app, method, uri, cookie, csrf, origin, body, &[]).await
}

/// Like [`request`] but allows arbitrary extra headers (e.g. `Authorization`).
#[allow(clippy::too_many_arguments)]
async fn request_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    csrf: bool,
    origin: Option<&str>,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> Resp {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-forwarded-for", "127.0.0.1");
    if let Some(c) = cookie {
        builder = builder.header("cookie", c);
    }
    if csrf {
        builder = builder.header("x-squirrel-csrf", "1");
    }
    if let Some(o) = origin {
        builder = builder.header("origin", o);
    }
    for (k, v) in extra {
        builder = builder.header(*k, *v);
    }
    let req = if let Some(b) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let set_cookies = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    Resp {
        status,
        body,
        set_cookies,
        headers,
    }
}

/// Authenticated GET.
async fn get_auth(app: &Router, uri: &str, cookie: &str) -> Resp {
    request(app, "GET", uri, Some(cookie), false, None, None).await
}

/// Authenticated, CSRF-bearing JSON mutation.
async fn send_auth(app: &Router, method: &str, uri: &str, cookie: &str, body: Value) -> Resp {
    request(app, method, uri, Some(cookie), true, None, Some(body)).await
}

/// Pull the `sid=...` pair out of a `Set-Cookie` header for replaying as a
/// `Cookie` header.
fn sid_from(set_cookies: &[String]) -> String {
    let raw = set_cookies
        .iter()
        .find(|c| c.starts_with("sid="))
        .expect("a session cookie was set");
    raw.split(';').next().unwrap().to_string()
}

/// Sign up a fresh user; returns (cookie header value, user JSON).
async fn signup(app: &Router, email: &str, password: &str) -> (String, Value) {
    let resp = request(
        app,
        "POST",
        "/api/auth/signup",
        None,
        true,
        None,
        Some(json!({ "email": email, "password": password })),
    )
    .await;
    assert_eq!(
        resp.status,
        StatusCode::OK,
        "signup failed: {:?}",
        resp.body
    );
    (sid_from(&resp.set_cookies), resp.body)
}

/// Convenience: sign up and return (cookie, user_id).
async fn auth(app: &Router, email: &str) -> (String, Uuid) {
    let (cookie, user) = signup(app, email, "correct horse battery").await;
    let id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    (cookie, id)
}

fn num(v: &Value) -> f64 {
    match v {
        Value::String(s) => s.parse().unwrap(),
        Value::Number(n) => n.as_f64().unwrap(),
        other => panic!("expected number, got {other:?}"),
    }
}

// --- Seeding helpers (operate directly on the pool, for a given user) --------

/// Seed one priced open lot for `user_id`. Returns the lot id.
async fn seed_lot(
    pool: &PgPool,
    user_id: Uuid,
    ticker: &str,
    price: rust_decimal::Decimal,
    basis: rust_decimal::Decimal,
    qty: rust_decimal::Decimal,
) -> Uuid {
    let item = db::queries::plaid_items::upsert(pool, user_id, "item_seed", b"enc", None)
        .await
        .unwrap();
    let acct = db::queries::accounts::upsert(
        pool,
        user_id,
        item.id,
        "acct_seed",
        "Brokerage",
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let sec = db::queries::securities::upsert(
        pool,
        &format!("sec_{ticker}"),
        Some(ticker),
        None,
        None,
        None,
        Some(price),
        None,
        None,
    )
    .await
    .unwrap();
    db::queries::tax_lots::replace_for_user(
        pool,
        user_id,
        &[db::queries::tax_lots::NewLot {
            account_id: acct,
            security_id: sec,
            open_date: chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            original_quantity: qty,
            remaining_quantity: qty,
            cost_basis_per_share: basis,
            source_transaction_id: None,
        }],
    )
    .await
    .unwrap();
    let lots = db::queries::tax_lots::list_with_security(pool, user_id)
        .await
        .unwrap();
    lots[0].id
}

// ============================ Public routes ==================================

#[sqlx::test(migrations = "../../migrations")]
async fn health_ok(pool: PgPool) {
    let app = app(pool);
    let resp = request(&app, "GET", "/health", None, false, None, None).await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["status"], "ok");
    assert_eq!(resp.body["db"], "up");
}

// ============================ Auth flow ======================================

#[sqlx::test(migrations = "../../migrations")]
async fn signup_creates_user_session_and_cookie(pool: PgPool) {
    let app = app(pool.clone());
    let resp = request(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        true,
        None,
        Some(json!({ "email": "Alice@Example.com ", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    // Email is normalized (trim + lowercase).
    assert_eq!(resp.body["email"], "alice@example.com");
    // Hash never leaks into the response.
    assert!(resp.body.get("password_hash").is_none());
    // A session cookie was issued, HttpOnly + Strict.
    let sc = &resp.set_cookies[0];
    assert!(sc.starts_with("sid="));
    assert!(sc.contains("HttpOnly"));
    assert!(sc.contains("SameSite=Strict"));
    // And a session row exists.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn signup_ignores_extra_fields_and_validates_password(pool: PgPool) {
    let app = app(pool);
    // Extra fields (filing_status, id, taxable_income) are ignored, not honored.
    let resp = request(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        true,
        None,
        Some(json!({
            "email": "eve@example.com",
            "password": "correct horse battery",
            "filing_status": "married_filing_jointly",
            "taxable_income": "999999",
            "id": "00000000-0000-0000-0000-000000000000"
        })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["filing_status"], "single");
    assert_eq!(num(&resp.body["taxable_income"]), 0.0);
    assert_ne!(resp.body["id"], "00000000-0000-0000-0000-000000000000");

    // Too-short password is rejected.
    let resp = request(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        true,
        None,
        Some(json!({ "email": "short@example.com", "password": "short" })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn login_sets_cookie_and_me_returns_user(pool: PgPool) {
    let app = app(pool);
    signup(&app, "bob@example.com", "correct horse battery").await;

    let resp = request(
        &app,
        "POST",
        "/api/auth/login",
        None,
        true,
        None,
        Some(json!({ "email": "bob@example.com", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    let cookie = sid_from(&resp.set_cookies);

    let me = get_auth(&app, "/api/auth/me", &cookie).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["email"], "bob@example.com");
    assert!(me.body.get("password_hash").is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn bad_creds_and_unknown_user_are_identical_401(pool: PgPool) {
    let app = app(pool);
    signup(&app, "real@example.com", "correct horse battery").await;

    // Wrong password for a real user.
    let start = Instant::now();
    let wrong = request(
        &app,
        "POST",
        "/api/auth/login",
        None,
        true,
        None,
        Some(json!({ "email": "real@example.com", "password": "wrong password here" })),
    )
    .await;
    let wrong_dur = start.elapsed();

    // Unknown user entirely.
    let start = Instant::now();
    let unknown = request(
        &app,
        "POST",
        "/api/auth/login",
        None,
        true,
        None,
        Some(json!({ "email": "ghost@example.com", "password": "wrong password here" })),
    )
    .await;
    let unknown_dur = start.elapsed();

    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    // Identical generic body — no enumeration signal.
    assert_eq!(wrong.body, unknown.body);
    assert_eq!(wrong.body["error"], "unauthorized");
    // Coarse timing parity: the unknown-user path still ran an argon2 verify
    // (against the dummy hash), so it isn't ~instant relative to the real verify.
    assert!(
        unknown_dur.as_secs_f64() > wrong_dur.as_secs_f64() * 0.25,
        "unknown-user login ({unknown_dur:?}) should run a verify like wrong-password ({wrong_dur:?})"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_signup_conflicts(pool: PgPool) {
    let app = app(pool);
    signup(&app, "dup@example.com", "correct horse battery").await;
    let resp = request(
        &app,
        "POST",
        "/api/auth/signup",
        None,
        true,
        None,
        Some(json!({ "email": "dup@example.com", "password": "another good password" })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_hash_user_cannot_authenticate(pool: PgPool) {
    // A user with no password hash (e.g. legacy) can never log in — not even
    // with an empty password.
    let user = db::queries::users::create(&pool, "nopass@example.com", "x")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET password_hash = NULL WHERE id = $1")
        .bind(user.id)
        .execute(&pool)
        .await
        .unwrap();

    let app = app(pool);
    for pw in ["", "x", "correct horse battery"] {
        let resp = request(
            &app,
            "POST",
            "/api/auth/login",
            None,
            true,
            None,
            Some(json!({ "email": "nopass@example.com", "password": pw })),
        )
        .await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED, "pw={pw:?}");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn logout_deletes_session(pool: PgPool) {
    let app = app(pool.clone());
    let (cookie, _) = auth(&app, "out@example.com").await;

    let resp = request(
        &app,
        "POST",
        "/api/auth/logout",
        Some(&cookie),
        true,
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, StatusCode::NO_CONTENT);

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    // The cookie no longer authenticates.
    let me = get_auth(&app, "/api/auth/me", &cookie).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn logout_all_clears_every_session(pool: PgPool) {
    let app = app(pool.clone());
    let (cookie1, _) = auth(&app, "many@example.com").await;
    // A second login → a second session for the same user.
    let login = request(
        &app,
        "POST",
        "/api/auth/login",
        None,
        true,
        None,
        Some(json!({ "email": "many@example.com", "password": "correct horse battery" })),
    )
    .await;
    let _cookie2 = sid_from(&login.set_cookies);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    let resp = request(
        &app,
        "POST",
        "/api/auth/logout-all",
        Some(&cookie1),
        true,
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, StatusCode::NO_CONTENT);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn login_invalidates_inbound_session(pool: PgPool) {
    // Fixation/login-CSRF: a session referenced by an inbound cookie is killed on
    // login, so a pre-login session id can't survive authentication.
    let app = app(pool.clone());
    let (stale, _) = auth(&app, "fix@example.com").await;
    let stale_hash: Vec<u8> = stale_token_hash(&stale);

    let login = request(
        &app,
        "POST",
        "/api/auth/login",
        Some(&stale),
        true,
        None,
        Some(json!({ "email": "fix@example.com", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);

    // The old session row is gone.
    let exists: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE token_hash = $1")
        .bind(&stale_hash)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(exists, 0);
}

/// SHA-256 of the raw token inside a `sid=...` cookie value.
fn stale_token_hash(cookie: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let raw = cookie.strip_prefix("sid=").unwrap();
    Sha256::digest(raw.as_bytes()).to_vec()
}

// ============================ Route protection / CSRF ========================

#[sqlx::test(migrations = "../../migrations")]
async fn protected_route_without_cookie_is_401(pool: PgPool) {
    let app = app(pool);
    let resp = request(&app, "GET", "/api/profile", None, false, None, None).await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn mutation_without_csrf_header_is_403(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "csrf@example.com").await;
    // Valid cookie, but no CSRF header → blocked before the handler.
    let resp = request(
        &app,
        "PATCH",
        "/api/profile",
        Some(&cookie),
        false,
        None,
        Some(json!({ "filing_status": "single" })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn mutation_with_foreign_origin_is_403(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "origin@example.com").await;
    let resp = request(
        &app,
        "PATCH",
        "/api/profile",
        Some(&cookie),
        true,
        Some("http://evil.test"),
        Some(json!({ "filing_status": "single" })),
    )
    .await;
    assert_eq!(resp.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn mutation_with_prefix_lookalike_referer_is_403(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "referer@example.com").await;
    // No Origin, and a Referer whose host only *prefixes* ours
    // (`http://app.test` vs `http://app.test.evil.test`). A naive `starts_with`
    // would accept this; the origin-boundary check must reject it.
    let resp = request_with_headers(
        &app,
        "PATCH",
        "/api/profile",
        Some(&cookie),
        true,
        None,
        Some(json!({ "filing_status": "single" })),
        &[("referer", "http://app.test.evil.test/dashboard")],
    )
    .await;
    assert_eq!(resp.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn mutation_with_same_origin_referer_is_allowed(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "referer-ok@example.com").await;
    // A genuine same-origin Referer (origin followed by a path) passes the guard.
    let resp = request_with_headers(
        &app,
        "PATCH",
        "/api/profile",
        Some(&cookie),
        true,
        None,
        Some(json!({ "filing_status": "single" })),
        &[("referer", "http://app.test/settings")],
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
}

// ============================ Profile / portfolio / tax ======================

#[sqlx::test(migrations = "../../migrations")]
async fn profile_defaults_then_updates(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "profile@example.com").await;

    let resp = get_auth(&app, "/api/profile", &cookie).await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["filing_status"], "single");

    let resp = send_auth(
        &app,
        "PATCH",
        "/api/profile",
        &cookie,
        json!({ "filing_status": "married_filing_jointly", "taxable_income": "250000" }),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["filing_status"], "married_filing_jointly");
    assert_eq!(num(&resp.body["taxable_income"]), 250000.0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn profile_patch_rejects_invalid_status(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "badprofile@example.com").await;
    let resp = send_auth(
        &app,
        "PATCH",
        "/api/profile",
        &cookie,
        json!({ "filing_status": "nonsense" }),
    )
    .await;
    assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    assert!(resp.body["error"]
        .as_str()
        .unwrap()
        .contains("invalid filing_status"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn portfolio_lists_start_empty(pool: PgPool) {
    let app = app(pool);
    let (cookie, _) = auth(&app, "empty@example.com").await;
    for (uri, key) in [
        ("/api/accounts", "accounts"),
        ("/api/holdings", "holdings"),
        ("/api/lots", "lots"),
    ] {
        let resp = get_auth(&app, uri, &cookie).await;
        assert_eq!(resp.status, StatusCode::OK, "{uri}");
        assert_eq!(resp.body[key].as_array().unwrap().len(), 0, "{uri} empty");
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn tax_summary_reports_long_term_gain(pool: PgPool) {
    let app = app(pool.clone());
    let (cookie, user_id) = auth(&app, "tax@example.com").await;
    db::queries::users::update_profile(&pool, user_id, "single", dec!(100000))
        .await
        .unwrap();
    // One long-term lot of 10 @ $5 basis, price $190.
    seed_lot(&pool, user_id, "AAPL", dec!(190), dec!(5), dec!(10)).await;

    let resp = get_auth(&app, "/api/tax/summary", &cookie).await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["lots_valued"], 1);
    assert_eq!(num(&resp.body["unrealized_long_term"]), 1850.0);
    assert_eq!(num(&resp.body["unrealized_short_term"]), 0.0);
    assert_eq!(num(&resp.body["total_market_value"]), 1900.0);
    assert!((num(&resp.body["estimated_tax_if_sold_now"]["federal"]) - 277.5).abs() < 0.01);
}

#[sqlx::test(migrations = "../../migrations")]
async fn account_kind_override_reclassifies_and_gates_harvest(pool: PgPool) {
    let app = app(pool.clone());
    let (cookie, user_id) = auth(&app, "kind@example.com").await;
    db::queries::users::update_profile(&pool, user_id, "single", dec!(100000))
        .await
        .unwrap();
    // A long-term loss lot in an account with no Plaid subtype → derived taxable,
    // so its loss is a harvest candidate.
    seed_lot(&pool, user_id, "LOSS", dec!(50), dec!(200), dec!(100)).await;

    // Starts taxable, with no manual override.
    let accts = get_auth(&app, "/api/accounts", &cookie).await;
    let acct = accts.body["accounts"][0].clone();
    assert_eq!(acct["kind"], "taxable");
    assert!(acct["kind_override"].is_null());
    let acct_id = acct["id"].as_str().unwrap().to_string();

    // The loss lot is harvestable while the account is taxable.
    let harvest = get_auth(&app, "/api/tax/harvest", &cookie).await;
    assert_eq!(harvest.body["candidates"].as_array().unwrap().len(), 1);

    // Mark the account as retirement.
    let uri = format!("/api/accounts/{acct_id}/kind");
    let resp = send_auth(
        &app,
        "PATCH",
        &uri,
        &cookie,
        json!({ "kind": "retirement" }),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK, "{:?}", resp.body);
    assert_eq!(resp.body["kind"], "retirement");
    assert_eq!(resp.body["kind_override"], "retirement");

    // It now reads as retirement, and its lot is no longer harvestable.
    let accts = get_auth(&app, "/api/accounts", &cookie).await;
    assert_eq!(accts.body["accounts"][0]["kind"], "retirement");
    assert_eq!(accts.body["accounts"][0]["kind_override"], "retirement");
    let harvest = get_auth(&app, "/api/tax/harvest", &cookie).await;
    assert!(harvest.body["candidates"].as_array().unwrap().is_empty());

    // Clearing the override (kind: null) reverts to the derived taxable kind.
    let resp = send_auth(&app, "PATCH", &uri, &cookie, json!({ "kind": null })).await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["kind"], "taxable");
    assert!(resp.body["kind_override"].is_null());

    // An unrecognized kind is rejected.
    let resp = send_auth(&app, "PATCH", &uri, &cookie, json!({ "kind": "roth" })).await;
    assert_eq!(resp.status, StatusCode::BAD_REQUEST);

    // An account the user doesn't own is a 404, not a silent no-op.
    let foreign = format!("/api/accounts/{}/kind", Uuid::new_v4());
    let resp = send_auth(
        &app,
        "PATCH",
        &foreign,
        &cookie,
        json!({ "kind": "retirement" }),
    )
    .await;
    assert_eq!(resp.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../../migrations")]
async fn alerts_evaluate_list_and_read(pool: PgPool) {
    let app = app(pool.clone());
    let (cookie, user_id) = auth(&app, "alert@example.com").await;
    db::queries::users::update_profile(&pool, user_id, "single", dec!(100000))
        .await
        .unwrap();
    // A long-term *loss* lot → harvestable-loss alert.
    seed_lot(&pool, user_id, "LOSS", dec!(50), dec!(200), dec!(100)).await;

    let resp = send_auth(&app, "POST", "/api/alerts/evaluate", &cookie, json!({})).await;
    assert_eq!(resp.status, StatusCode::OK);
    assert!(num(&resp.body["created"]) >= 1.0);
    assert_eq!(num(&resp.body["emailed"]), 0.0);

    let resp = get_auth(&app, "/api/alerts", &cookie).await;
    assert_eq!(resp.status, StatusCode::OK);
    let alerts = resp.body["alerts"].as_array().unwrap();
    assert!(!alerts.is_empty());
    assert_eq!(alerts[0]["type"], "harvestable_loss");
    let id = alerts[0]["id"].as_str().unwrap().to_string();

    let resp = send_auth(
        &app,
        "POST",
        &format!("/api/alerts/{id}/read"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    let resp = get_auth(&app, "/api/alerts?unread_only=true", &cookie).await;
    assert_eq!(resp.body["alerts"].as_array().unwrap().len(), 0);
}

// ============================ Cross-tenant isolation =========================

#[sqlx::test(migrations = "../../migrations")]
async fn user_a_cannot_read_or_mutate_user_b(pool: PgPool) {
    let app = app(pool.clone());
    let (cookie_a, _a) = auth(&app, "a@example.com").await;
    let (_cookie_b, b) = auth(&app, "b@example.com").await;
    db::queries::users::update_profile(&pool, b, "single", dec!(100000))
        .await
        .unwrap();

    // B has a loss lot (harvestable) and an alert; A has nothing.
    let b_lot = seed_lot(&pool, b, "BONLY", dec!(50), dec!(200), dec!(100)).await;
    let b_alert = db::queries::alerts::create_if_absent(
        &pool,
        b,
        "harvestable_loss",
        None,
        "B alert",
        "msg",
        json!({}),
    )
    .await
    .unwrap()
    .unwrap();

    // A's reads never surface B's data.
    let holdings = get_auth(&app, "/api/holdings", &cookie_a).await;
    assert_eq!(holdings.body["holdings"].as_array().unwrap().len(), 0);
    let lots = get_auth(&app, "/api/lots", &cookie_a).await;
    assert_eq!(lots.body["lots"].as_array().unwrap().len(), 0);
    let alerts = get_auth(&app, "/api/alerts", &cookie_a).await;
    assert_eq!(alerts.body["alerts"].as_array().unwrap().len(), 0);
    let summary = get_auth(&app, "/api/tax/summary", &cookie_a).await;
    assert_eq!(summary.body["lots_valued"], 0);

    // A's simulate with B's lot id → unknown lot.
    let sim = send_auth(
        &app,
        "POST",
        "/api/tax/simulate",
        &cookie_a,
        json!({ "sales": [{ "lot_id": b_lot }] }),
    )
    .await;
    assert_eq!(sim.status, StatusCode::BAD_REQUEST);
    assert!(sim.body["error"].as_str().unwrap().contains("unknown lot"));

    // A marking B's alert read → 404 (and B's alert stays unread).
    let mark = send_auth(
        &app,
        "POST",
        &format!("/api/alerts/{}/read", b_alert.id),
        &cookie_a,
        json!({}),
    )
    .await;
    assert_eq!(mark.status, StatusCode::NOT_FOUND);
    let still_unread: i64 =
        sqlx::query_scalar("SELECT count(*) FROM alerts WHERE id = $1 AND read_at IS NULL")
            .bind(b_alert.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(still_unread, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn same_institution_two_users_keep_separate_rows(pool: PgPool) {
    // The composite-unique fix: two users connecting the *same* institution mint
    // identical Plaid ids (guaranteed in sandbox). With global uniques, B's sync
    // would overwrite A's row; with (user_id, plaid_*_id) they coexist.
    let app = app(pool.clone());
    let (cookie_a, a) = auth(&app, "insta@example.com").await;
    let (cookie_b, b) = auth(&app, "instb@example.com").await;

    // Same Plaid ids for both users.
    for uid in [a, b] {
        let item =
            db::queries::plaid_items::upsert(&pool, uid, "item_shared", b"enc", Some("ins_1"))
                .await
                .unwrap();
        let acct = db::queries::accounts::upsert(
            &pool,
            uid,
            item.id,
            "acct_shared",
            "Shared Brokerage",
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let sec = db::queries::securities::upsert(
            &pool,
            "sec_shared",
            Some("SHRD"),
            None,
            None,
            None,
            Some(dec!(10)),
            None,
            None,
        )
        .await
        .unwrap();
        let _ = sec;
        let _ = acct;
        db::queries::transactions::insert_ignore(
            &pool,
            &db::queries::transactions::NewTransaction {
                user_id: uid,
                account_id: acct,
                security_id: Some(sec),
                plaid_investment_transaction_id: "tx_shared",
                transaction_type: Some("buy"),
                subtype: None,
                quantity: Some(dec!(1)),
                price: Some(dec!(5)),
                amount: Some(dec!(5)),
                fees: None,
                date: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                name: None,
                currency: Some("USD"),
            },
        )
        .await
        .unwrap();
    }

    // Two distinct rows per table — one per user — despite shared Plaid ids.
    for (table, plaid_col, plaid_val) in [
        ("plaid_items", "plaid_item_id", "item_shared"),
        ("accounts", "plaid_account_id", "acct_shared"),
        (
            "transactions",
            "plaid_investment_transaction_id",
            "tx_shared",
        ),
    ] {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE {plaid_col} = $1"
        ))
        .bind(plaid_val)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 2, "{table} should hold one row per user");
    }

    // Each user sees exactly their own account through the API.
    let a_accts = get_auth(&app, "/api/accounts", &cookie_a).await;
    let b_accts = get_auth(&app, "/api/accounts", &cookie_b).await;
    assert_eq!(a_accts.body["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(b_accts.body["accounts"].as_array().unwrap().len(), 1);
    assert_ne!(
        a_accts.body["accounts"][0]["id"], b_accts.body["accounts"][0]["id"],
        "the two users' accounts are distinct rows"
    );
}

// ============================ Prod config (Part C) ===========================

#[sqlx::test(migrations = "../../migrations")]
async fn security_headers_present(pool: PgPool) {
    // Dev config (cookie_secure=false) → no HSTS, but the other headers apply.
    let app = app(pool);
    let resp = request(&app, "GET", "/health", None, false, None, None).await;
    assert_eq!(resp.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(resp.header("referrer-policy"), Some("same-origin"));
    assert!(resp
        .header("content-security-policy")
        .unwrap()
        .contains("frame-ancestors 'none'"));
    // HSTS is HTTPS-only, so it must be absent in development.
    assert_eq!(resp.header("strict-transport-security"), None);
}

#[sqlx::test(migrations = "../../migrations")]
async fn hsts_present_in_secure_posture(pool: PgPool) {
    let mut cfg = test_config();
    cfg.cookie_secure = true; // staging/prod posture
    let app = app_with(pool, cfg);
    let resp = request(&app, "GET", "/health", None, false, None, None).await;
    assert!(resp
        .header("strict-transport-security")
        .unwrap()
        .contains("max-age="));
}

#[sqlx::test(migrations = "../../migrations")]
async fn sandbox_connect_forbidden_outside_development(pool: PgPool) {
    let mut cfg = test_config();
    cfg.app_env = api::config::AppEnv::Staging;
    let app = app_with(pool, cfg);
    let (cookie, _) = auth(&app, "sandbox@example.com").await;
    // 403 fires on the env gate, before any Plaid config check.
    let resp = request(
        &app,
        "POST",
        "/api/plaid/sandbox/connect",
        Some(&cookie),
        true,
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(resp.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../../migrations")]
async fn internal_endpoint_requires_bearer_token(pool: PgPool) {
    let mut cfg = test_config();
    cfg.internal_api_token = Some("s3cr3t-internal-token".into());
    let app = app_with(pool, cfg);

    // No Authorization header → 401.
    let resp = request(
        &app,
        "POST",
        "/api/internal/alerts/run",
        None,
        false,
        None,
        None,
    )
    .await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);

    // Wrong token → 401.
    let resp = request_with_headers(
        &app,
        "POST",
        "/api/internal/alerts/run",
        None,
        false,
        None,
        None,
        &[("authorization", "Bearer wrong-token")],
    )
    .await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);

    // Correct token → 200 with a cycle summary. Note: no CSRF header is sent,
    // proving the internal routes are exempt from the CSRF guard.
    let resp = request_with_headers(
        &app,
        "POST",
        "/api/internal/alerts/run",
        None,
        false,
        None,
        None,
        &[("authorization", "Bearer s3cr3t-internal-token")],
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    assert!(resp.body.get("alerts_created").is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn webhook_rejects_unsigned_and_is_csrf_exempt(pool: PgPool) {
    let app = app(pool);
    // No Plaid-Verification header and no CSRF header. If the webhook weren't
    // CSRF-exempt we'd get 403; instead we reach signature verification, which
    // rejects the unsigned body → 401. (A 401 here proves both: wired + exempt.)
    let resp = request(
        &app,
        "POST",
        "/api/plaid/webhook",
        None,
        false,
        None,
        Some(
            json!({ "webhook_type": "HOLDINGS", "webhook_code": "DEFAULT_UPDATE", "item_id": "x" }),
        ),
    )
    .await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);

    // A bogus verification header is likewise rejected.
    let resp = request_with_headers(
        &app,
        "POST",
        "/api/plaid/webhook",
        None,
        false,
        None,
        Some(json!({ "webhook_type": "HOLDINGS" })),
        &[("plaid-verification", "not-a-jwt")],
    )
    .await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../../migrations")]
async fn internal_endpoint_closed_when_unconfigured(pool: PgPool) {
    // No internal token configured → endpoint is closed even with a bearer.
    let app = app(pool);
    let resp = request_with_headers(
        &app,
        "POST",
        "/api/internal/alerts/run",
        None,
        false,
        None,
        None,
        &[("authorization", "Bearer anything")],
    )
    .await;
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
}

// ============================ SPA static serving (Part D) ====================

fn make_static_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "squirrel_static_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><title>Squirrel</title>",
    )
    .unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    dir
}

#[sqlx::test(migrations = "../../migrations")]
async fn serves_spa_and_does_not_swallow_api_404(pool: PgPool) {
    let dir = make_static_dir();
    let mut cfg = test_config();
    cfg.static_dir = Some(dir.to_str().unwrap().to_string());
    let app = app_with(pool, cfg);

    // Root → index.html.
    let root = request(&app, "GET", "/", None, false, None, None).await;
    assert_eq!(root.status, StatusCode::OK);
    assert!(root.header("content-type").unwrap().contains("text/html"));

    // Unknown client route → SPA fallback (index.html), not a 404.
    let spa = request(&app, "GET", "/dashboard", None, false, None, None).await;
    assert_eq!(spa.status, StatusCode::OK);
    assert!(spa.header("content-type").unwrap().contains("text/html"));

    // A real asset is served.
    let asset = request(&app, "GET", "/assets/app.js", None, false, None, None).await;
    assert_eq!(asset.status, StatusCode::OK);

    // Unknown /api/* path is a JSON 404 — NOT swallowed into 200 index.html.
    let api404 = request(&app, "GET", "/api/does-not-exist", None, false, None, None).await;
    assert_eq!(api404.status, StatusCode::NOT_FOUND);
    assert!(api404.body["error"].is_string());

    // /health still works behind the static fallback.
    let health = request(&app, "GET", "/health", None, false, None, None).await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.body["status"], "ok");

    let _ = std::fs::remove_dir_all(dir);
}

// ============================ Rate-limit key fallback =========================

#[sqlx::test(migrations = "../../migrations")]
async fn login_without_forwarded_header_does_not_500(pool: PgPool) {
    // The dev/direct case: no X-Forwarded-For at all. The auth rate limiter must
    // fall back to a shared bucket instead of erroring — so an unknown-user login
    // is a clean 401, never a 500 ("Unable To Extract Key").
    let app = app(pool);
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("content-type", "application/json")
        .header("x-squirrel-csrf", "1")
        // deliberately NO x-forwarded-for
        .body(Body::from(
            json!({ "email": "ghost@example.com", "password": "correct horse battery" })
                .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
