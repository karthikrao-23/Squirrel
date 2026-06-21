//! `/investments/transactions/get` — investment transactions over a date range.
//! Results are paginated (count/offset); callers loop until they've fetched
//! `total_investment_transactions`.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::models::{PlaidInvestmentTransaction, PlaidSecurity};
use crate::{PlaidClient, PlaidError};

/// Plaid caps `count` at 500 per page.
pub const MAX_PAGE_SIZE: u32 = 500;

#[derive(Serialize)]
struct Options {
    count: u32,
    offset: u32,
}

#[derive(Serialize)]
struct InvestmentsTransactionsGetReq<'a> {
    access_token: &'a str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    options: Options,
}

#[derive(Debug, Deserialize)]
pub struct InvestmentsTransactionsGetResp {
    pub securities: Vec<PlaidSecurity>,
    pub investment_transactions: Vec<PlaidInvestmentTransaction>,
    pub total_investment_transactions: u32,
}

impl PlaidClient {
    /// Fetch one page of investment transactions.
    pub async fn get_investments_transactions_page(
        &self,
        access_token: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        offset: u32,
        count: u32,
    ) -> Result<InvestmentsTransactionsGetResp, PlaidError> {
        self.post(
            "/investments/transactions/get",
            InvestmentsTransactionsGetReq {
                access_token,
                start_date,
                end_date,
                options: Options { count, offset },
            },
        )
        .await
    }
}
