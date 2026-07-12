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
use rust_decimal::Decimal;
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

    let mut tx = db::begin_as_user(&state.db, user.id).await?;
    let lots = db::queries::tax_lots::list_open_with_price(&mut tx, user.id).await?;
    let since = as_of - Duration::days(WASH_SALE_DAYS);
    let recent_buys: HashSet<Uuid> =
        db::queries::transactions::recent_buy_security_ids(&mut tx, user.id, since)
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
        // Upsert: create a new alert, or refresh the standing one in place so its
        // figures + timestamp stay current instead of freezing at first detection.
        let (_, is_new) = db::queries::alerts::upsert_active(
            &mut tx,
            user.id,
            candidate.kind.as_str(),
            Some(candidate.security_id),
            &candidate.title,
            &candidate.message,
            payload,
        )
        .await?;
        if is_new {
            created += 1;
        }
    }

    // Missed harvest opportunities: an unread harvestable-loss alert whose
    // security is no longer a current loss candidate means the window closed
    // without the user acting. Retype it to `missed_harvest`, keeping the
    // last-known saving and recording the window it was open.
    let active_loss: HashSet<Uuid> = candidates
        .iter()
        .filter(|c| c.kind.as_str() == "harvestable_loss")
        .map(|c| c.security_id)
        .collect();
    let mut missed = 0;
    for alert in
        db::queries::alerts::list_unread_by_type(&mut tx, user.id, "harvestable_loss").await?
    {
        let Some(sid) = alert.security_id else {
            continue;
        };
        if active_loss.contains(&sid) {
            continue; // still an active opportunity — leave it (already refreshed above)
        }
        let label = alert
            .payload
            .get("ticker")
            .and_then(|v| v.as_str())
            .unwrap_or("this security");
        let saving = alert
            .payload
            .get("estimated_tax_saving")
            .and_then(|v| v.as_str());
        let available_since = alert.created_at.date_naive();
        let message = match saving {
            Some(s) => format!(
                "Missed tax-loss harvesting on {label}: ~${s} of estimated tax savings was \
                 available (open {available_since} – {as_of}) but the opportunity has since closed."
            ),
            None => format!(
                "Missed tax-loss harvesting on {label}: the opportunity was available \
                 (open {available_since} – {as_of}) but has since closed."
            ),
        };
        let mut payload = alert.payload.clone();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("missed_on".into(), serde_json::json!(as_of));
            obj.insert("available_since".into(), serde_json::json!(available_since));
        }
        db::queries::alerts::retype(&mut tx, alert.id, "missed_harvest", &message, payload).await?;
        missed += 1;
    }

    tx.commit().await?;
    tracing::info!(user = %user.id, created, missed, evaluated = candidates.len(), "alerts evaluated");
    Ok(created)
}

/// Email a digest of a user's not-yet-emailed alerts **to that user's address**.
/// No-op (returns 0) when SMTP is not configured or nothing is pending. Each
/// alert is marked emailed on success.
pub async fn send_pending_emails_for_user(state: &AppState, user: &User) -> anyhow::Result<usize> {
    let Some(smtp) = state.config.smtp.as_ref() else {
        return Ok(0);
    };
    let mut tx = db::begin_as_user(&state.db, user.id).await?;
    let pending = db::queries::alerts::list_unemailed(&mut tx, user.id).await?;
    tx.commit().await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let mut body = String::from("Squirrel — new alerts:\n\n");
    for a in &pending {
        body.push_str(&format!("• {}\n  {}\n\n", a.title, a.message));
    }
    let subject = format!("Squirrel: {} new alert(s)", pending.len());
    // Recipient is the user's own email — not a global ALERT_EMAIL_TO. Sent
    // outside a DB transaction so a slow SMTP server can't pin a connection.
    crate::email::send(smtp, &user.email, &subject, body).await?;

    let mut tx = db::begin_as_user(&state.db, user.id).await?;
    for a in &pending {
        db::queries::alerts::mark_emailed(&mut tx, a.id).await?;
    }
    tx.commit().await?;
    tracing::info!(user = %user.id, sent = pending.len(), "alert email sent");
    Ok(pending.len())
}

/// Record today's portfolio totals as a daily snapshot (idempotent per day).
/// Market value sums only lots with a known current price; cost basis sums all
/// open lots. Backs the dashboard's value-over-time chart.
pub async fn record_snapshot_for_user(state: &AppState, user: &User) -> anyhow::Result<()> {
    use domain::accounts::AccountKind;
    let today = Utc::now().date_naive();
    // Per-account lot view so we can split by account kind and snapshot each scope.
    let mut tx = db::begin_as_user(&state.db, user.id).await?;
    let lots = db::queries::tax_lots::list_open_with_account(&mut tx, user.id).await?;

    // (market_value, cost_basis) per scope.
    let mut total = (Decimal::ZERO, Decimal::ZERO);
    let mut retirement = (Decimal::ZERO, Decimal::ZERO);
    let mut taxable = (Decimal::ZERO, Decimal::ZERO);
    for lot in &lots {
        let kind = AccountKind::resolve(
            lot.account_subtype.as_deref(),
            lot.account_kind_override.as_deref(),
        );
        if kind.is_debt() {
            continue; // liabilities aren't part of portfolio value
        }
        let cb = lot.remaining_quantity * lot.cost_basis_per_share;
        let mv = lot
            .close_price
            .map(|p| lot.remaining_quantity * p)
            .unwrap_or(Decimal::ZERO);
        total.0 += mv;
        total.1 += cb;
        let bucket = if kind.is_retirement() {
            &mut retirement
        } else {
            &mut taxable
        };
        bucket.0 += mv;
        bucket.1 += cb;
    }

    // Accounts valued from Plaid's balance (no lots) add to market value only.
    // Debt accounts are liabilities, so they're excluded.
    for a in db::queries::accounts::balance_only_accounts(&mut tx, user.id).await? {
        let kind = AccountKind::resolve(a.subtype.as_deref(), a.kind_override.as_deref());
        if kind.is_debt() {
            continue;
        }
        total.0 += a.current_balance;
        let bucket = if kind.is_retirement() {
            &mut retirement
        } else {
            &mut taxable
        };
        bucket.0 += a.current_balance;
    }

    for (scope, (mv, cb)) in [
        ("total", total),
        ("retirement", retirement),
        ("taxable", taxable),
    ] {
        db::queries::snapshots::upsert(&mut tx, user.id, today, scope, mv, cb).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Full cycle for one user: refresh that user's prices from Plaid (best-effort),
/// evaluate alert rules, then email any pending alerts to them.
pub async fn run_cycle_for_user(state: &AppState, user: &User) -> anyhow::Result<CycleSummary> {
    let mut summary = CycleSummary::default();

    // 1. Refresh prices by re-syncing each of the user's items (Plaid permitting).
    if state.plaid.is_configured() {
        if let Some(key) = state.config.token_encryption_key {
            let mut tx = db::begin_as_user(&state.db, user.id).await?;
            let items = db::queries::plaid_items::list_for_user(&mut tx, user.id).await?;
            tx.commit().await?;
            for item in &items {
                let client = state.plaid.for_item(item.plaid_client_id.as_deref());
                // Each item's sync gets its own tenant transaction (scoped to the
                // owner) so the Plaid network round-trips don't pin one for the
                // whole loop.
                let mut tx = db::begin_as_user(&state.db, user.id).await?;
                match crate::sync::sync_item(&mut tx, client, &key, item).await {
                    Ok(_) => {
                        tx.commit().await?;
                        summary.items_synced += 1;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, item = %item.plaid_item_id, "scheduled sync failed")
                    }
                }
            }
        }
    }

    // 2. Record today's value snapshot (non-fatal: a failure here must not block
    //    alert evaluation/email for this user).
    if let Err(e) = record_snapshot_for_user(state, user).await {
        tracing::error!(user = %user.id, error = %e, "snapshot failed");
    }

    // 3. Evaluate + 4. email.
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
