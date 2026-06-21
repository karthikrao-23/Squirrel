//! `/investments/holdings/get` — current positions plus the security and account
//! metadata they reference.

use serde::{Deserialize, Serialize};

use crate::models::{PlaidAccount, PlaidHolding, PlaidItemMeta, PlaidSecurity};
use crate::{PlaidClient, PlaidError};

#[derive(Serialize)]
struct HoldingsGetReq<'a> {
    access_token: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct HoldingsGetResp {
    pub accounts: Vec<PlaidAccount>,
    pub holdings: Vec<PlaidHolding>,
    pub securities: Vec<PlaidSecurity>,
    pub item: PlaidItemMeta,
}

impl PlaidClient {
    pub async fn get_holdings(&self, access_token: &str) -> Result<HoldingsGetResp, PlaidError> {
        self.post("/investments/holdings/get", HoldingsGetReq { access_token })
            .await
    }
}
