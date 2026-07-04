//! Onboarding endpoints: minting a Link token, exchanging the public token for a
//! long-lived access token, and (sandbox only) minting a public token directly
//! so the whole flow can be tested without a frontend.

use serde::{Deserialize, Serialize};

use crate::{PlaidClient, PlaidError};

#[derive(Serialize)]
struct LinkUser<'a> {
    client_user_id: &'a str,
}

#[derive(Serialize)]
struct LinkTokenCreateReq<'a> {
    client_name: &'a str,
    language: &'a str,
    country_codes: [&'a str; 1],
    user: LinkUser<'a>,
    products: [&'a str; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    webhook: Option<&'a str>,
    /// Registered OAuth return URL. Required for OAuth institutions; omitted when
    /// unset so non-OAuth linking is unaffected.
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_uri: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub struct LinkTokenCreateResp {
    pub link_token: String,
    pub expiration: String,
}

#[derive(Serialize)]
struct PublicTokenExchangeReq<'a> {
    public_token: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct PublicTokenExchangeResp {
    pub access_token: String,
    pub item_id: String,
}

#[derive(Serialize)]
struct SandboxPublicTokenCreateReq<'a> {
    institution_id: &'a str,
    initial_products: [&'a str; 1],
}

#[derive(Debug, Deserialize)]
pub struct SandboxPublicTokenCreateResp {
    pub public_token: String,
}

#[derive(Serialize)]
struct ItemRemoveReq<'a> {
    access_token: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct ItemRemoveResp {
    pub request_id: String,
}

impl PlaidClient {
    /// `/link/token/create` — the token the frontend hands to Plaid Link.
    pub async fn create_link_token(
        &self,
        client_user_id: &str,
        webhook: Option<&str>,
        redirect_uri: Option<&str>,
    ) -> Result<LinkTokenCreateResp, PlaidError> {
        self.post(
            "/link/token/create",
            LinkTokenCreateReq {
                client_name: "Squirrel",
                language: "en",
                country_codes: ["US"],
                user: LinkUser { client_user_id },
                products: ["investments"],
                webhook,
                redirect_uri,
            },
        )
        .await
    }

    /// `/item/public_token/exchange` — swap the short-lived public token for the
    /// permanent access token we store (encrypted).
    pub async fn exchange_public_token(
        &self,
        public_token: &str,
    ) -> Result<PublicTokenExchangeResp, PlaidError> {
        self.post(
            "/item/public_token/exchange",
            PublicTokenExchangeReq { public_token },
        )
        .await
    }

    /// `/item/remove` — invalidate the access token and disconnect the item on
    /// Plaid's side (stops future webhooks and billing for it). Used when a user
    /// removes a connection; the local rows are deleted separately.
    pub async fn remove_item(&self, access_token: &str) -> Result<ItemRemoveResp, PlaidError> {
        self.post("/item/remove", ItemRemoveReq { access_token })
            .await
    }

    /// `/sandbox/public_token/create` — sandbox-only shortcut that mints a public
    /// token for a test institution, letting us exercise the full connect flow
    /// from the backend without Plaid Link.
    pub async fn sandbox_public_token_create(
        &self,
        institution_id: &str,
    ) -> Result<SandboxPublicTokenCreateResp, PlaidError> {
        self.post(
            "/sandbox/public_token/create",
            SandboxPublicTokenCreateReq {
                institution_id,
                initial_products: ["investments"],
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(webhook: Option<&'static str>, redirect_uri: Option<&'static str>) -> serde_json::Value {
        serde_json::to_value(LinkTokenCreateReq {
            client_name: "Squirrel",
            language: "en",
            country_codes: ["US"],
            user: LinkUser { client_user_id: "u" },
            products: ["investments"],
            webhook,
            redirect_uri,
        })
        .unwrap()
    }

    #[test]
    fn redirect_uri_is_sent_only_when_set() {
        // Present when configured — required for OAuth banks (E*Trade, …).
        let with = req(None, Some("https://squirrel.example/"));
        assert_eq!(with["redirect_uri"], "https://squirrel.example/");

        // Omitted (not null) when unset, so non-OAuth linking is unaffected.
        let without = req(None, None);
        assert!(without.get("redirect_uri").is_none());
        assert!(without.get("webhook").is_none());
    }
}
