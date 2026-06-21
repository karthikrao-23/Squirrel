//! Plaid onboarding + webhook routes (all under `/api/plaid`).
//!
//! - `POST /api/plaid/link-token`   → token the frontend hands to Plaid Link
//! - `POST /api/plaid/exchange`     → swap a public_token, store the item, sync
//! - `POST /api/plaid/sandbox/connect` → dev shortcut: mint + exchange + sync
//!   without a frontend (sandbox only)
//! - `POST /api/plaid/webhook`      → Plaid pings us when data changes; re-sync

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::AppState;
use crate::sync::{self, SyncSummary};

/// Default sandbox institution that supports the Investments product.
const SANDBOX_INSTITUTION: &str = "ins_109508";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/plaid/link-token", post(link_token))
        .route("/api/plaid/exchange", post(exchange))
        .route("/api/plaid/sandbox/connect", post(sandbox_connect))
        .route("/api/plaid/webhook", post(webhook))
}

#[derive(Deserialize)]
struct ExchangeReq {
    public_token: String,
}

#[derive(Deserialize, Default)]
struct SandboxConnectReq {
    /// Override the sandbox institution; defaults to one with Investments.
    institution_id: Option<String>,
}

#[derive(Serialize)]
struct ConnectResponse {
    item_id: String,
    summary: SyncSummary,
}

/// `POST /api/plaid/link-token`
async fn link_token(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    require_plaid(&state)?;
    let user = db::queries::users::ensure_default(&state.db).await?;
    let resp = state
        .plaid
        .create_link_token(&user.id.to_string(), state.config.plaid_webhook_url.as_deref())
        .await?;
    Ok(Json(json!({
        "link_token": resp.link_token,
        "expiration": resp.expiration,
    })))
}

/// `POST /api/plaid/exchange`
async fn exchange(
    State(state): State<AppState>,
    Json(body): Json<ExchangeReq>,
) -> Result<Json<ConnectResponse>, AppError> {
    let resp = connect_with_public_token(&state, &body.public_token).await?;
    Ok(Json(resp))
}

/// `POST /api/plaid/sandbox/connect` — end-to-end test path without a frontend.
async fn sandbox_connect(
    State(state): State<AppState>,
    body: Option<Json<SandboxConnectReq>>,
) -> Result<Json<ConnectResponse>, AppError> {
    require_plaid(&state)?;
    let institution = body
        .and_then(|b| b.0.institution_id)
        .unwrap_or_else(|| SANDBOX_INSTITUTION.to_string());
    let minted = state.plaid.sandbox_public_token_create(&institution).await?;
    let resp = connect_with_public_token(&state, &minted.public_token).await?;
    Ok(Json(resp))
}

/// `POST /api/plaid/webhook` — always answer 200 (Plaid retries otherwise);
/// errors are logged, not surfaced. Signature verification is deferred to M8.
async fn webhook(State(state): State<AppState>, Json(hook): Json<plaid::webhooks::PlaidWebhook>) -> StatusCode {
    tracing::info!(
        webhook_type = %hook.webhook_type,
        webhook_code = %hook.webhook_code,
        item = %hook.item_id,
        "plaid webhook received"
    );

    if hook.is_investments_update() {
        if let Err(e) = resync_item(&state, &hook.item_id).await {
            tracing::error!(error = %e, item = %hook.item_id, "webhook re-sync failed");
        }
    }
    StatusCode::OK
}

// --- helpers ---

/// Shared path for exchange + sandbox: exchange token → store encrypted item →
/// initial sync.
async fn connect_with_public_token(
    state: &AppState,
    public_token: &str,
) -> Result<ConnectResponse, AppError> {
    require_plaid(state)?;
    let key = require_key(state)?;

    let exchanged = state.plaid.exchange_public_token(public_token).await?;
    let encrypted = crate::crypto::encrypt(&key, exchanged.access_token.as_bytes())?;
    let user = db::queries::users::ensure_default(&state.db).await?;
    let item =
        db::queries::plaid_items::upsert(&state.db, user.id, &exchanged.item_id, &encrypted, None)
            .await?;

    let summary = sync::sync_item(&state.db, &state.plaid, &key, &item).await?;
    Ok(ConnectResponse {
        item_id: exchanged.item_id,
        summary,
    })
}

/// Re-sync an existing item by Plaid item id (webhook path).
async fn resync_item(state: &AppState, plaid_item_id: &str) -> anyhow::Result<()> {
    let key = state
        .config
        .token_encryption_key
        .ok_or_else(|| anyhow::anyhow!("TOKEN_ENCRYPTION_KEY not configured"))?;
    let Some(item) = db::queries::plaid_items::find_by_plaid_item_id(&state.db, plaid_item_id).await?
    else {
        tracing::warn!(item = %plaid_item_id, "webhook for unknown item; ignoring");
        return Ok(());
    };
    sync::sync_item(&state.db, &state.plaid, &key, &item).await?;
    Ok(())
}

fn require_plaid(state: &AppState) -> Result<(), AppError> {
    if state.plaid.is_configured() {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "Plaid is not configured (set PLAID_CLIENT_ID and PLAID_SECRET)".into(),
        ))
    }
}

fn require_key(state: &AppState) -> Result<[u8; 32], AppError> {
    state.config.token_encryption_key.ok_or_else(|| {
        AppError::BadRequest(
            "TOKEN_ENCRYPTION_KEY not configured (generate: openssl rand -base64 32)".into(),
        )
    })
}
