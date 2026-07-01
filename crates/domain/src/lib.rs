//! Pure domain logic for the investment tracker: tax computations, cost-basis
//! lot reconstruction, and alert rules. This crate has no database or HTTP
//! dependencies so it can be unit-tested in isolation.
//!
//! Modules are filled in across milestones:
//! - `tax`   (M4): short/long-term classification and federal tax estimates
//! - `lots`  (M3): FIFO cost-basis lot reconstruction from transactions
//! - `alerts`(M5): "good time to sell" and tax-loss-harvest rules

pub mod accounts;
pub mod alerts;
pub mod lots;
pub mod performance;
pub mod tax;

/// Filing status drives which federal bracket table applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilingStatus {
    Single,
    MarriedFilingJointly,
    MarriedFilingSeparately,
    HeadOfHousehold,
}

impl FilingStatus {
    /// Parse the value stored in the DB (snake_case), defaulting to `Single` for
    /// anything unrecognized.
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "married_filing_jointly" => FilingStatus::MarriedFilingJointly,
            "married_filing_separately" => FilingStatus::MarriedFilingSeparately,
            "head_of_household" => FilingStatus::HeadOfHousehold,
            _ => FilingStatus::Single,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filing_status_serializes_to_snake_case() {
        let json = serde_json::to_string(&FilingStatus::MarriedFilingJointly).unwrap();
        assert_eq!(json, "\"married_filing_jointly\"");
    }
}
