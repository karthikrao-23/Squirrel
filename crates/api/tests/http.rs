//! HTTP-level integration tests. Build the real router over an isolated test
//! database (`#[sqlx::test]`) and drive it with in-process requests via
//! `tower::ServiceExt::oneshot` — no network, no running server. Plaid is left
//! unconfigured (those handlers return 400, covered elsewhere).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

fn test_config() -> api::config::Config {
    api::config::Config {
        database_url: String::new(),
        bind_addr: "127.0.0.1:0".into(),
        plaid_env: plaid::PlaidEnv::Sandbox,
        plaid_client_id: String::new(),
        plaid_secret: String::new(),
        token_encryption_key: None,
        plaid_webhook_url: None,
    }
}

fn app(pool: PgPool) -> Router {
    api::build_app(api::state::AppState::new(pool, test_config()))
}

async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    body_json(resp).await
}

async fn send_json(app: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    body_json(resp).await
}

async fn body_json(resp: axum::response::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

/// Decimals serialize as JSON strings; parse to f64 for tolerant numeric asserts.
fn num(v: &Value) -> f64 {
    match v {
        Value::String(s) => s.parse().unwrap(),
        Value::Number(n) => n.as_f64().unwrap(),
        other => panic!("expected number, got {other:?}"),
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn health_ok(pool: PgPool) {
    let app = app(pool);
    let (status, body) = get(&app, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["db"], "up");
}

#[sqlx::test(migrations = "../../migrations")]
async fn profile_defaults_then_updates(pool: PgPool) {
    let app = app(pool);

    let (status, body) = get(&app, "/api/profile").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["filing_status"], "single");

    let (status, body) = send_json(
        &app,
        "PATCH",
        "/api/profile",
        json!({"filing_status": "married_filing_jointly", "taxable_income": "250000"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["filing_status"], "married_filing_jointly");
    assert_eq!(num(&body["taxable_income"]), 250000.0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn profile_patch_rejects_invalid_status(pool: PgPool) {
    let app = app(pool);
    let (status, body) = send_json(
        &app,
        "PATCH",
        "/api/profile",
        json!({"filing_status": "nonsense"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("invalid filing_status"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn portfolio_lists_start_empty(pool: PgPool) {
    let app = app(pool);
    for (uri, key) in [
        ("/api/accounts", "accounts"),
        ("/api/holdings", "holdings"),
        ("/api/lots", "lots"),
    ] {
        let (status, body) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(
            body[key].as_array().unwrap().len(),
            0,
            "{uri} should be empty"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn tax_summary_reports_long_term_gain(pool: PgPool) {
    // Seed: single filer @ $100k, one long-term lot of 10 @ $5 basis, price $190.
    let user = db::queries::users::ensure_default(&pool).await.unwrap();
    db::queries::users::update_profile(&pool, user.id, "single", dec!(100000))
        .await
        .unwrap();
    let item = db::queries::plaid_items::upsert(&pool, user.id, "item_1", b"enc", None)
        .await
        .unwrap();
    let acct = db::queries::accounts::upsert(
        &pool,
        user.id,
        item.id,
        "acct_1",
        "Brokerage",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let sec = db::queries::securities::upsert(
        &pool,
        "sec_1",
        Some("AAPL"),
        None,
        None,
        None,
        Some(dec!(190)),
        None,
        None,
    )
    .await
    .unwrap();
    db::queries::tax_lots::replace_for_user(
        &pool,
        user.id,
        &[db::queries::tax_lots::NewLot {
            account_id: acct,
            security_id: sec,
            open_date: chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            original_quantity: dec!(10),
            remaining_quantity: dec!(10),
            cost_basis_per_share: dec!(5),
            source_transaction_id: None,
        }],
    )
    .await
    .unwrap();

    let app = app(pool);
    let (status, body) = get(&app, "/api/tax/summary").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["lots_valued"], 1);
    // gain = (190 - 5) * 10 = 1850, all long-term
    assert_eq!(num(&body["unrealized_long_term"]), 1850.0);
    assert_eq!(num(&body["unrealized_short_term"]), 0.0);
    assert_eq!(num(&body["total_market_value"]), 1900.0);
    // Federal LT @ 15% (gain sits in the 15% band above $100k base) = 277.5
    assert!((num(&body["estimated_tax_if_sold_now"]["federal"]) - 277.5).abs() < 0.01);
}
