//! Thin client for the Plaid REST API (Investments product).
//!
//! We call Plaid's HTTP API directly via `reqwest` rather than depend on an
//! unofficial SDK. The full endpoint set (link token, public-token exchange,
//! `/investments/holdings/get`, `/investments/transactions/get`, webhooks) is
//! implemented in M2; this scaffolding establishes the client + config.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaidEnv {
    Sandbox,
    Production,
}

impl PlaidEnv {
    pub fn base_url(self) -> &'static str {
        match self {
            PlaidEnv::Sandbox => "https://sandbox.plaid.com",
            PlaidEnv::Production => "https://production.plaid.com",
        }
    }

    /// Parse from the `PLAID_ENV` env value; defaults to sandbox.
    pub fn from_str_or_sandbox(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "production" => PlaidEnv::Production,
            _ => PlaidEnv::Sandbox,
        }
    }
}

#[derive(Debug, Error)]
pub enum PlaidError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("plaid api error: {0}")]
    Api(String),
}

/// Holds credentials and a reusable HTTP client. `client_id`/`secret` are sent
/// in the body of every Plaid request.
#[derive(Clone)]
pub struct PlaidClient {
    http: reqwest::Client,
    env: PlaidEnv,
    client_id: String,
    secret: String,
}

impl PlaidClient {
    pub fn new(env: PlaidEnv, client_id: String, secret: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            env,
            client_id,
            secret,
        }
    }

    pub fn env(&self) -> PlaidEnv {
        self.env
    }

    // Endpoint methods (link/token/holdings/transactions) are added in M2.
    // Kept private now; fields are read here so they don't warn as unused.
    #[allow(dead_code)]
    fn internal(&self) -> (&reqwest::Client, &str, &str, &str) {
        (&self.http, self.env.base_url(), &self.client_id, &self.secret)
    }
}
