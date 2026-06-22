//! Tax bracket tables and the progressive-tax primitive.
//!
//! Figures are **tax year 2024** (federal + California). Brackets are returned
//! per filing status so they're trivial to update or add new years later — the
//! engine just asks for "the brackets for this status". This is decision-support
//! using marginal/progressive math, **not tax advice**.
//!
//! Simplifications (documented, refine later): California's 1% mental-health
//! surcharge over $1M is omitted (top modeled rate 12.3%); CA Single and Married
//! Filing Separately share one schedule (as CA prescribes).

use crate::FilingStatus;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// One marginal bracket: `rate` applies to income at or above `floor`, up to the
/// next bracket's floor.
#[derive(Debug, Clone, Copy)]
pub struct Bracket {
    pub floor: Decimal,
    pub rate: Decimal,
}

const fn b(floor: Decimal, rate: Decimal) -> Bracket {
    Bracket { floor, rate }
}

/// Federal ordinary-income brackets (used for short-term gains).
pub fn federal_ordinary(status: FilingStatus) -> Vec<Bracket> {
    use FilingStatus::*;
    match status {
        Single | MarriedFilingSeparately => vec![
            b(dec!(0), dec!(0.10)),
            b(dec!(11600), dec!(0.12)),
            b(dec!(47150), dec!(0.22)),
            b(dec!(100525), dec!(0.24)),
            b(dec!(191950), dec!(0.32)),
            b(dec!(243725), dec!(0.35)),
            // MFS technically tops out at a lower 37% floor; close enough for an estimate.
            b(dec!(609350), dec!(0.37)),
        ],
        MarriedFilingJointly => vec![
            b(dec!(0), dec!(0.10)),
            b(dec!(23200), dec!(0.12)),
            b(dec!(94300), dec!(0.22)),
            b(dec!(201050), dec!(0.24)),
            b(dec!(383900), dec!(0.32)),
            b(dec!(487450), dec!(0.35)),
            b(dec!(731200), dec!(0.37)),
        ],
        HeadOfHousehold => vec![
            b(dec!(0), dec!(0.10)),
            b(dec!(16550), dec!(0.12)),
            b(dec!(63100), dec!(0.22)),
            b(dec!(100500), dec!(0.24)),
            b(dec!(191950), dec!(0.32)),
            b(dec!(243700), dec!(0.35)),
            b(dec!(609350), dec!(0.37)),
        ],
    }
}

/// Federal long-term capital-gains brackets (0/15/20%).
pub fn federal_long_term(status: FilingStatus) -> Vec<Bracket> {
    use FilingStatus::*;
    match status {
        Single | MarriedFilingSeparately => vec![
            b(dec!(0), dec!(0.0)),
            b(dec!(47025), dec!(0.15)),
            b(dec!(518900), dec!(0.20)),
        ],
        MarriedFilingJointly => vec![
            b(dec!(0), dec!(0.0)),
            b(dec!(94050), dec!(0.15)),
            b(dec!(583750), dec!(0.20)),
        ],
        HeadOfHousehold => vec![
            b(dec!(0), dec!(0.0)),
            b(dec!(63000), dec!(0.15)),
            b(dec!(551350), dec!(0.20)),
        ],
    }
}

/// California taxes all capital gains as ordinary income (no preferential rate).
pub fn california(status: FilingStatus) -> Vec<Bracket> {
    use FilingStatus::*;
    match status {
        Single | MarriedFilingSeparately => vec![
            b(dec!(0), dec!(0.01)),
            b(dec!(10412), dec!(0.02)),
            b(dec!(24684), dec!(0.04)),
            b(dec!(38959), dec!(0.06)),
            b(dec!(54081), dec!(0.08)),
            b(dec!(68350), dec!(0.093)),
            b(dec!(349137), dec!(0.103)),
            b(dec!(418961), dec!(0.113)),
            b(dec!(698271), dec!(0.123)),
        ],
        MarriedFilingJointly => vec![
            b(dec!(0), dec!(0.01)),
            b(dec!(20824), dec!(0.02)),
            b(dec!(49368), dec!(0.04)),
            b(dec!(77918), dec!(0.06)),
            b(dec!(108162), dec!(0.08)),
            b(dec!(136700), dec!(0.093)),
            b(dec!(698274), dec!(0.103)),
            b(dec!(837922), dec!(0.113)),
            b(dec!(1396542), dec!(0.123)),
        ],
        HeadOfHousehold => vec![
            b(dec!(0), dec!(0.01)),
            b(dec!(20839), dec!(0.02)),
            b(dec!(49371), dec!(0.04)),
            b(dec!(63644), dec!(0.06)),
            b(dec!(78765), dec!(0.08)),
            b(dec!(93037), dec!(0.093)),
            b(dec!(474824), dec!(0.103)),
            b(dec!(569790), dec!(0.113)),
            b(dec!(949649), dec!(0.123)),
        ],
    }
}

/// Net Investment Income Tax rate and the MAGI threshold above which it applies.
pub const NIIT_RATE: Decimal = dec!(0.038);

pub fn niit_threshold(status: FilingStatus) -> Decimal {
    use FilingStatus::*;
    match status {
        MarriedFilingJointly => dec!(250000),
        MarriedFilingSeparately => dec!(125000),
        Single | HeadOfHousehold => dec!(200000),
    }
}

/// Progressive tax owed on `income` given marginal `brackets` (sorted ascending
/// by floor). Returns 0 for non-positive income.
pub fn progressive_tax(brackets: &[Bracket], income: Decimal) -> Decimal {
    if income <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let mut tax = Decimal::ZERO;
    for (i, bracket) in brackets.iter().enumerate() {
        if income <= bracket.floor {
            break;
        }
        let next_floor = brackets.get(i + 1).map(|n| n.floor);
        let upper = match next_floor {
            Some(f) => income.min(f),
            None => income,
        };
        tax += (upper - bracket.floor) * bracket.rate;
    }
    tax
}

/// Incremental tax on the slice of income in `[from, to]` — i.e. the tax
/// attributable to stacking `to - from` of income on top of `from`. Handles
/// gains that span brackets, and returns a negative value for a loss (`to < from`).
pub fn tax_on_range(brackets: &[Bracket], from: Decimal, to: Decimal) -> Decimal {
    let from = from.max(Decimal::ZERO);
    let to = to.max(Decimal::ZERO);
    progressive_tax(brackets, to) - progressive_tax(brackets, from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progressive_single_ordinary_50k() {
        // 10%*11,600 + 12%*(47,150-11,600) + 22%*(50,000-47,150)
        // = 1,160 + 4,266 + 627 = 6,053
        let tax = progressive_tax(&federal_ordinary(FilingStatus::Single), dec!(50000));
        assert_eq!(tax, dec!(6053));
    }

    #[test]
    fn zero_and_negative_income_is_zero() {
        let brackets = federal_ordinary(FilingStatus::Single);
        assert_eq!(progressive_tax(&brackets, dec!(0)), dec!(0));
        assert_eq!(progressive_tax(&brackets, dec!(-100)), dec!(0));
    }

    #[test]
    fn long_term_zero_bracket_then_fifteen() {
        // Single LT: 0% to 47,025, then 15%. Stack a 10k gain on 40k base:
        // 0% on 40k..47,025, 15% on 47,025..50,000 = 0.15 * 2,975 = 446.25
        let lt = federal_long_term(FilingStatus::Single);
        assert_eq!(tax_on_range(&lt, dec!(40000), dec!(50000)), dec!(446.25));
    }

    #[test]
    fn range_is_negative_for_a_loss() {
        let ord = federal_ordinary(FilingStatus::Single);
        // Removing 10k of income from a 50k base is a tax saving (negative).
        assert!(tax_on_range(&ord, dec!(50000), dec!(40000)) < dec!(0));
    }
}
