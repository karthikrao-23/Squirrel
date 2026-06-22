//! Tax-aware sell-signal rules (pure). Given a snapshot of open lots plus the
//! user's tax profile, produce alert candidates:
//!
//! - **Approaching long-term:** a lot sitting on a *gain* that's within a few
//!   days of crossing the 1-year boundary — selling now wastes the lower
//!   long-term rate, so we flag the tax saving from waiting.
//! - **Harvestable loss:** a lot at a *loss* worth realizing to offset taxes,
//!   carrying a wash-sale warning when a recent purchase taints it.
//!
//! No DB/HTTP here — the caller supplies inputs and persists the results — so the
//! rules are exhaustively unit-tested.

use crate::tax::{self, Term};
use crate::FilingStatus;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// One open lot's snapshot, the unit the rules evaluate.
#[derive(Debug, Clone)]
pub struct AlertInput {
    pub security_id: Uuid,
    pub ticker: Option<String>,
    pub open_date: NaiveDate,
    pub quantity: Decimal,
    pub cost_basis_per_share: Decimal,
    pub current_price: Decimal,
    /// True if the same security was bought within the wash-sale window.
    pub wash_sale: bool,
}

/// Tunable rule parameters.
#[derive(Debug, Clone, Copy)]
pub struct AlertConfig {
    /// Flag gains this many days (or fewer) from becoming long-term.
    pub approaching_window_days: i64,
    /// Suppress alerts whose estimated tax impact is below this (noise floor).
    pub min_tax_saving: Decimal,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            approaching_window_days: 30,
            min_tax_saving: Decimal::new(50, 0), // $50
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    ApproachingLongTerm,
    HarvestableLoss,
}

impl AlertKind {
    /// Stable string used as the `type` column / dedup key.
    pub fn as_str(self) -> &'static str {
        match self {
            AlertKind::ApproachingLongTerm => "approaching_long_term",
            AlertKind::HarvestableLoss => "harvestable_loss",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertCandidate {
    pub kind: AlertKind,
    pub security_id: Uuid,
    pub ticker: Option<String>,
    pub title: String,
    pub message: String,
    pub estimated_tax_saving: Decimal,
    pub unrealized_gain: Decimal,
    pub days_to_long_term: Option<i64>,
    pub wash_sale_warning: bool,
}

fn ticker_label(ticker: &Option<String>) -> String {
    ticker
        .clone()
        .unwrap_or_else(|| "this security".to_string())
}

/// Evaluate all rules across the given lots.
pub fn evaluate(
    inputs: &[AlertInput],
    status: FilingStatus,
    taxable_income: Decimal,
    as_of: NaiveDate,
    config: AlertConfig,
) -> Vec<AlertCandidate> {
    let mut out = Vec::new();
    for input in inputs {
        let g = tax::lot_gain(
            input.quantity,
            input.cost_basis_per_share,
            input.current_price,
            input.open_date,
            as_of,
        );

        // Rule 1: a short-term *gain* about to become long-term.
        if g.gain > Decimal::ZERO && g.term == Term::ShortTerm {
            let days_held = tax::holding_period_days(input.open_date, as_of);
            let days_to_lt = tax::LONG_TERM_DAYS + 1 - days_held;
            if days_to_lt > 0 && days_to_lt <= config.approaching_window_days {
                let st = tax::estimate_tax(status, taxable_income, Term::ShortTerm, g.gain).total;
                let lt = tax::estimate_tax(status, taxable_income, Term::LongTerm, g.gain).total;
                let saving = st - lt;
                if saving >= config.min_tax_saving {
                    let label = ticker_label(&input.ticker);
                    out.push(AlertCandidate {
                        kind: AlertKind::ApproachingLongTerm,
                        security_id: input.security_id,
                        ticker: input.ticker.clone(),
                        title: format!("{label} becomes long-term in {days_to_lt} days"),
                        message: format!(
                            "Holding {label} {days_to_lt} more day(s) makes the gain long-term — \
                             an estimated ${} tax saving versus selling now.",
                            saving.round_dp(2)
                        ),
                        estimated_tax_saving: saving,
                        unrealized_gain: g.gain,
                        days_to_long_term: Some(days_to_lt),
                        wash_sale_warning: false,
                    });
                }
            }
        }

        // Rule 2: a loss worth harvesting.
        if g.gain < Decimal::ZERO {
            let saving = -tax::estimate_tax(status, taxable_income, g.term, g.gain).total;
            if saving >= config.min_tax_saving {
                let label = ticker_label(&input.ticker);
                let wash = if input.wash_sale {
                    " (wash-sale risk: bought recently)"
                } else {
                    ""
                };
                out.push(AlertCandidate {
                    kind: AlertKind::HarvestableLoss,
                    security_id: input.security_id,
                    ticker: input.ticker.clone(),
                    title: format!("Harvestable loss in {label}"),
                    message: format!(
                        "Selling {label} at a loss could save an estimated ${} in taxes{wash}.",
                        saving.round_dp(2)
                    ),
                    estimated_tax_saving: saving,
                    unrealized_gain: g.gain,
                    days_to_long_term: None,
                    wash_sale_warning: input.wash_sale,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn input(open: &str, basis: Decimal, price: Decimal, wash: bool) -> AlertInput {
        AlertInput {
            security_id: Uuid::from_u128(1),
            ticker: Some("AAPL".into()),
            open_date: d(open),
            quantity: dec!(100),
            cost_basis_per_share: basis,
            current_price: price,
            wash_sale: wash,
        }
    }

    #[test]
    fn flags_gain_approaching_long_term() {
        // Held ~350 days (as of 2024-12-16 from 2024-01-01), big gain, single @ $100k.
        let inputs = [input("2024-01-01", dec!(50), dec!(150), false)];
        let alerts = evaluate(
            &inputs,
            FilingStatus::Single,
            dec!(100000),
            d("2024-12-16"),
            AlertConfig::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::ApproachingLongTerm);
        assert!(alerts[0].days_to_long_term.unwrap() > 0);
        assert!(alerts[0].estimated_tax_saving > dec!(0));
    }

    #[test]
    fn no_approaching_alert_when_far_from_boundary() {
        // Held only ~60 days → not within the 30-day window.
        let inputs = [input("2024-01-01", dec!(50), dec!(150), false)];
        let alerts = evaluate(
            &inputs,
            FilingStatus::Single,
            dec!(100000),
            d("2024-03-01"),
            AlertConfig::default(),
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn no_approaching_alert_once_already_long_term() {
        // Held > 1 year already → it's long-term, nothing to wait for.
        let inputs = [input("2022-01-01", dec!(50), dec!(150), false)];
        let alerts = evaluate(
            &inputs,
            FilingStatus::Single,
            dec!(100000),
            d("2024-06-01"),
            AlertConfig::default(),
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn flags_harvestable_loss_with_wash_sale() {
        let inputs = [input("2024-01-01", dec!(150), dec!(50), true)];
        let alerts = evaluate(
            &inputs,
            FilingStatus::Single,
            dec!(100000),
            d("2024-06-01"),
            AlertConfig::default(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::HarvestableLoss);
        assert!(alerts[0].wash_sale_warning);
        assert!(alerts[0].estimated_tax_saving > dec!(0));
        assert!(alerts[0].unrealized_gain < dec!(0));
    }

    #[test]
    fn suppresses_tiny_impacts_below_threshold() {
        // A 1-share, $0.10 loss is below the $50 noise floor.
        let small = AlertInput {
            security_id: Uuid::from_u128(2),
            ticker: Some("XYZ".into()),
            open_date: d("2024-01-01"),
            quantity: dec!(1),
            cost_basis_per_share: dec!(10.10),
            current_price: dec!(10.00),
            wash_sale: false,
        };
        let alerts = evaluate(
            &[small],
            FilingStatus::Single,
            dec!(100000),
            d("2024-06-01"),
            AlertConfig::default(),
        );
        assert!(alerts.is_empty());
    }
}
