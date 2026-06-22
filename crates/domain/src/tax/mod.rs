//! Tax engine: holding-period classification, per-lot gain math, and federal +
//! NIIT + California tax estimates. Pure functions over `Decimal` — no DB/HTTP —
//! so they're exhaustively unit-tested. **Decision-support, not tax advice.**

pub mod brackets;

use crate::FilingStatus;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

/// Long-term if the holding period exceeds one year (> 365 days). Exactly one
/// year is short-term, matching the IRS "more than one year" rule. (Day-count is
/// a slight approximation across leap years; refine later if needed.)
pub const LONG_TERM_DAYS: i64 = 365;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Term {
    ShortTerm,
    LongTerm,
}

pub fn holding_period_days(open_date: NaiveDate, as_of: NaiveDate) -> i64 {
    (as_of - open_date).num_days()
}

pub fn classify_term(open_date: NaiveDate, as_of: NaiveDate) -> Term {
    if holding_period_days(open_date, as_of) > LONG_TERM_DAYS {
        Term::LongTerm
    } else {
        Term::ShortTerm
    }
}

/// Gain/loss for a position: market value minus cost basis, with its term.
#[derive(Debug, Clone, Serialize)]
pub struct GainBreakdown {
    pub cost_basis: Decimal,
    pub market_value: Decimal,
    pub gain: Decimal,
    pub term: Term,
}

/// Compute the unrealized gain for `quantity` shares with a given per-share cost
/// basis at `current_price`, classifying the term from the open date.
pub fn lot_gain(
    quantity: Decimal,
    cost_basis_per_share: Decimal,
    current_price: Decimal,
    open_date: NaiveDate,
    as_of: NaiveDate,
) -> GainBreakdown {
    let cost_basis = cost_basis_per_share * quantity;
    let market_value = current_price * quantity;
    GainBreakdown {
        cost_basis,
        market_value,
        gain: market_value - cost_basis,
        term: classify_term(open_date, as_of),
    }
}

/// Estimated tax on a gain, broken out by component. A negative `gain` (a loss)
/// yields negative values — i.e. the estimated tax *saving* from realizing it.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TaxEstimate {
    pub federal: Decimal,
    pub niit: Decimal,
    pub state: Decimal,
    pub total: Decimal,
}

/// Estimate the tax (or saving) of realizing `gain`, stacked on the user's
/// existing `taxable_income`.
///
/// - Federal: long-term gains use the 0/15/20% brackets; short-term use ordinary
///   brackets. Computed incrementally so a gain spanning brackets is handled.
/// - NIIT: 3.8% on the portion of a positive gain above the MAGI threshold.
/// - State: California taxes all gains as ordinary income.
pub fn estimate_tax(
    status: FilingStatus,
    taxable_income: Decimal,
    term: Term,
    gain: Decimal,
) -> TaxEstimate {
    let base = taxable_income.max(Decimal::ZERO);
    let top = base + gain;

    let federal = match term {
        Term::LongTerm => brackets::tax_on_range(&brackets::federal_long_term(status), base, top),
        Term::ShortTerm => brackets::tax_on_range(&brackets::federal_ordinary(status), base, top),
    };
    let state = brackets::tax_on_range(&brackets::california(status), base, top);
    let niit = niit_on_gain(status, base, gain);

    TaxEstimate {
        federal,
        niit,
        state,
        total: federal + niit + state,
    }
}

/// Estimate the combined tax of realizing both short- and long-term gains at
/// once (e.g. liquidating a whole portfolio, or a multi-lot sale). Stacks
/// correctly: short-term gains pile onto ordinary income first, long-term gains
/// sit on top of that for the 0/15/20% brackets, California taxes the whole lot
/// as ordinary income, and NIIT is computed once on the combined gain.
pub fn estimate_liquidation(
    status: FilingStatus,
    taxable_income: Decimal,
    short_gain: Decimal,
    long_gain: Decimal,
) -> TaxEstimate {
    let base = taxable_income.max(Decimal::ZERO);
    let after_short = (base + short_gain).max(Decimal::ZERO);
    let total_gain = short_gain + long_gain;

    let federal_short =
        brackets::tax_on_range(&brackets::federal_ordinary(status), base, after_short);
    let federal_long = brackets::tax_on_range(
        &brackets::federal_long_term(status),
        after_short,
        after_short + long_gain,
    );
    let federal = federal_short + federal_long;
    let state = brackets::tax_on_range(&brackets::california(status), base, base + total_gain);
    let niit = niit_on_gain(status, base, total_gain);

    TaxEstimate {
        federal,
        niit,
        state,
        total: federal + niit + state,
    }
}

/// NIIT applies only to positive investment gains, on the amount of the gain that
/// sits above the filing-status MAGI threshold.
fn niit_on_gain(status: FilingStatus, base: Decimal, gain: Decimal) -> Decimal {
    if gain <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let over_threshold = (base + gain - brackets::niit_threshold(status)).max(Decimal::ZERO);
    let taxable = gain.min(over_threshold);
    taxable * brackets::NIIT_RATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn term_boundary_is_more_than_one_year() {
        let open = d("2023-01-01");
        assert_eq!(classify_term(open, d("2023-12-31")), Term::ShortTerm); // 364 days
        assert_eq!(classify_term(open, d("2024-01-01")), Term::ShortTerm); // 365 days exactly
        assert_eq!(classify_term(open, d("2024-01-02")), Term::LongTerm); // 366 days
    }

    #[test]
    fn lot_gain_basic() {
        // 10 shares, basis $5, price $8 → cost 50, value 80, gain 30
        let g = lot_gain(dec!(10), dec!(5), dec!(8), d("2020-01-01"), d("2024-01-01"));
        assert_eq!(g.cost_basis, dec!(50));
        assert_eq!(g.market_value, dec!(80));
        assert_eq!(g.gain, dec!(30));
        assert_eq!(g.term, Term::LongTerm);
    }

    #[test]
    fn short_term_costs_more_than_long_term() {
        // Same $20k gain, $100k base, single filer: ST (ordinary) > LT (15%).
        let st = estimate_tax(
            FilingStatus::Single,
            dec!(100000),
            Term::ShortTerm,
            dec!(20000),
        );
        let lt = estimate_tax(
            FilingStatus::Single,
            dec!(100000),
            Term::LongTerm,
            dec!(20000),
        );
        assert!(st.federal > lt.federal);
        // LT federal here is a clean 15% (gain sits below the 20% threshold).
        assert_eq!(lt.federal, dec!(3000));
    }

    #[test]
    fn niit_applies_above_threshold_only() {
        // Single threshold 200k. Base 190k, 30k gain → 20k over threshold.
        let est = estimate_tax(
            FilingStatus::Single,
            dec!(190000),
            Term::LongTerm,
            dec!(30000),
        );
        assert_eq!(est.niit, dec!(20000) * dec!(0.038)); // 760
                                                         // No NIIT when well under the threshold.
        let low = estimate_tax(
            FilingStatus::Single,
            dec!(50000),
            Term::LongTerm,
            dec!(10000),
        );
        assert_eq!(low.niit, dec!(0));
    }

    #[test]
    fn california_taxes_gains_as_ordinary_income() {
        // CA portion is positive for any gain regardless of term.
        let est = estimate_tax(
            FilingStatus::Single,
            dec!(100000),
            Term::LongTerm,
            dec!(10000),
        );
        assert!(est.state > dec!(0));
        assert_eq!(est.total, est.federal + est.niit + est.state);
    }

    #[test]
    fn liquidation_stacks_short_then_long() {
        // ST gain piles on ordinary income; LT gain stacks above it. The combined
        // estimate equals ST-ordinary on the first slice plus LT-rate on the next.
        let est =
            estimate_liquidation(FilingStatus::Single, dec!(100000), dec!(20000), dec!(20000));
        let st_only = estimate_tax(
            FilingStatus::Single,
            dec!(100000),
            Term::ShortTerm,
            dec!(20000),
        );
        // Federal should exceed the ST-only federal (LT slice adds more).
        assert!(est.federal > st_only.federal);
        assert_eq!(est.total, est.federal + est.niit + est.state);
    }

    #[test]
    fn a_loss_yields_a_negative_estimate() {
        // Realizing a loss reduces tax → negative total (a saving).
        let est = estimate_tax(
            FilingStatus::Single,
            dec!(100000),
            Term::ShortTerm,
            dec!(-5000),
        );
        assert!(est.total < dec!(0));
        assert_eq!(est.niit, dec!(0)); // no NIIT on a loss
    }
}
