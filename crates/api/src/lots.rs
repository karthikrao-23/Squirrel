//! Rebuilds derived tax lots from stored transactions.
//!
//! This is the glue between the DB and the pure `domain::lots` reconstructor:
//! load the user's transactions, group them by (account, security), run FIFO on
//! each group, then atomically replace the stored lots. Called after every sync
//! (data changed → derived lots are stale) and exposed via `/api/lots/rebuild`.
//!
//! Because Plaid's investment-transaction feed only reaches back ~24 months, the
//! reconstructed lots don't match the current holdings: positions opened earlier
//! are under-counted, and positions sold (whose sells fell outside the window)
//! are over-counted. After FIFO we therefore [`reconcile`] the lots to the actual
//! holdings so the totals match Plaid's positions exactly — adding opening-balance
//! lots for the shortfall and trimming the excess.

use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, NaiveDate, Utc};
use db::queries::holdings::Position;
use db::queries::tax_lots::NewLot;
use domain::lots::{reconstruct_fifo, LotInput};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

/// Reconstruct and persist all tax lots for a user. Returns the number of open
/// lots stored. Runs on the caller's tenant transaction (RLS-scoped), so the
/// read → reconcile → replace all see and write only this user's rows.
pub async fn rebuild_lots(conn: &mut PgConnection, user_id: Uuid) -> anyhow::Result<u64> {
    let rows = db::queries::transactions::list_for_lots(&mut *conn, user_id).await?;

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

    let positions = db::queries::holdings::positions_for_user(&mut *conn, user_id).await?;
    reconcile(
        &mut new_lots,
        &positions,
        &earliest,
        global_earliest,
        Utc::now().date_naive(),
    );

    let count = db::queries::tax_lots::replace_for_user(&mut *conn, user_id, &new_lots).await?;
    tracing::info!(lots = count, user = %user_id, "tax lots rebuilt");
    Ok(count)
}

/// Quantities at or below this are treated as zero (guards against Plaid's
/// fractional-share rounding dust producing spurious lots).
fn qty_epsilon() -> Decimal {
    Decimal::new(1, 6) // 0.000001
}

/// Reconcile reconstructed lots to the current `positions` so the open-lot totals
/// match Plaid's holdings exactly, in **both** directions. Pure (no I/O), and
/// exhaustively unit-tested below. Lots fully consumed by trimming (remaining ≈ 0)
/// are removed.
///
/// * **Over-count** — FIFO left more shares lotted than the holding has (a position
///   partly/fully sold whose sells fell outside the ~24-month window). Trim the
///   excess oldest-first (FIFO sells oldest); a security no longer held has its
///   lots dropped entirely. Accounts Plaid won't share holdings for (e.g. Fidelity
///   BrokerageLink) therefore end up with no lots — their value is anchored to the
///   Plaid account balance instead (see `accounts::balance_only_accounts`).
/// * **Under-count** — the holding has more shares than were lotted (bought before
///   the window). Append one opening-balance lot for the difference, carrying the
///   residual of Plaid's position cost basis, dated before our earliest record and
///   `today - 367d` so it sorts oldest (FIFO) and classifies as long-term.
fn reconcile(
    new_lots: &mut Vec<NewLot>,
    positions: &[Position],
    earliest: &HashMap<(Uuid, Uuid), NaiveDate>,
    global_earliest: Option<NaiveDate>,
    today: NaiveDate,
) {
    let target_qty: HashMap<(Uuid, Uuid), Decimal> = positions
        .iter()
        .map(|p| ((p.account_id, p.security_id), p.quantity))
        .collect();

    // --- Over-count: trim each position's lots down to its holding quantity
    // (zero when not held), oldest-first. ---
    let mut idx_by_pos: HashMap<(Uuid, Uuid), Vec<usize>> = HashMap::new();
    for (i, l) in new_lots.iter().enumerate() {
        idx_by_pos
            .entry((l.account_id, l.security_id))
            .or_default()
            .push(i);
    }
    for (key, mut idxs) in idx_by_pos {
        let held = target_qty.get(&key).copied().unwrap_or(Decimal::ZERO);
        let current: Decimal = idxs.iter().map(|&i| new_lots[i].remaining_quantity).sum();
        let mut excess = current - held;
        if excess <= qty_epsilon() {
            continue;
        }
        idxs.sort_by_key(|&i| new_lots[i].open_date); // oldest first
        for &i in &idxs {
            if excess <= qty_epsilon() {
                break;
            }
            let take = excess.min(new_lots[i].remaining_quantity);
            new_lots[i].remaining_quantity -= take;
            excess -= take;
        }
    }

    // --- Under-count: append opening-balance lots. Reconstructed totals are
    // recomputed from the (now-trimmed) lots. ---
    let mut recon: HashMap<(Uuid, Uuid), (Decimal, Decimal)> = HashMap::new();
    for lot in new_lots.iter() {
        let e = recon
            .entry((lot.account_id, lot.security_id))
            .or_insert((Decimal::ZERO, Decimal::ZERO));
        e.0 += lot.remaining_quantity;
        e.1 += lot.remaining_quantity * lot.cost_basis_per_share;
    }

    let long_term_cutoff = today - Duration::days(367);
    let day_before = |d: NaiveDate| d.pred_opt().unwrap_or(d);

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

    // Drop lots fully consumed by the over-count trim.
    new_lots.retain(|l| l.remaining_quantity > qty_epsilon());
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::ToPrimitive;
    use uuid::Uuid;

    const A: Uuid = Uuid::from_u128(0xA);
    const S: Uuid = Uuid::from_u128(0x5); // one security used across most tests

    fn d(n: i64) -> Decimal {
        Decimal::from(n)
    }
    fn date(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    fn today() -> NaiveDate {
        date(2026, 6, 30)
    }

    /// A reconstructed lot: (open_date, remaining_qty, cost_basis_per_share).
    fn lot(open: NaiveDate, qty: Decimal, basis: Decimal) -> NewLot {
        NewLot {
            account_id: A,
            security_id: S,
            open_date: open,
            original_quantity: qty,
            remaining_quantity: qty,
            cost_basis_per_share: basis,
            source_transaction_id: Some(Uuid::from_u128(0x7)),
        }
    }
    fn pos(qty: Decimal, basis: Option<Decimal>, price: Option<Decimal>) -> Position {
        Position {
            account_id: A,
            security_id: S,
            quantity: qty,
            cost_basis: basis,
            institution_price: price,
        }
    }
    fn run(lots: &mut Vec<NewLot>, positions: &[Position]) {
        let earliest: HashMap<(Uuid, Uuid), NaiveDate> =
            [((A, S), date(2024, 7, 1))].into_iter().collect();
        reconcile(lots, positions, &earliest, Some(date(2024, 7, 1)), today());
    }
    fn total_qty(lots: &[NewLot]) -> Decimal {
        lots.iter().map(|l| l.remaining_quantity).sum()
    }

    #[test]
    fn exact_match_is_unchanged() {
        let mut lots = vec![lot(date(2025, 1, 1), d(100), d(10))];
        run(&mut lots, &[pos(d(100), Some(d(1000)), None)]);
        assert_eq!(lots.len(), 1);
        assert_eq!(total_qty(&lots), d(100));
    }

    #[test]
    fn under_count_appends_long_term_opening_lot() {
        // Holding has 100 shares; only 30 are lotted → add a 70-share opening lot.
        let mut lots = vec![lot(date(2025, 1, 1), d(30), d(12))];
        run(&mut lots, &[pos(d(100), Some(d(900)), Some(d(20)))]);
        assert_eq!(total_qty(&lots), d(100), "lots reconcile to the holding");
        let opening = lots
            .iter()
            .find(|l| l.source_transaction_id.is_none())
            .unwrap();
        assert_eq!(opening.remaining_quantity, d(70));
        // basis residual = position basis 900 − lotted (30×12=360) = 540 over 70 sh.
        assert_eq!(opening.cost_basis_per_share, (d(540) / d(70)).round_dp(6));
        // Dated long-term: before today−367d.
        assert!(opening.open_date <= today() - Duration::days(367));
        assert!(opening.open_date < date(2025, 1, 1));
    }

    #[test]
    fn over_count_trims_oldest_first() {
        // 70 lotted (oldest 40 + newer 30) but only 50 held → trim 20 from oldest.
        let mut lots = vec![
            lot(date(2024, 8, 1), d(40), d(10)), // oldest
            lot(date(2025, 3, 1), d(30), d(15)),
        ];
        run(&mut lots, &[pos(d(50), Some(d(600)), None)]);
        assert_eq!(total_qty(&lots), d(50));
        // Oldest reduced 40 → 20; newer untouched at 30.
        let oldest = lots
            .iter()
            .find(|l| l.open_date == date(2024, 8, 1))
            .unwrap();
        assert_eq!(oldest.remaining_quantity, d(20));
        let newer = lots
            .iter()
            .find(|l| l.open_date == date(2025, 3, 1))
            .unwrap();
        assert_eq!(newer.remaining_quantity, d(30));
    }

    #[test]
    fn sold_security_drops_when_account_is_covered() {
        // The account IS covered by holdings (another security is still held), but
        // *this* security is gone → its lots are trimmed away. This is the genuine
        // "sold" case, distinct from holdings being unavailable.
        const S2: Uuid = Uuid::from_u128(0x6);
        let mut lots = vec![
            lot(date(2024, 8, 1), d(5), d(10)),
            lot(date(2025, 3, 1), d(3), d(15)),
        ];
        // Account A now holds only S2; S is absent from positions.
        let held_other = Position {
            account_id: A,
            security_id: S2,
            quantity: d(4),
            cost_basis: Some(d(40)),
            institution_price: None,
        };
        run(&mut lots, &[held_other]);
        assert!(
            lots.iter().all(|l| l.security_id != S),
            "sold security's lots are dropped when the account is covered"
        );
    }

    #[test]
    fn no_holdings_drops_all_lots() {
        // No positions at all → every lot is trimmed away. Accounts Plaid won't
        // share holdings for (e.g. Fidelity BrokerageLink) land here and are valued
        // from their Plaid balance instead (accounts::balance_only_accounts), rather
        // than from an unreliable transaction-window estimate.
        let mut lots = vec![
            lot(date(2024, 8, 1), d(5), d(10)),
            lot(date(2025, 3, 1), d(3), d(15)),
        ];
        run(&mut lots, &[]); // no positions
        assert!(lots.is_empty(), "no holdings → no open lots");
    }

    #[test]
    fn aggregates_multiple_holdings_quantity_via_position() {
        // Mirrors the ESPP case: the position quantity is the SUM the caller stored
        // (e.g. CRM 562 + 1425 = 1987). Reconcile must value all of it.
        let mut lots = vec![lot(date(2025, 1, 1), d(1425), d(100))];
        run(&mut lots, &[pos(d(1987), Some(d(200_000)), Some(d(158)))]);
        assert_eq!(total_qty(&lots), d(1987));
    }

    #[test]
    fn tiny_gap_adds_no_opening_lot() {
        // A sub-epsilon shortfall (rounding dust) must not create a spurious lot.
        let mut lots = vec![lot(date(2025, 1, 1), d(100), d(10))];
        let positions = [pos(d(100) + Decimal::new(1, 9), Some(d(1000)), None)];
        run(&mut lots, &positions);
        assert_eq!(lots.len(), 1);
    }

    #[test]
    fn missing_basis_falls_back_to_current_price() {
        // No Plaid cost basis → opening lot priced at current price (zero gain).
        let mut lots: Vec<NewLot> = vec![];
        run(&mut lots, &[pos(d(10), None, Some(d(50)))]);
        let opening = &lots[0];
        assert_eq!(opening.remaining_quantity, d(10));
        assert_eq!(opening.cost_basis_per_share, d(50));
    }

    #[test]
    fn independent_positions_reconcile_separately() {
        let s2 = Uuid::from_u128(0x6);
        let mut lots = vec![
            lot(date(2025, 1, 1), d(30), d(10)), // (A,S): under (held 100)
            NewLot {
                security_id: s2,
                ..lot(date(2025, 1, 1), d(80), d(10)) // (A,s2): over (held 50)
            },
        ];
        let positions = [
            pos(d(100), Some(d(1000)), None),
            Position {
                security_id: s2,
                ..pos(d(50), Some(d(500)), None)
            },
        ];
        let earliest = [((A, S), date(2024, 7, 1)), ((A, s2), date(2024, 7, 1))]
            .into_iter()
            .collect();
        reconcile(
            &mut lots,
            &positions,
            &earliest,
            Some(date(2024, 7, 1)),
            today(),
        );
        let q = |sec: Uuid| -> f64 {
            lots.iter()
                .filter(|l| l.security_id == sec)
                .map(|l| l.remaining_quantity)
                .sum::<Decimal>()
                .to_f64()
                .unwrap()
        };
        assert_eq!(q(S), 100.0);
        assert_eq!(q(s2), 50.0);
    }
}
