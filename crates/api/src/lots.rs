//! Rebuilds derived tax lots from stored transactions.
//!
//! This is the glue between the DB and the pure `domain::lots` reconstructor:
//! load the user's transactions, group them by (account, security), run FIFO on
//! each group, then atomically replace the stored lots. Called after every sync
//! (data changed → derived lots are stale) and exposed via `/api/lots/rebuild`.
//!
//! Because Plaid's investment-transaction feed only reaches back ~24 months, a
//! position bought earlier has no transactions to reconstruct from — its shares
//! would be missing from valuation and tax math. After FIFO we therefore
//! reconcile to the actual holdings, synthesizing an **opening-balance** lot for
//! any shares a holding has beyond what the transactions account for.

use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, NaiveDate, Utc};
use db::queries::tax_lots::NewLot;
use domain::lots::{reconstruct_fifo, LotInput};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Reconstruct and persist all tax lots for a user. Returns the number of open
/// lots stored.
pub async fn rebuild_lots(pool: &PgPool, user_id: Uuid) -> anyhow::Result<u64> {
    let rows = db::queries::transactions::list_for_lots(pool, user_id).await?;

    // Group transactions by (account, security). BTreeMap keeps a deterministic
    // order, which keeps the resulting lot rows stable across rebuilds. While we
    // group, also remember each group's earliest transaction date — opening lots
    // are dated just before it so they're the oldest (FIFO) and pre-window.
    let mut groups: BTreeMap<(Uuid, Uuid), Vec<LotInput>> = BTreeMap::new();
    let mut earliest: HashMap<(Uuid, Uuid), NaiveDate> = HashMap::new();
    let mut global_earliest: Option<NaiveDate> = None;
    for r in rows {
        let key = (r.account_id, r.security_id);
        earliest
            .entry(key)
            .and_modify(|d| {
                if r.date < *d {
                    *d = r.date;
                }
            })
            .or_insert(r.date);
        global_earliest = Some(global_earliest.map_or(r.date, |g| g.min(r.date)));
        groups.entry(key).or_default().push(LotInput {
            source_transaction_id: r.id,
            date: r.date,
            transaction_type: r.transaction_type,
            quantity: r.quantity,
            price: r.price,
            amount: r.amount,
            fees: r.fees,
        });
    }

    let mut new_lots: Vec<NewLot> = Vec::new();
    for ((account_id, security_id), inputs) in groups {
        for lot in reconstruct_fifo(&inputs) {
            new_lots.push(NewLot {
                account_id,
                security_id,
                open_date: lot.open_date,
                original_quantity: lot.original_quantity,
                remaining_quantity: lot.remaining_quantity,
                cost_basis_per_share: lot.cost_basis_per_share,
                source_transaction_id: Some(lot.source_transaction_id),
            });
        }
    }

    append_opening_lots(pool, user_id, &mut new_lots, &earliest, global_earliest).await?;

    let count = db::queries::tax_lots::replace_for_user(pool, user_id, &new_lots).await?;
    tracing::info!(lots = count, user = %user_id, "tax lots rebuilt");
    Ok(count)
}

/// Quantities at or below this are treated as zero (guards against Plaid's
/// fractional-share rounding dust producing spurious opening lots).
fn qty_epsilon() -> Decimal {
    Decimal::new(1, 6) // 0.000001
}

/// Reconcile reconstructed lots to the actual holdings. For each position whose
/// holding quantity exceeds the reconstructed lot quantity, append one
/// opening-balance lot for the difference: priced at the current price (via the
/// usual holdings join when valued) and carrying the residual of Plaid's
/// position-level cost basis. These shares predate our transaction history, so
/// they're dated before it and treated as long-term.
async fn append_opening_lots(
    pool: &PgPool,
    user_id: Uuid,
    new_lots: &mut Vec<NewLot>,
    earliest: &HashMap<(Uuid, Uuid), NaiveDate>,
    global_earliest: Option<NaiveDate>,
) -> anyhow::Result<()> {
    // Quantity + cost basis already accounted for by reconstructed lots, per position.
    let mut recon: HashMap<(Uuid, Uuid), (Decimal, Decimal)> = HashMap::new();
    for lot in new_lots.iter() {
        let e = recon
            .entry((lot.account_id, lot.security_id))
            .or_insert((Decimal::ZERO, Decimal::ZERO));
        e.0 += lot.remaining_quantity;
        e.1 += lot.remaining_quantity * lot.cost_basis_per_share;
    }

    // Opening lots are dated before our earliest record AND > 1 year ago, so they
    // sort oldest for FIFO and classify as long-term (prior holdings).
    let long_term_cutoff = Utc::now().date_naive() - Duration::days(367);
    let day_before = |d: NaiveDate| d.pred_opt().unwrap_or(d);

    let positions = db::queries::holdings::positions_for_user(pool, user_id).await?;
    for p in positions {
        let key = (p.account_id, p.security_id);
        let (recon_qty, recon_basis) = recon
            .get(&key)
            .copied()
            .unwrap_or((Decimal::ZERO, Decimal::ZERO));
        let gap = p.quantity - recon_qty;
        if gap <= qty_epsilon() {
            continue;
        }

        // Basis of the opening shares = position basis minus what's already lotted
        // (clamped ≥ 0). With no Plaid basis, fall back to current price → no gain.
        let basis_total = match p.cost_basis {
            Some(cb) => (cb - recon_basis).max(Decimal::ZERO),
            None => p
                .institution_price
                .map(|px| px * gap)
                .unwrap_or(Decimal::ZERO),
        };
        let cost_basis_per_share = (basis_total / gap).round_dp(6);

        let open_date = earliest
            .get(&key)
            .map(|d| day_before(*d))
            .or_else(|| global_earliest.map(day_before))
            .map(|d| d.min(long_term_cutoff))
            .unwrap_or(long_term_cutoff);

        new_lots.push(NewLot {
            account_id: p.account_id,
            security_id: p.security_id,
            open_date,
            original_quantity: gap,
            remaining_quantity: gap,
            cost_basis_per_share,
            source_transaction_id: None,
        });
    }
    Ok(())
}
