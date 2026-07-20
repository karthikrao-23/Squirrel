//! Plaid as a [`BrokerageProvider`] implementation.
//!
//! This is the adapter layer: it calls the concrete Plaid endpoints
//! ([`get_holdings`](PlaidClient::get_holdings),
//! [`get_investments_transactions_page`](PlaidClient::get_investments_transactions_page)),
//! converts Plaid's `f64`-money wire DTOs into the provider-neutral
//! [`Decimal`]-money types, and hides Plaid's pagination behind a single
//! `fetch_transactions` call. The rest of the app depends only on the trait.

use broker::{
    BrokerAccount, BrokerHolding, BrokerSecurity, BrokerTransaction, BrokerageProvider,
    HoldingsSnapshot, ProviderError, TransactionBatch,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::models::{PlaidAccount, PlaidHolding, PlaidInvestmentTransaction, PlaidSecurity};
use crate::transactions::MAX_PAGE_SIZE;
use crate::{PlaidClient, PlaidError};

/// Plaid sends money as JSON floats; convert to `Decimal` here so no float ever
/// crosses the provider boundary.
fn dec(v: f64) -> Decimal {
    Decimal::from_f64_retain(v).unwrap_or_default()
}

fn odec(v: Option<f64>) -> Option<Decimal> {
    v.and_then(Decimal::from_f64_retain)
}

impl From<PlaidError> for ProviderError {
    fn from(e: PlaidError) -> Self {
        match e {
            PlaidError::Http(err) => ProviderError::Transport(err.to_string()),
            PlaidError::Decode(msg) => ProviderError::Decode(msg),
            PlaidError::Api { .. } => ProviderError::Api(e.to_string()),
        }
    }
}

fn map_account(a: &PlaidAccount) -> BrokerAccount {
    BrokerAccount {
        external_id: a.account_id.clone(),
        name: a.name.clone(),
        official_name: a.official_name.clone(),
        account_type: a.account_type.clone(),
        subtype: a.subtype.clone(),
        current_balance: odec(a.balances.as_ref().and_then(|b| b.current)),
        currency: a
            .balances
            .as_ref()
            .and_then(|b| b.iso_currency_code.clone()),
    }
}

fn map_security(s: &PlaidSecurity) -> BrokerSecurity {
    BrokerSecurity {
        external_id: s.security_id.clone(),
        ticker: s.ticker_symbol.clone(),
        name: s.name.clone(),
        cusip: s.cusip.clone(),
        security_type: s.security_type.clone(),
        close_price: odec(s.close_price),
        close_price_as_of: s.close_price_as_of,
        currency: s.iso_currency_code.clone(),
    }
}

fn map_holding(h: &PlaidHolding) -> BrokerHolding {
    BrokerHolding {
        account_external_id: h.account_id.clone(),
        security_external_id: h.security_id.clone(),
        quantity: dec(h.quantity),
        price: odec(h.institution_price),
        price_as_of: h.institution_price_as_of,
        value: odec(h.institution_value),
        cost_basis: odec(h.cost_basis),
        currency: h.iso_currency_code.clone(),
    }
}

fn map_transaction(t: &PlaidInvestmentTransaction) -> BrokerTransaction {
    BrokerTransaction {
        external_id: t.investment_transaction_id.clone(),
        account_external_id: t.account_id.clone(),
        security_external_id: t.security_id.clone(),
        txn_type: t.transaction_type.clone(),
        subtype: t.subtype.clone(),
        quantity: odec(t.quantity),
        price: odec(t.price),
        amount: odec(t.amount),
        fees: odec(t.fees),
        date: t.date,
        name: t.name.clone(),
        currency: t.iso_currency_code.clone(),
    }
}

#[async_trait::async_trait]
impl BrokerageProvider for PlaidClient {
    fn name(&self) -> &'static str {
        "plaid"
    }

    async fn fetch_holdings(&self, access_token: &str) -> Result<HoldingsSnapshot, ProviderError> {
        let resp = self.get_holdings(access_token).await?;
        Ok(HoldingsSnapshot {
            institution_id: resp.item.institution_id,
            accounts: resp.accounts.iter().map(map_account).collect(),
            securities: resp.securities.iter().map(map_security).collect(),
            holdings: resp.holdings.iter().map(map_holding).collect(),
        })
    }

    async fn fetch_transactions(
        &self,
        access_token: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<TransactionBatch, ProviderError> {
        // Plaid paginates; loop until we've fetched every transaction in the
        // window, accumulating into one provider-neutral batch. Securities can be
        // repeated across pages — the caller dedups on upsert.
        let mut batch = TransactionBatch::default();
        let mut offset: u32 = 0;
        loop {
            let page = self
                .get_investments_transactions_page(access_token, start, end, offset, MAX_PAGE_SIZE)
                .await?;

            batch
                .securities
                .extend(page.securities.iter().map(map_security));
            let page_len = page.investment_transactions.len() as u32;
            batch
                .transactions
                .extend(page.investment_transactions.iter().map(map_transaction));

            offset += page_len;
            if page_len == 0 || offset >= page.total_investment_transactions {
                break;
            }
        }
        Ok(batch)
    }
}
