//! Account classification: taxable vs retirement.
//!
//! Retirement accounts (IRA / Roth / 401k / …) are tax-advantaged, so the tax
//! framing (harvesting, ST/LT, "tax if sold") doesn't apply — they get a
//! performance view instead. We derive the kind from Plaid's account `subtype`;
//! it's not stored, just computed.

/// Whether an account is taxable or a tax-advantaged retirement account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    Taxable,
    Retirement,
}

/// Plaid investment `subtype` values that denote a retirement account. Matched
/// case-insensitively; a subtype that *contains* one of these also counts (e.g.
/// "roth ira", "sep ira"), so the list is the set of distinctive tokens.
const RETIREMENT_SUBTYPES: &[&str] = &[
    "ira",
    "roth",
    "401k",
    "401a",
    "403b",
    "457b",
    "sep",
    "simple ira",
    "rollover",
    "pension",
    "keogh",
    "tsp",
    "thrift savings plan",
    "retirement",
];

impl AccountKind {
    /// Classify from a Plaid account `subtype` (None or unknown → taxable).
    pub fn from_subtype(subtype: Option<&str>) -> Self {
        let s = subtype.unwrap_or_default().to_ascii_lowercase();
        let s = s.trim();
        if !s.is_empty() && RETIREMENT_SUBTYPES.iter().any(|r| s == *r || s.contains(r)) {
            AccountKind::Retirement
        } else {
            AccountKind::Taxable
        }
    }

    pub fn is_retirement(self) -> bool {
        matches!(self, AccountKind::Retirement)
    }

    /// Stable lowercase tag used in APIs / snapshot scope.
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Taxable => "taxable",
            AccountKind::Retirement => "retirement",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AccountKind::*;
    use super::*;

    #[test]
    fn retirement_subtypes_classify_as_retirement() {
        for s in [
            "ira",
            "roth",
            "roth ira",
            "401k",
            "roth 401k",
            "403b",
            "457b",
            "sep ira",
            "rollover ira",
            "pension",
            "tsp",
            "IRA",      // case-insensitive
            "Roth IRA", // mixed case + contains
        ] {
            assert_eq!(
                AccountKind::from_subtype(Some(s)),
                Retirement,
                "{s:?} should be retirement"
            );
        }
    }

    #[test]
    fn taxable_and_unknown_classify_as_taxable() {
        for s in [
            "brokerage",
            "stock plan",
            "cash management",
            "mutual fund",
            "",
            "checking",
        ] {
            assert_eq!(
                AccountKind::from_subtype(Some(s)),
                Taxable,
                "{s:?} should be taxable"
            );
        }
        assert_eq!(AccountKind::from_subtype(None), Taxable);
    }

    #[test]
    fn helpers() {
        assert!(Retirement.is_retirement());
        assert!(!Taxable.is_retirement());
        assert_eq!(Retirement.as_str(), "retirement");
        assert_eq!(Taxable.as_str(), "taxable");
    }
}
