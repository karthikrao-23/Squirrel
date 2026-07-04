//! Account classification: taxable, retirement, or debt.
//!
//! Retirement accounts (IRA / Roth / 401k / …) are tax-advantaged, so the tax
//! framing (harvesting, ST/LT, "tax if sold") doesn't apply — they get a
//! performance view instead. **Debt** accounts (a loan, margin, or credit line)
//! are liabilities: their balance is excluded from portfolio value and the
//! investment views entirely. By default we derive the kind from Plaid's account
//! `subtype` ([`AccountKind::from_subtype`], which only ever yields taxable or
//! retirement); a user can override a misclassified account — including marking
//! one as debt — and [`AccountKind::resolve`] applies that override on top.

/// How an account is treated: a taxable brokerage, a tax-advantaged retirement
/// account, or a debt/liability excluded from portfolio value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    Taxable,
    Retirement,
    Debt,
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

    /// Parse a stored override tag back into a kind. `Some("taxable")` /
    /// `Some("retirement")` / `Some("debt")` pin the kind; anything else
    /// (including `None`) means "no override — classify automatically".
    pub fn from_override(over: Option<&str>) -> Option<Self> {
        match over {
            Some("taxable") => Some(AccountKind::Taxable),
            Some("retirement") => Some(AccountKind::Retirement),
            Some("debt") => Some(AccountKind::Debt),
            _ => None,
        }
    }

    /// The effective kind for an account: an explicit user override wins;
    /// otherwise fall back to deriving it from Plaid's `subtype`. (`from_subtype`
    /// never yields `Debt` — that's a deliberate manual mark only.)
    pub fn resolve(subtype: Option<&str>, over: Option<&str>) -> Self {
        Self::from_override(over).unwrap_or_else(|| Self::from_subtype(subtype))
    }

    pub fn is_retirement(self) -> bool {
        matches!(self, AccountKind::Retirement)
    }

    /// A liability excluded from portfolio value and the investment views.
    pub fn is_debt(self) -> bool {
        matches!(self, AccountKind::Debt)
    }

    /// Stable lowercase tag used in APIs / snapshot scope.
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Taxable => "taxable",
            AccountKind::Retirement => "retirement",
            AccountKind::Debt => "debt",
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
    fn override_wins_over_subtype() {
        // A retirement subtype that the user forced back to taxable, and a
        // taxable subtype the user promoted to retirement (the misclassified case).
        assert_eq!(
            AccountKind::resolve(Some("roth ira"), Some("taxable")),
            Taxable
        );
        assert_eq!(
            AccountKind::resolve(Some("brokerage"), Some("retirement")),
            Retirement
        );
    }

    #[test]
    fn resolve_falls_back_to_subtype_without_override() {
        // No override (None) or an unrecognized tag → derive from the subtype.
        assert_eq!(AccountKind::resolve(Some("brokerage"), None), Taxable);
        assert_eq!(AccountKind::resolve(Some("roth ira"), None), Retirement);
        assert_eq!(
            AccountKind::resolve(Some("brokerage"), Some("bogus")),
            Taxable
        );
    }

    #[test]
    fn from_override_parses_known_tags_only() {
        assert_eq!(AccountKind::from_override(Some("taxable")), Some(Taxable));
        assert_eq!(
            AccountKind::from_override(Some("retirement")),
            Some(Retirement)
        );
        assert_eq!(AccountKind::from_override(Some("debt")), Some(Debt));
        assert_eq!(AccountKind::from_override(Some("Retirement")), None); // exact tag only
        assert_eq!(AccountKind::from_override(None), None);
    }

    #[test]
    fn debt_is_override_only_never_derived() {
        // Marked debt regardless of subtype …
        assert_eq!(AccountKind::resolve(Some("brokerage"), Some("debt")), Debt);
        assert!(Debt.is_debt());
        assert!(!Debt.is_retirement());
        assert_eq!(Debt.as_str(), "debt");
        // … but never derived from a subtype alone.
        for s in ["loan", "mortgage", "credit card", "brokerage", "roth ira"] {
            assert_ne!(AccountKind::from_subtype(Some(s)), Debt, "{s:?}");
        }
    }

    #[test]
    fn helpers() {
        assert!(Retirement.is_retirement());
        assert!(!Taxable.is_retirement());
        assert_eq!(Retirement.as_str(), "retirement");
        assert_eq!(Taxable.as_str(), "taxable");
    }
}
