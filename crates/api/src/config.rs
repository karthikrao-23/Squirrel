//! Application configuration loaded from environment variables (via a `.env`
//! file in development). Fails fast at startup if required values are missing.

use plaid::PlaidEnv;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub plaid_env: PlaidEnv,
    pub plaid_client_id: String,
    pub plaid_secret: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = required("DATABASE_URL")?;
        let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let plaid_env =
            PlaidEnv::from_str_or_sandbox(&std::env::var("PLAID_ENV").unwrap_or_default());
        // Plaid creds are optional at M1 so the server still boots without them;
        // M2 endpoints will error clearly if they're unset.
        let plaid_client_id = std::env::var("PLAID_CLIENT_ID").unwrap_or_default();
        let plaid_secret = std::env::var("PLAID_SECRET").unwrap_or_default();

        Ok(Self {
            database_url,
            bind_addr,
            plaid_env,
            plaid_client_id,
            plaid_secret,
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("missing required env var: {key}"))
}
