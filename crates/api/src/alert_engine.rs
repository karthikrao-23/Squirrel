//! Alert orchestration: glue between stored data, the pure `domain::alerts`
//! rules, the `alerts` table, and email delivery.
//!
//! Everything is **per user**: the engine never touches a "default"/global
//! identity, and alert emails go to the owning user's own address — never a
//! shared inbox. `run_cycle_all_users` fans out over every user for the
//! scheduler/internal-cron path; a single user's failure can't abort the rest.

use chrono::{Duration, Utc};
use db::models::User;
use domain::alerts::{self, AlertConfig, AlertInput};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

use crate::state::AppState;

/// Wash-sale lookback used when flagging harvest candidates.
const WASH_SALE_DAYS: i64 = 30;

#[derive(Debug, Default, Serialize)]
pub struct CycleSummary {
    pub items_synced: usize,
    pub alerts_created: usize,
    pub emails_sent: usize,
}

/// Evaluate the alert rules against one user's current lots and store any new
/// alerts (dedup-aware). Returns how many were created.
pub async fn evaluate_and_store_for_user(state: &AppState, user: &User) -> anyhow::Result<usize> {
    let status = domain::FilingStatus::from_db_str(&user.filing_status);
    let as_of = Utc::now().date_naive();

    let lots = db::queries::tax_lots::list_open_with_price(&state.db, user.id).await?;
    let since = as_of - Duration::days(WASH_SALE_DAYS);
    let recent_buys: HashSet<Uuid> =
        db::queries::transactions::recent_buy_security_ids(&state.db, user.id, since)
            .await?
            .into_iter()
            .collect();

    let inputs: Vec<AlertInput> = lots
        .iter()
        .filter_map(|lot| {
            let price = lot.close_price?;
            Some(AlertInput {
                security_id: lot.security_id,
                ticker: lot.ticker.clone(),
                open_date: lot.open_date,
                quantity: lot.remaining_quantity,
                cost_basis_per_share: lot.cost_basis_per_share,
                current_price: price,
                wash_sale: recent_buys.contains(&lot.security_id),
            })
        })
        .collect();

    let config = AlertConfig {
        approaching_window_days: state.config.alert_approaching_window_days,
        min_tax_saving: state.config.alert_min_tax_saving,
    };
    let candidates = alerts::evaluate(&inputs, status, user.taxable_income, as_of, config);

    let mut created = 0;
    for candidate in &candidates {
        let payload = serde_json::to_value(candidate)?;
        let inserted = db::queries::alerts::create_if_absent(
            &state.db,
            user.id,
            candidate.kind.as_str(),
            Some(candidate.security_id),
            &candidate.title,
            &candidate.message,
            payload,
        )
        .await?;
        if inserted.is_some() {
            created += 1;
        }
    }
    tracing::info!(user = %user.id, created, evaluated = candidates.len(), "alerts evaluated");
    Ok(created)
}

/// Email a digest of a user's not-yet-emailed alerts **to that user's address**.
/// No-op (returns 0) when SMTP is not configured or nothing is pending. Each
/// alert is marked emailed on success.
pub async fn send_pending_emails_for_user(state: &AppState, user: &User) -> anyhow::Result<usize> {
    let Some(smtp) = state.config.smtp.as_ref() else {
        return Ok(0);
    };
    let pending = db::queries::alerts::list_unemailed(&state.db, user.id).await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let mut body = String::from("Squirrel — new alerts:\n\n");
    for a in &pending {
        body.push_str(&format!("• {}\n  {}\n\n", a.title, a.message));
    }
    let subject = format!("Squirrel: {} new alert(s)", pending.len());
    // Recipient is the user's own email — not a global ALERT_EMAIL_TO.
    crate::email::send(smtp, &user.email, &subject, body).await?;

    for a in &pending {
        db::queries::alerts::mark_emailed(&state.db, a.id).await?;
    }
    tracing::info!(user = %user.id, sent = pending.len(), "alert email sent");
    Ok(pending.len())
}

/// Full cycle for one user: refresh that user's prices from Plaid (best-effort),
/// evaluate alert rules, then email any pending alerts to them.
pub async fn run_cycle_for_user(state: &AppState, user: &User) -> anyhow::Result<CycleSummary> {
    let mut summary = CycleSummary::default();

    // 1. Refresh prices by re-syncing each of the user's items (Plaid permitting).
    if state.plaid.is_configured() {
        if let Some(key) = state.config.token_encryption_key {
            let items = db::queries::plaid_items::list_for_user(&state.db, user.id).await?;
            for item in &items {
                match crate::sync::sync_item(&state.db, &state.plaid, &key, item).await {
                    Ok(_) => summary.items_synced += 1,
                    Err(e) => {
                        tracing::error!(error = %e, item = %item.plaid_item_id, "scheduled sync failed")
                    }
                }
            }
        }
    }

    // 2. Evaluate + 3. email.
    summary.alerts_created = evaluate_and_store_for_user(state, user).await?;
    summary.emails_sent = send_pending_emails_for_user(state, user).await?;

    tracing::info!(user = %user.id, ?summary, "alert cycle complete");
    Ok(summary)
}

/// Run the cycle for **every** user (scheduler / internal-cron entry point). One
/// user's failure is logged and skipped so it can't stop the others. Also reaps
/// expired sessions, since the in-process scheduler is off in production and
/// this hourly cycle is the only thing that runs there.
pub async fn run_cycle_all_users(state: &AppState) -> anyhow::Result<CycleSummary> {
    let mut total = CycleSummary::default();

    let users = db::queries::users::list_all(&state.db).await?;
    for user in &users {
        match run_cycle_for_user(state, user).await {
            Ok(s) => {
                total.items_synced += s.items_synced;
                total.alerts_created += s.alerts_created;
                total.emails_sent += s.emails_sent;
            }
            Err(e) => {
                tracing::error!(user = %user.id, error = %e, "alert cycle failed for user")
            }
        }
    }

    match db::queries::sessions::delete_expired(&state.db).await {
        Ok(n) if n > 0 => tracing::info!(reaped = n, "expired sessions reaped"),
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, "expired-session reap failed"),
    }

    tracing::info!(users = users.len(), ?total, "all-user alert cycle complete");
    Ok(total)
}
