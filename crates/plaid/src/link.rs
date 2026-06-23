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

impl PlaidClient {
    /// `/link/token/create` — the token the frontend hands to Plaid Link.
    pub async fn create_link_token(
        &self,
        client_user_id: &str,
        webhook: Option<&str>,
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
