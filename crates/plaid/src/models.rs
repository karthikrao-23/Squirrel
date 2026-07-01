//! Data-transfer objects mirroring Plaid's JSON shapes. These are the *wire*
//! types — numeric fields are `f64` because Plaid sends JSON numbers; callers
//! convert to `rust_decimal::Decimal` before persisting (never store money as a
//! float). Plaid's `type` fields are reserved words in Rust, so they're renamed.

use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidAccount {
    pub account_id: String,
    pub name: String,
    pub official_name: Option<String>,
    #[serde(rename = "type")]
    pub account_type: Option<String>,
    pub subtype: Option<String>,
    /// Balances Plaid reports for the account. `current` is the total dollar
    /// value — present even for accounts (e.g. Fidelity BrokerageLink) whose
    /// per-security holdings Plaid won't share.
    pub balances: Option<PlaidBalances>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidBalances {
    pub current: Option<f64>,
    pub iso_currency_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidSecurity {
    pub security_id: String,
    pub ticker_symbol: Option<String>,
    pub name: Option<String>,
    pub cusip: Option<String>,
    #[serde(rename = "type")]
    pub security_type: Option<String>,
    pub close_price: Option<f64>,
    pub close_price_as_of: Option<NaiveDate>,
    pub iso_currency_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidHolding {
    pub account_id: String,
    pub security_id: String,
    pub quantity: f64,
    pub institution_price: Option<f64>,
    pub institution_price_as_of: Option<NaiveDate>,
    pub institution_value: Option<f64>,
    pub cost_basis: Option<f64>,
    pub iso_currency_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidInvestmentTransaction {
    pub investment_transaction_id: String,
    pub account_id: String,
    pub security_id: Option<String>,
    #[serde(rename = "type")]
    pub transaction_type: Option<String>,
    pub subtype: Option<String>,
    pub quantity: Option<f64>,
    pub price: Option<f64>,
    pub amount: Option<f64>,
    pub fees: Option<f64>,
    pub date: NaiveDate,
    pub name: Option<String>,
    pub iso_currency_code: Option<String>,
}

/// Subset of the `item` object Plaid echoes back on most responses.
#[derive(Debug, Clone, Deserialize)]
pub struct PlaidItemMeta {
    pub item_id: String,
    pub institution_id: Option<String>,
}
