//! Plaid onboarding + webhook routes (all under `/api/plaid`).
//!
//! - `POST /api/plaid/link-token`   → token the frontend hands to Plaid Link
//! - `POST /api/plaid/exchange`     → swap a public_token, store the item, sync
//! - `POST /api/plaid/sandbox/connect` → dev shortcut: mint + exchange + sync
//!   without a frontend (sandbox only)
//! - `POST /api/plaid/webhook`      → Plaid pings us when data changes; re-sync

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::sync::{self, SyncSummary};
use uuid::Uuid;

/// Default sandbox institution that supports the Investments product.
const SANDBOX_INSTITUTION: &str = "ins_109508";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/plaid/link-token", post(link_token))
        .route("/api/plaid/exchange", post(exchange))
        .route("/api/plaid/resync", post(resync))
        .route("/api/plaid/items", get(list_items))
        .route("/api/plaid/items/{id}", delete(remove_connection))
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
async fn link_token(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    require_plaid(&state)?;
    let resp = state
        .plaid
        .create_link_token(
            &user.0.id.to_string(),
            state.config.plaid_webhook_url.as_deref(),
        )
        .await?;
    Ok(Json(json!({
        "link_token": resp.link_token,
        "expiration": resp.expiration,
    })))
}

/// `POST /api/plaid/exchange`
async fn exchange(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ExchangeReq>,
) -> Result<Json<ConnectResponse>, AppError> {
    let resp = connect_with_public_token(&state, user.0.id, &body.public_token).await?;
    Ok(Json(resp))
}

/// `POST /api/plaid/resync` — re-pull holdings + transactions for all of the
/// user's connected items. Lets data refresh (and code/data fixes) reach an
/// existing connection without reconnecting; `sync_item` rebuilds tax lots too.
async fn resync(State(state): State<AppState>, user: AuthUser) -> Result<Json<Value>, AppError> {
    require_plaid(&state)?;
    let key = require_key(&state)?;
    let items = db::queries::plaid_items::list_for_user(&state.db, user.0.id).await?;
    let mut accounts = 0;
    let mut holdings = 0;
    let mut transactions = 0;
    for item in &items {
        let s = sync::sync_item(&state.db, &state.plaid, &key, item).await?;
        accounts += s.accounts;
        holdings += s.holdings;
        transactions += s.transactions_inserted;
    }
    Ok(Json(json!({
        "items": items.len(),
        "accounts": accounts,
        "holdings": holdings,
        "transactions_inserted": transactions,
    })))
}

/// `GET /api/plaid/items` — the user's connections (one per Plaid Link session),
/// each with the accounts it brought in. Powers the "Connections" manager so a
/// user can see (and remove) a duplicate connection.
async fn list_items(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = db::queries::plaid_items::list_for_user(&state.db, user.0.id).await?;
    let accounts = db::queries::accounts::list(&state.db, user.0.id).await?;

    let connections: Vec<Value> = items
        .iter()
        .map(|it| {
            let accts: Vec<Value> = accounts
                .iter()
                .filter(|a| a.plaid_item_id == it.id)
                .map(|a| {
                    let kind =
                        domain::accounts::AccountKind::from_subtype(a.subtype.as_deref()).as_str();
                    json!({ "id": a.id, "name": a.name, "subtype": a.subtype, "kind": kind })
                })
                .collect();
            json!({
                "id": it.id,
                "institution_name": it.institution_name,
                "institution_id": it.institution_id,
                "status": it.status,
                "created_at": it.created_at,
                "accounts": accts,
            })
        })
        .collect();
    Ok(Json(json!({ "connections": connections })))
}

/// `DELETE /api/plaid/items/{id}` — remove a connection: disconnect it on Plaid's
/// side (best-effort) and delete the local item, which cascades to its accounts,
/// holdings, transactions, and tax lots. Removing at the connection level (not a
/// single account) is deliberate: the item's next sync would just re-create any
/// account we deleted while the connection stays live.
async fn remove_connection(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let item = db::queries::plaid_items::find_by_id(&state.db, user.0.id, id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Disconnect on Plaid's side so the token is invalidated and no more
    // webhooks/billing accrue. A failure here must not block local removal — the
    // user asked for it gone, so we log and proceed to delete our rows.
    if state.plaid.is_configured() {
        if let Some(key) = state.config.token_encryption_key {
            match crate::crypto::decrypt(&key, &item.access_token_encrypted)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
            {
                Some(token) => {
                    if let Err(e) = state.plaid.remove_item(&token).await {
                        tracing::warn!(error = %e, item = %item.id, "plaid item/remove failed; removing locally anyway");
                    }
                }
                None => {
                    tracing::warn!(item = %item.id, "could not decrypt access token; removing locally anyway");
                }
            }
        }
    }

    let removed = db::queries::plaid_items::delete(&state.db, user.0.id, id).await? > 0;
    tracing::info!(user = %user.0.id, item = %id, removed, "connection removed");
    Ok(Json(json!({ "removed": removed })))
}

/// `POST /api/plaid/sandbox/connect` — end-to-end test path without a frontend.
async fn sandbox_connect(
    State(state): State<AppState>,
    user: AuthUser,
    body: Option<Json<SandboxConnectReq>>,
) -> Result<Json<ConnectResponse>, AppError> {
    // The sandbox shortcut mints fake tokens; it must never be reachable outside
    // local development (posture is driven by APP_ENV, not PLAID_ENV).
    if !state.config.app_env.is_development() {
        return Err(AppError::Forbidden(
            "sandbox connect is only available in development".into(),
        ));
    }
    require_plaid(&state)?;
    let institution = body
        .and_then(|b| b.0.institution_id)
        .unwrap_or_else(|| SANDBOX_INSTITUTION.to_string());
    let minted = state
        .plaid
        .sandbox_public_token_create(&institution)
        .await?;
    let resp = connect_with_public_token(&state, user.0.id, &minted.public_token).await?;
    Ok(Json(resp))
}

/// `POST /api/plaid/webhook` — the only public mutating route, so it's verified
/// by Plaid's `Plaid-Verification` JWT signature (ES256) over the raw body. Any
/// verification failure → 401 and nothing is acted on. Takes raw `Bytes` because
/// the signature covers the exact byte string (parsing first would lose it).
async fn webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let verification = headers
        .get("plaid-verification")
        .and_then(|v| v.to_str().ok());

    let hook = match state
        .webhook_verifier
        .verify_and_parse(&state.plaid, verification, &body)
        .await
    {
        Ok(hook) => hook,
        Err(e) => {
            // Don't leak which check failed; log server-side, return a flat 401.
            tracing::warn!(error = %e, "plaid webhook verification failed");
            return StatusCode::UNAUTHORIZED;
        }
    };

    tracing::info!(
        webhook_type = %hook.webhook_type,
        webhook_code = %hook.webhook_code,
        item = %hook.item_id,
        "plaid webhook verified"
    );

    if hook.is_investments_update() {
        // Dedupe: collapse a burst of webhooks for the same item into one sync.
        if state.try_claim_sync(&hook.item_id) {
            let result = resync_item(&state, &hook.item_id).await;
            state.release_sync(&hook.item_id);
            if let Err(e) = result {
                tracing::error!(error = %e, item = %hook.item_id, "webhook re-sync failed");
            }
        } else {
            tracing::info!(item = %hook.item_id, "sync already in flight; skipping duplicate webhook");
        }
    }
    StatusCode::OK
}

// --- helpers ---

/// Shared path for exchange + sandbox: exchange token → store encrypted item →
/// initial sync.
async fn connect_with_public_token(
    state: &AppState,
    user_id: Uuid,
    public_token: &str,
) -> Result<ConnectResponse, AppError> {
    require_plaid(state)?;
    let key = require_key(state)?;

    let exchanged = state.plaid.exchange_public_token(public_token).await?;
    let encrypted = crate::crypto::encrypt(&key, exchanged.access_token.as_bytes())?;
    let item =
        db::queries::plaid_items::upsert(&state.db, user_id, &exchanged.item_id, &encrypted, None)
            .await?;

    let summary = sync::sync_item(&state.db, &state.plaid, &key, &item).await?;
    Ok(ConnectResponse {
        item_id: exchanged.item_id,
        summary,
    })
}

/// Re-sync every item with this Plaid item id (webhook path). Because the id is
/// unique only per user now, the same id can map to one item per user (sandbox);
/// we re-sync each so one user's webhook never touches another user's data.
async fn resync_item(state: &AppState, plaid_item_id: &str) -> anyhow::Result<()> {
    let key = state
        .config
        .token_encryption_key
        .ok_or_else(|| anyhow::anyhow!("TOKEN_ENCRYPTION_KEY not configured"))?;
    let items =
        db::queries::plaid_items::find_all_by_plaid_item_id(&state.db, plaid_item_id).await?;
    if items.is_empty() {
        tracing::warn!(item = %plaid_item_id, "webhook for unknown item; ignoring");
        return Ok(());
    }
    for item in &items {
        sync::sync_item(&state.db, &state.plaid, &key, item).await?;
    }
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
