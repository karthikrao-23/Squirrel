//! Pulls a linked item's investment data from Plaid and persists it.
//!
//! Flow: decrypt the access token → fetch holdings (securities, accounts,
//! positions) → page through investment transactions over the last ~24 months.
//! Everything upserts, so re-running (manual sync or webhook) is idempotent.
//!
//! Plaid sends numbers as JSON floats; we convert to `Decimal` here so money is
//! never stored as a float downstream.

use std::collections::HashMap;

use chrono::{Duration, Utc};
use db::models::PlaidItem;
use plaid::models::PlaidSecurity;
use plaid::transactions::MAX_PAGE_SIZE;
use plaid::PlaidClient;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

/// Plaid investment transactions go back at most 24 months.
const LOOKBACK_DAYS: i64 = 730;

#[derive(Debug, Default, Serialize)]
pub struct SyncSummary {
    pub accounts: usize,
    pub securities: usize,
    pub holdings: usize,
    pub transactions_inserted: usize,
}

fn dec(v: f64) -> Decimal {
    Decimal::from_f64_retain(v).unwrap_or_default()
}

fn odec(v: Option<f64>) -> Option<Decimal> {
    v.and_then(Decimal::from_f64_retain)
}

/// Upsert one security and remember its UUID keyed by Plaid's security id.
async fn upsert_security(
    pool: &PgPool,
    map: &mut HashMap<String, Uuid>,
    s: &PlaidSecurity,
) -> anyhow::Result<()> {
    let id = db::queries::securities::upsert(
        pool,
        &s.security_id,
        s.ticker_symbol.as_deref(),
        s.name.as_deref(),
        s.cusip.as_deref(),
        s.security_type.as_deref(),
        odec(s.close_price),
        s.close_price_as_of,
        s.iso_currency_code.as_deref(),
    )
    .await?;
    map.insert(s.security_id.clone(), id);
    Ok(())
}

/// Sync a single item end-to-end and return a count summary.
pub async fn sync_item(
    pool: &PgPool,
    plaid: &PlaidClient,
    key: &[u8; 32],
    item: &PlaidItem,
) -> anyhow::Result<SyncSummary> {
    let access_token = String::from_utf8(crate::crypto::decrypt(key, &item.access_token_encrypted)?)?;
    let mut summary = SyncSummary::default();

    // --- Holdings (also yields the securities + accounts they reference) ---
    let holdings = plaid.get_holdings(&access_token).await?;

    if let Some(inst) = holdings.item.institution_id.as_deref() {
        db::queries::plaid_items::set_institution_id(pool, item.id, inst).await?;
    }

    let mut sec_map: HashMap<String, Uuid> = HashMap::new();
    for s in &holdings.securities {
        upsert_security(pool, &mut sec_map, s).await?;
    }
    summary.securities = sec_map.len();

    let mut acct_map: HashMap<String, Uuid> = HashMap::new();
    for a in &holdings.accounts {
        let id = db::queries::accounts::upsert(
            pool,
            item.user_id,
            item.id,
            &a.account_id,
            &a.name,
            a.official_name.as_deref(),
            a.account_type.as_deref(),
            a.subtype.as_deref(),
        )
        .await?;
        acct_map.insert(a.account_id.clone(), id);
    }
    summary.accounts = acct_map.len();

    for h in &holdings.holdings {
        let (Some(&account_id), Some(&security_id)) =
            (acct_map.get(&h.account_id), sec_map.get(&h.security_id))
        else {
            tracing::warn!(account = %h.account_id, security = %h.security_id, "holding references unknown account/security; skipping");
            continue;
        };
        db::queries::holdings::upsert(
            pool,
            item.user_id,
            account_id,
            security_id,
            dec(h.quantity),
            odec(h.institution_price),
            h.institution_price_as_of,
            odec(h.institution_value),
            odec(h.cost_basis),
            h.iso_currency_code.as_deref(),
        )
        .await?;
        summary.holdings += 1;
    }

    // --- Investment transactions (paginated) ---
    let end = Utc::now().date_naive();
    let start = end - Duration::days(LOOKBACK_DAYS);
    let mut offset: u32 = 0;
    loop {
        let page = plaid
            .get_investments_transactions_page(&access_token, start, end, offset, MAX_PAGE_SIZE)
            .await?;

        // Transactions can reference securities not in the holdings snapshot.
        for s in &page.securities {
            if !sec_map.contains_key(&s.security_id) {
                upsert_security(pool, &mut sec_map, s).await?;
            }
        }

        let page_len = page.investment_transactions.len() as u32;
        for t in &page.investment_transactions {
            let Some(&account_id) = acct_map.get(&t.account_id) else {
                tracing::warn!(account = %t.account_id, "transaction references unknown account; skipping");
                continue;
            };
            let security_id = t
                .security_id
                .as_ref()
                .and_then(|sid| sec_map.get(sid).copied());

            let inserted = db::queries::transactions::insert_ignore(
                pool,
                &db::queries::transactions::NewTransaction {
                    user_id: item.user_id,
                    account_id,
                    security_id,
                    plaid_investment_transaction_id: &t.investment_transaction_id,
                    transaction_type: t.transaction_type.as_deref(),
                    subtype: t.subtype.as_deref(),
                    quantity: odec(t.quantity),
                    price: odec(t.price),
                    amount: odec(t.amount),
                    fees: odec(t.fees),
                    date: t.date,
                    name: t.name.as_deref(),
                    currency: t.iso_currency_code.as_deref(),
                },
            )
            .await?;
            if inserted {
                summary.transactions_inserted += 1;
            }
        }

        summary.securities = sec_map.len();
        offset += page_len;
        if page_len == 0 || offset >= page.total_investment_transactions {
            break;
        }
    }

    tracing::info!(?summary, item = %item.plaid_item_id, "sync complete");
    Ok(summary)
}
