//! Webhook payloads. Plaid POSTs these to our `/api/plaid/webhook` endpoint when
//! data changes. For Investments we care about the `HOLDINGS` and
//! `INVESTMENTS_TRANSACTIONS` webhook types, whose `DEFAULT_UPDATE` code means
//! "new/changed data is available — re-sync this item".

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PlaidWebhook {
    pub webhook_type: String,
    pub webhook_code: String,
    pub item_id: String,
    /// Present on transaction webhooks; how many new txns are available.
    #[serde(default)]
    pub new_investments_transactions: Option<i64>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

impl PlaidWebhook {
    /// True when this webhook means "investment data changed, re-sync".
    pub fn is_investments_update(&self) -> bool {
        matches!(self.webhook_type.as_str(), "HOLDINGS" | "INVESTMENTS_TRANSACTIONS")
            && self.webhook_code == "DEFAULT_UPDATE"
    }
}
