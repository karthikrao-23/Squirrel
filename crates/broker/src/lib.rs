//! Provider-neutral vocabulary for pulling brokerage data.
//!
//! The app's sync layer speaks to a [`BrokerageProvider`] rather than to any one
//! aggregator. Plaid is the first implementation (see `crates/plaid`), but a
//! different aggregator, a broker's own API, or even a CSV importer can be added
//! by implementing this trait — without touching the sync/persistence code.
//!
//! Money is [`Decimal`] and dates are [`NaiveDate`] at this boundary: providers
//! do their own `f64`→`Decimal` conversion, so a float can never leak into our
//! money path. `external_id` on each DTO is the provider's own identifier for
//! that record (Plaid's `account_id`, `security_id`, …).

use chrono::NaiveDate;
use rust_decimal::Decimal;

/// A brokerage account (cash/investment/loan) as reported by a provider.
#[derive(Debug, Clone)]
pub struct BrokerAccount {
    pub external_id: String,
    pub name: String,
    pub official_name: Option<String>,
    pub account_type: Option<String>,
    pub subtype: Option<String>,
    /// Total account balance, when the provider reports one (used for accounts
    /// whose per-security holdings aren't shared).
    pub current_balance: Option<Decimal>,
    pub currency: Option<String>,
}

/// A security (stock, ETF, mutual fund, …) referenced by holdings or transactions.
#[derive(Debug, Clone)]
pub struct BrokerSecurity {
    pub external_id: String,
    pub ticker: Option<String>,
    pub name: Option<String>,
    pub cusip: Option<String>,
    pub security_type: Option<String>,
    pub close_price: Option<Decimal>,
    pub close_price_as_of: Option<NaiveDate>,
    pub currency: Option<String>,
}

/// One position: a quantity of a security held in an account. A provider may
/// report several rows for the same (account, security) — e.g. stock-plan
/// accounts split by cost basis — which the caller aggregates.
#[derive(Debug, Clone)]
pub struct BrokerHolding {
    pub account_external_id: String,
    pub security_external_id: String,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub price_as_of: Option<NaiveDate>,
    pub value: Option<Decimal>,
    pub cost_basis: Option<Decimal>,
    pub currency: Option<String>,
}

/// A single investment transaction (buy, sell, dividend, fee, …).
#[derive(Debug, Clone)]
pub struct BrokerTransaction {
    pub external_id: String,
    pub account_external_id: String,
    /// Not every transaction references a security (e.g. a cash fee).
    pub security_external_id: Option<String>,
    pub txn_type: Option<String>,
    pub subtype: Option<String>,
    pub quantity: Option<Decimal>,
    pub price: Option<Decimal>,
    pub amount: Option<Decimal>,
    pub fees: Option<Decimal>,
    pub date: NaiveDate,
    pub name: Option<String>,
    pub currency: Option<String>,
}

/// One holdings pull: the positions plus the accounts and securities they
/// reference, and the institution the connection belongs to (when known).
#[derive(Debug, Clone, Default)]
pub struct HoldingsSnapshot {
    pub institution_id: Option<String>,
    pub accounts: Vec<BrokerAccount>,
    pub securities: Vec<BrokerSecurity>,
    pub holdings: Vec<BrokerHolding>,
}

/// A full transactions pull over the requested window (already de-paginated),
/// plus the securities those transactions reference (which may not all appear in
/// the holdings snapshot).
#[derive(Debug, Clone, Default)]
pub struct TransactionBatch {
    pub securities: Vec<BrokerSecurity>,
    pub transactions: Vec<BrokerTransaction>,
}

/// A failure while talking to a provider. Deliberately provider-agnostic: each
/// implementation maps its own error into one of these variants.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider transport error: {0}")]
    Transport(String),
    #[error("provider decode error: {0}")]
    Decode(String),
    #[error("provider api error: {0}")]
    Api(String),
}

/// The seam the sync layer depends on: fetch a connection's current holdings and
/// its transactions over a date window. The caller supplies the already-decrypted
/// `access_token` (an opaque per-connection credential) and owns the date window;
/// pagination, wire formats, and `f64`→`Decimal` conversion are the provider's
/// concern.
///
/// Implementations are held as trait objects (`&dyn BrokerageProvider`), so this
/// uses [`async_trait`] rather than native `async fn` in traits (which is not yet
/// `dyn`-safe without hand-written boxing). The one boxed allocation per call is
/// negligible next to the network round-trip.
#[async_trait::async_trait]
pub trait BrokerageProvider: Send + Sync {
    /// Stable identifier for this provider, e.g. `"plaid"`. Logged per sync, and
    /// reserved for keying `(provider, external_id)` if persistence is later
    /// generalized.
    fn name(&self) -> &'static str;

    /// Current positions plus the accounts/securities they reference.
    async fn fetch_holdings(&self, access_token: &str) -> Result<HoldingsSnapshot, ProviderError>;

    /// All investment transactions in `[start, end]` (inclusive of whatever the
    /// provider returns for that range), de-paginated into a single batch.
    async fn fetch_transactions(
        &self,
        access_token: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<TransactionBatch, ProviderError>;
}
