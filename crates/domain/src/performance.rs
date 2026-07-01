//! Return metrics for a set of holdings: money-weighted (IRR/XIRR) and
//! time-weighted (TWR).
//!
//! **IRR** answers "what annual rate makes the money I put in, on the dates I put
//! it in, grow into what I have now?" We build the cash-flow series from the tax
//! lots — each open lot's cost is an outflow on its acquisition date, and the
//! current market value is the terminal inflow — then solve for the rate. It's
//! computable from data we already have.
//!
//! **TWR** removes the effect of contribution *timing* by chaining sub-period
//! returns between value observations. It needs a history of portfolio values,
//! which we only start collecting once daily snapshots accrue — so it returns
//! `None` until there are at least two snapshots.
//!
//! All math is `f64`: a rate of return needs no decimal exactness, and the
//! iteration is far cheaper than `Decimal`.

use chrono::NaiveDate;

/// Solve for the annualized internal rate of return (XIRR) of a dated cash-flow
/// series. Convention: money **out** (invested) is negative, money **in**
/// (received / current value) is positive. Returns `None` for a degenerate
/// series (fewer than two flows, or all one sign — no root).
pub fn xirr(flows: &[(NaiveDate, f64)]) -> Option<f64> {
    if flows.len() < 2 {
        return None;
    }
    if !flows.iter().any(|&(_, a)| a > 0.0) || !flows.iter().any(|&(_, a)| a < 0.0) {
        return None;
    }
    let t0 = flows.iter().map(|&(d, _)| d).min()?;
    let npv = |r: f64| -> f64 {
        flows
            .iter()
            .map(|&(d, a)| {
                let years = (d - t0).num_days() as f64 / 365.0;
                a / (1.0 + r).powf(years)
            })
            .sum()
    };

    // NPV is monotonic in r for a normal invest-then-realize series, so bisect.
    let (mut lo, mut hi) = (-0.999_9_f64, 100.0_f64);
    let (mut f_lo, f_hi) = (npv(lo), npv(hi));
    if f_lo * f_hi > 0.0 {
        return None; // no sign change in range → no solvable rate
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let f_mid = npv(mid);
        if f_mid.abs() < 1e-9 {
            return Some(mid);
        }
        if f_lo * f_mid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// One point in a value history for TWR: the portfolio `value` observed at this
/// date, and the net external `flow` (contributions positive, withdrawals
/// negative) that occurred during the period *ending* at this point. The first
/// point's `flow` is ignored (it's the starting value).
#[derive(Debug, Clone, Copy)]
pub struct TwrPoint {
    pub value: f64,
    pub flow: f64,
}

/// Time-weighted return across an ordered value history. Each sub-period return
/// is `(V_end − flow) / V_start − 1` (external flow removed), chained together.
/// Returns `None` with fewer than two points or a non-positive starting value.
pub fn twr(points: &[TwrPoint]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let mut growth = 1.0_f64;
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start.value <= 0.0 {
            return None;
        }
        let period = (end.value - end.flow) / start.value;
        growth *= period;
    }
    Some(growth - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn xirr_simple_one_year_gain() {
        // Invest 100, worth 110 a year later → ~10%.
        let r = xirr(&[(d(2025, 1, 1), -100.0), (d(2026, 1, 1), 110.0)]).unwrap();
        assert!((r - 0.10).abs() < 1e-3, "got {r}");
    }

    #[test]
    fn xirr_two_year_compounding() {
        // 100 → 121 over two years → ~10% annualized.
        let r = xirr(&[(d(2024, 1, 1), -100.0), (d(2026, 1, 1), 121.0)]).unwrap();
        assert!((r - 0.10).abs() < 2e-3, "got {r}");
    }

    #[test]
    fn xirr_loss_is_negative() {
        let r = xirr(&[(d(2025, 1, 1), -100.0), (d(2026, 1, 1), 90.0)]).unwrap();
        assert!(r < 0.0 && (r + 0.10).abs() < 1e-3, "got {r}");
    }

    #[test]
    fn xirr_multiple_contributions() {
        // Two lots bought a year apart, current value 250.
        let r = xirr(&[
            (d(2024, 1, 1), -100.0),
            (d(2025, 1, 1), -100.0),
            (d(2026, 1, 1), 250.0),
        ])
        .unwrap();
        assert!(r > 0.0, "positive return, got {r}");
    }

    #[test]
    fn xirr_degenerate_returns_none() {
        assert!(xirr(&[]).is_none());
        assert!(xirr(&[(d(2025, 1, 1), -100.0)]).is_none());
        assert!(xirr(&[(d(2025, 1, 1), -100.0), (d(2026, 1, 1), -50.0)]).is_none());
    }

    #[test]
    fn twr_chains_periods_removing_flows() {
        // 100 → 150 (with +30 contributed) → 180. Sub-returns 20% then 20% → 44%.
        let r = twr(&[
            TwrPoint {
                value: 100.0,
                flow: 0.0,
            },
            TwrPoint {
                value: 150.0,
                flow: 30.0,
            },
            TwrPoint {
                value: 180.0,
                flow: 0.0,
            },
        ])
        .unwrap();
        assert!((r - 0.44).abs() < 1e-9, "got {r}");
    }

    #[test]
    fn twr_simple_two_points() {
        let r = twr(&[
            TwrPoint {
                value: 100.0,
                flow: 0.0,
            },
            TwrPoint {
                value: 110.0,
                flow: 0.0,
            },
        ])
        .unwrap();
        assert!((r - 0.10).abs() < 1e-9);
    }

    #[test]
    fn twr_needs_two_points() {
        assert!(twr(&[]).is_none());
        assert!(twr(&[TwrPoint {
            value: 100.0,
            flow: 0.0
        }])
        .is_none());
    }
}
