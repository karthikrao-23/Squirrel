//! Cost-basis **tax lot** reconstruction from a transaction history.
//!
//! Plaid (and brokerages generally) don't hand us per-purchase lots, so we
//! rebuild them from the transaction feed. Each *buy* opens a lot; each *sell*
//! closes shares **FIFO** (oldest lot first), which is how brokers report
//! realized gains by default. The result is the set of still-open lots, which
//! drive holding-period and gain/loss math in later milestones.
//!
//! This module is pure (no DB/HTTP): callers group transactions by
//! `(account, security)` and feed each group in, which keeps it trivial to
//! unit-test. Cost basis includes fees (`(price × qty + fees) / qty`).
//!
//! Known M3 simplifications: only `buy`/`sell` affect lots; dividends, fees,
//! cash, and transfers are ignored here. Over-selling more than we have a record
//! of (e.g. buys older than Plaid's 24-month window) simply stops closing rather
//! than erroring.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::VecDeque;
use uuid::Uuid;

/// One transaction, already narrowed to a single (account, security).
#[derive(Debug, Clone)]
pub struct LotInput {
    pub source_transaction_id: Uuid,
    pub date: NaiveDate,
    /// Plaid transaction `type` (e.g. "buy", "sell"); case-insensitive.
    pub transaction_type: Option<String>,
    /// Signed quantity as reported; we use the magnitude.
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub fees: Option<Decimal>,
}

/// A still-open tax lot produced by reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLot {
    pub source_transaction_id: Uuid,
    pub open_date: NaiveDate,
    /// Shares originally acquired in this lot.
    pub original_quantity: Decimal,
    /// Shares still held (≤ original after FIFO sells).
    pub remaining_quantity: Decimal,
    pub cost_basis_per_share: Decimal,
}

/// What a transaction does to the lot queue.
enum LotAction {
    Open,
    Close,
    Ignore,
}

fn classify(transaction_type: Option<&str>) -> LotAction {
    match transaction_type.map(|t| t.to_ascii_lowercase()) {
        Some(t) if t == "buy" => LotAction::Open,
        Some(t) if t == "sell" => LotAction::Close,
        _ => LotAction::Ignore,
    }
}

/// Cost basis per share, fees included: `(price × qty + fees) / qty`. Falls back
/// to `|amount| / qty` when price is absent. Returns `None` for zero quantity.
fn cost_basis_per_share(input: &LotInput, qty: Decimal) -> Option<Decimal> {
    if qty.is_zero() {
        return None;
    }
    let base_total = match input.price {
        Some(price) => price * qty,
        None => input.amount?.abs(),
    };
    let fees = input.fees.unwrap_or(Decimal::ZERO);
    Some((base_total + fees) / qty)
}

/// Reconstruct the open lots for one (account, security) group.
///
/// `inputs` need not be pre-sorted; we sort by `(date, id)` so the FIFO order is
/// deterministic even when several transactions share a date.
pub fn reconstruct_fifo(inputs: &[LotInput]) -> Vec<OpenLot> {
    let mut sorted: Vec<&LotInput> = inputs.iter().collect();
    sorted.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then_with(|| a.source_transaction_id.cmp(&b.source_transaction_id))
    });

    let mut open: VecDeque<OpenLot> = VecDeque::new();

    for input in sorted {
        let qty = input.quantity.map(|q| q.abs()).unwrap_or(Decimal::ZERO);
        if qty.is_zero() {
            continue;
        }

        match classify(input.transaction_type.as_deref()) {
            LotAction::Open => {
                if let Some(per_share) = cost_basis_per_share(input, qty) {
                    open.push_back(OpenLot {
                        source_transaction_id: input.source_transaction_id,
                        open_date: input.date,
                        original_quantity: qty,
                        remaining_quantity: qty,
                        cost_basis_per_share: per_share,
                    });
                }
            }
            LotAction::Close => close_fifo(&mut open, qty),
            LotAction::Ignore => {}
        }
    }

    open.into_iter().collect()
}

/// Remove `to_close` shares from the front of the queue, oldest first. Stops
/// (rather than going negative) if we run out of recorded lots.
fn close_fifo(open: &mut VecDeque<OpenLot>, mut to_close: Decimal) {
    while to_close > Decimal::ZERO {
        let Some(front) = open.front_mut() else {
            break; // no recorded buys left to sell against
        };
        if front.remaining_quantity <= to_close {
            to_close -= front.remaining_quantity;
            open.pop_front();
        } else {
            front.remaining_quantity -= to_close;
            to_close = Decimal::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn buy(id: u128, date: &str, qty: Decimal, price: Decimal, fees: Option<Decimal>) -> LotInput {
        LotInput {
            source_transaction_id: Uuid::from_u128(id),
            date: d(date),
            transaction_type: Some("buy".into()),
            quantity: Some(qty),
            price: Some(price),
            amount: Some(qty * price),
            fees,
        }
    }

    fn sell(id: u128, date: &str, qty: Decimal) -> LotInput {
        LotInput {
            source_transaction_id: Uuid::from_u128(id),
            date: d(date),
            transaction_type: Some("sell".into()),
            // Plaid reports sells with negative quantity; reconstruction uses |q|.
            quantity: Some(-qty),
            price: Some(dec!(0)),
            amount: Some(dec!(0)),
            fees: None,
        }
    }

    #[test]
    fn single_buy_opens_one_lot() {
        let lots = reconstruct_fifo(&[buy(1, "2024-01-01", dec!(10), dec!(5), None)]);
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].remaining_quantity, dec!(10));
        assert_eq!(lots[0].cost_basis_per_share, dec!(5));
    }

    #[test]
    fn cost_basis_includes_fees() {
        // (5*10 + 10) / 10 = 6
        let lots = reconstruct_fifo(&[buy(1, "2024-01-01", dec!(10), dec!(5), Some(dec!(10)))]);
        assert_eq!(lots[0].cost_basis_per_share, dec!(6));
    }

    #[test]
    fn partial_sell_reduces_oldest_lot_first() {
        let lots = reconstruct_fifo(&[
            buy(1, "2024-01-01", dec!(10), dec!(5), None),
            buy(2, "2024-02-01", dec!(10), dec!(7), None),
            sell(3, "2024-03-01", dec!(5)),
        ]);
        assert_eq!(lots.len(), 2);
        // Oldest lot drained from 10 → 5, newer untouched.
        assert_eq!(lots[0].cost_basis_per_share, dec!(5));
        assert_eq!(lots[0].remaining_quantity, dec!(5));
        assert_eq!(lots[1].cost_basis_per_share, dec!(7));
        assert_eq!(lots[1].remaining_quantity, dec!(10));
    }

    #[test]
    fn sell_spanning_lots_closes_first_and_reduces_second() {
        let lots = reconstruct_fifo(&[
            buy(1, "2024-01-01", dec!(10), dec!(5), None),
            buy(2, "2024-02-01", dec!(10), dec!(7), None),
            sell(3, "2024-03-01", dec!(15)),
        ]);
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].cost_basis_per_share, dec!(7));
        assert_eq!(lots[0].remaining_quantity, dec!(5));
    }

    #[test]
    fn overselling_stops_gracefully() {
        let lots = reconstruct_fifo(&[
            buy(1, "2024-01-01", dec!(5), dec!(5), None),
            sell(2, "2024-02-01", dec!(10)),
        ]);
        assert!(lots.is_empty());
    }

    #[test]
    fn non_trading_transactions_are_ignored() {
        let dividend = LotInput {
            source_transaction_id: Uuid::from_u128(2),
            date: d("2024-02-01"),
            transaction_type: Some("cash".into()),
            quantity: None,
            price: None,
            amount: Some(dec!(12)),
            fees: None,
        };
        let lots = reconstruct_fifo(&[buy(1, "2024-01-01", dec!(10), dec!(5), None), dividend]);
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].remaining_quantity, dec!(10));
    }

    #[test]
    fn unsorted_input_is_handled_by_date() {
        // Same data as the spanning test but shuffled; result must match.
        let lots = reconstruct_fifo(&[
            sell(3, "2024-03-01", dec!(15)),
            buy(2, "2024-02-01", dec!(10), dec!(7), None),
            buy(1, "2024-01-01", dec!(10), dec!(5), None),
        ]);
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].cost_basis_per_share, dec!(7));
        assert_eq!(lots[0].remaining_quantity, dec!(5));
    }
}
