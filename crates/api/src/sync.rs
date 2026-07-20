//! Pulls a linked item's investment data from its brokerage provider and
//! persists it.
//!
//! Flow: decrypt the access token → fetch holdings (securities, accounts,
//! positions) → fetch investment transactions over the last ~24 months.
//! Everything upserts, so re-running (manual sync or webhook) is idempotent.
//!
//! This layer is provider-agnostic: it speaks to a [`BrokerageProvider`] and
//! consumes the neutral `Decimal`-money DTOs it returns (each provider does its
//! own float conversion + pagination). Plaid is one such provider.

use std::collections::HashMap;

use broker::{BrokerSecurity, BrokerageProvider};
use chrono::{Duration, NaiveDate, Utc};
use db::models::PlaidItem;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

/// Investment transactions are fetched back at most 24 months.
const LOOKBACK_DAYS: i64 = 730;

#[derive(Debug, Default, Serialize)]
pub struct SyncSummary {
    pub accounts: usize,
    pub securities: usize,
    pub holdings: usize,
    pub transactions_inserted: usize,
}

/// Sum two optional decimals; `None` is absent (not zero), so an all-`None`
/// aggregate stays `None`.
fn add_opt(a: Option<Decimal>, b: Option<Decimal>) -> Option<Decimal> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (a, b) => a.or(b),
    }
}

/// One resolved holding row (account/security already mapped to our UUIDs),
/// ready to aggregate.
struct HoldingRow {
    account_id: Uuid,
    security_id: Uuid,
    quantity: Decimal,
    institution_value: Option<Decimal>,
    cost_basis: Option<Decimal>,
    institution_price: Option<Decimal>,
    institution_price_as_of: Option<NaiveDate>,
    currency: Option<String>,
}

/// Accumulator for the (possibly several) Plaid holding rows of one position.
#[derive(Default)]
struct PositionAgg {
    quantity: Decimal,
    institution_value: Option<Decimal>,
    cost_basis: Option<Decimal>,
    institution_price: Option<Decimal>,
    institution_price_as_of: Option<NaiveDate>,
    currency: Option<String>,
}

/// Collapse the (possibly several) holding rows for the same (account, security)
/// into one position: sum quantity / value / cost basis; the per-share price +
/// currency are the same across the split rows, so keep the first. Providers
/// (e.g. Plaid) split stock-plan positions (ESPP / RSU / purchased) into separate
/// rows — without this they'd clobber each other on upsert and shares would be
/// silently dropped. Pure; unit-tested.
fn aggregate_holdings(rows: Vec<HoldingRow>) -> Vec<(Uuid, Uuid, PositionAgg)> {
    let mut by_pos: HashMap<(Uuid, Uuid), PositionAgg> = HashMap::new();
    for r in rows {
        let agg = by_pos.entry((r.account_id, r.security_id)).or_default();
        agg.quantity += r.quantity;
        agg.institution_value = add_opt(agg.institution_value, r.institution_value);
        agg.cost_basis = add_opt(agg.cost_basis, r.cost_basis);
        agg.institution_price = agg.institution_price.or(r.institution_price);
        agg.institution_price_as_of = agg.institution_price_as_of.or(r.institution_price_as_of);
        if agg.currency.is_none() {
            agg.currency = r.currency;
        }
    }
    by_pos
        .into_iter()
        .map(|((a, s), agg)| (a, s, agg))
        .collect()
}

/// Upsert one security and remember its UUID keyed by the provider's security id.
async fn upsert_security(
    conn: &mut PgConnection,
    map: &mut HashMap<String, Uuid>,
    s: &BrokerSecurity,
) -> anyhow::Result<()> {
    let id = db::queries::securities::upsert(
        &mut *conn,
        &s.external_id,
        s.ticker.as_deref(),
        s.name.as_deref(),
        s.cusip.as_deref(),
        s.security_type.as_deref(),
        s.close_price,
        s.close_price_as_of,
        s.currency.as_deref(),
    )
    .await?;
    map.insert(s.external_id.clone(), id);
    Ok(())
}

/// Sync a single item end-to-end and return a count summary.
pub async fn sync_item(
    conn: &mut PgConnection,
    provider: &dyn BrokerageProvider,
    key: &[u8; 32],
    item: &PlaidItem,
) -> anyhow::Result<SyncSummary> {
    let access_token =
        String::from_utf8(crate::crypto::decrypt(key, &item.access_token_encrypted)?)?;
    let mut summary = SyncSummary::default();

    // --- Holdings (also yields the securities + accounts they reference) ---
    let holdings = provider.fetch_holdings(&access_token).await?;

    if let Some(inst) = holdings.institution_id.as_deref() {
        db::queries::plaid_items::set_institution_id(&mut *conn, item.id, inst).await?;
    }

    let mut sec_map: HashMap<String, Uuid> = HashMap::new();
    for s in &holdings.securities {
        upsert_security(&mut *conn, &mut sec_map, s).await?;
    }
    summary.securities = sec_map.len();

    let mut acct_map: HashMap<String, Uuid> = HashMap::new();
    for a in &holdings.accounts {
        let id = db::queries::accounts::upsert(
            &mut *conn,
            item.user_id,
            item.id,
            &a.external_id,
            &a.name,
            a.official_name.as_deref(),
            a.account_type.as_deref(),
            a.subtype.as_deref(),
            a.current_balance,
        )
        .await?;
        acct_map.insert(a.external_id.clone(), id);
    }
    summary.accounts = acct_map.len();

    // A provider can return MULTIPLE holdings for the same (account, security) —
    // most commonly stock-plan accounts that split a position into ESPP / RSU /
    // purchased lots (by cost basis). Our `holdings` row is one position per
    // (account, security), so aggregate (sum quantity / value / cost basis)
    // before upserting; otherwise later rows clobber earlier ones and shares are
    // silently dropped. The per-share price + currency are the same across the
    // split rows, so we keep the first non-null.
    let mut rows: Vec<HoldingRow> = Vec::new();
    for h in &holdings.holdings {
        let (Some(&account_id), Some(&security_id)) = (
            acct_map.get(&h.account_external_id),
            sec_map.get(&h.security_external_id),
        ) else {
            tracing::warn!(account = %h.account_external_id, security = %h.security_external_id, "holding references unknown account/security; skipping");
            continue;
        };
        rows.push(HoldingRow {
            account_id,
            security_id,
            quantity: h.quantity,
            institution_value: h.value,
            cost_basis: h.cost_basis,
            institution_price: h.price,
            institution_price_as_of: h.price_as_of,
            currency: h.currency.clone(),
        });
    }
    for (account_id, security_id, agg) in aggregate_holdings(rows) {
        db::queries::holdings::upsert(
            &mut *conn,
            item.user_id,
            account_id,
            security_id,
            agg.quantity,
            agg.institution_price,
            agg.institution_price_as_of,
            agg.institution_value,
            agg.cost_basis,
            agg.currency.as_deref(),
        )
        .await?;
        summary.holdings += 1;
    }

    // --- Investment transactions (provider handles pagination) ---
    let end = Utc::now().date_naive();
    let start = end - Duration::days(LOOKBACK_DAYS);
    let batch = provider
        .fetch_transactions(&access_token, start, end)
        .await?;

    // Transactions can reference securities not in the holdings snapshot.
    for s in &batch.securities {
        if !sec_map.contains_key(&s.external_id) {
            upsert_security(&mut *conn, &mut sec_map, s).await?;
        }
    }
    summary.securities = sec_map.len();

    for t in &batch.transactions {
        let Some(&account_id) = acct_map.get(&t.account_external_id) else {
            tracing::warn!(account = %t.account_external_id, "transaction references unknown account; skipping");
            continue;
        };
        let security_id = t
            .security_external_id
            .as_ref()
            .and_then(|sid| sec_map.get(sid).copied());

        let inserted = db::queries::transactions::insert_ignore(
            &mut *conn,
            &db::queries::transactions::NewTransaction {
                user_id: item.user_id,
                account_id,
                security_id,
                plaid_investment_transaction_id: &t.external_id,
                transaction_type: t.txn_type.as_deref(),
                subtype: t.subtype.as_deref(),
                quantity: t.quantity,
                price: t.price,
                amount: t.amount,
                fees: t.fees,
                date: t.date,
                name: t.name.as_deref(),
                currency: t.currency.as_deref(),
            },
        )
        .await?;
        if inserted {
            summary.transactions_inserted += 1;
        }
    }

    // Transactions changed, so derived tax lots are stale — rebuild them for the
    // item's owner (never a default/global user).
    crate::lots::rebuild_lots(&mut *conn, item.user_id).await?;

    tracing::info!(?summary, item = %item.plaid_item_id, "sync complete");
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Uuid = Uuid::from_u128(0xA);
    const S: Uuid = Uuid::from_u128(0x5);

    fn row(qty: i64, value: Option<i64>, basis: Option<i64>, price: Option<i64>) -> HoldingRow {
        HoldingRow {
            account_id: A,
            security_id: S,
            quantity: Decimal::from(qty),
            institution_value: value.map(Decimal::from),
            cost_basis: basis.map(Decimal::from),
            institution_price: price.map(Decimal::from),
            institution_price_as_of: None,
            currency: Some("USD".into()),
        }
    }

    #[test]
    fn add_opt_sums_present_and_keeps_single() {
        assert_eq!(
            add_opt(Some(Decimal::from(2)), Some(Decimal::from(3))),
            Some(Decimal::from(5))
        );
        assert_eq!(
            add_opt(Some(Decimal::from(2)), None),
            Some(Decimal::from(2))
        );
        assert_eq!(
            add_opt(None, Some(Decimal::from(3))),
            Some(Decimal::from(3))
        );
        assert_eq!(add_opt(None, None), None);
    }

    #[test]
    fn aggregates_split_rows_for_same_position() {
        // The ESPP case: Plaid returns two CRM rows for one stock-plan account.
        // 562 + 1425 must sum to 1987, not clobber to 1425.
        let out = aggregate_holdings(vec![
            row(562, Some(88_964), Some(70_000), Some(158)),
            row(1425, Some(225_577), Some(180_000), Some(158)),
        ]);
        assert_eq!(out.len(), 1, "one aggregated position");
        let (a, s, agg) = &out[0];
        assert_eq!((*a, *s), (A, S));
        assert_eq!(agg.quantity, Decimal::from(1987));
        assert_eq!(agg.institution_value, Some(Decimal::from(314_541)));
        assert_eq!(agg.cost_basis, Some(Decimal::from(250_000)));
        assert_eq!(agg.institution_price, Some(Decimal::from(158)));
    }

    #[test]
    fn distinct_positions_are_kept_separate() {
        let s2 = Uuid::from_u128(0x6);
        let out = aggregate_holdings(vec![
            row(10, Some(100), None, None),
            HoldingRow {
                security_id: s2,
                ..row(20, Some(200), None, None)
            },
        ]);
        assert_eq!(out.len(), 2);
        let q: std::collections::HashMap<_, _> = out
            .into_iter()
            .map(|(_, s, agg)| (s, agg.quantity))
            .collect();
        assert_eq!(q[&S], Decimal::from(10));
        assert_eq!(q[&s2], Decimal::from(20));
    }
}
