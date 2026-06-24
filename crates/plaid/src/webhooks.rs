//! Webhook payloads. Plaid POSTs these to our `/api/plaid/webhook` endpoint when
//! data changes. For Investments we care about the `HOLDINGS` and
//! `INVESTMENTS_TRANSACTIONS` webhook types, whose `DEFAULT_UPDATE` code means
//! "new/changed data is available — re-sync this item".
//!
//! Every webhook carries a `Plaid-Verification` JWT signed with a key we fetch
//! from `/webhook_verification_key/get`; the verification itself lives in the
//! api crate (it needs the raw request bytes and a cross-request key cache).

use serde::{Deserialize, Serialize};

use crate::{PlaidClient, PlaidError};

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
        matches!(
            self.webhook_type.as_str(),
            "HOLDINGS" | "INVESTMENTS_TRANSACTIONS"
        ) && self.webhook_code == "DEFAULT_UPDATE"
    }
}

/// A JWK (EC P-256 public key) used to verify the `Plaid-Verification` JWT.
/// Only the fields we need for ES256 verification are modeled.
#[derive(Debug, Clone, Deserialize)]
pub struct WebhookVerificationKey {
    pub alg: String,
    pub kty: String,
    pub crv: String,
    pub kid: String,
    /// base64url EC public-key coordinates.
    pub x: String,
    pub y: String,
    /// Unix seconds at which Plaid expired this key (set during rotation).
    #[serde(default)]
    pub expired_at: Option<i64>,
}

#[derive(Serialize)]
struct WebhookVerificationKeyReq<'a> {
    key_id: &'a str,
}

#[derive(Deserialize)]
struct WebhookVerificationKeyResp {
    key: WebhookVerificationKey,
}

impl PlaidClient {
    /// `/webhook_verification_key/get` — fetch the public key (by `kid`) that
    /// signs incoming webhooks. Keys rotate, so callers cache per-`kid` with a TTL.
    pub async fn get_webhook_verification_key(
        &self,
        key_id: &str,
    ) -> Result<WebhookVerificationKey, PlaidError> {
        let resp: WebhookVerificationKeyResp = self
            .post(
                "/webhook_verification_key/get",
                WebhookVerificationKeyReq { key_id },
            )
            .await?;
        Ok(resp.key)
    }
}
