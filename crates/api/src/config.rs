//! Application configuration loaded from environment variables (via a `.env`
//! file in development). Fails fast at startup if required values are missing.

use base64::Engine;
use plaid::PlaidEnv;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub plaid_env: PlaidEnv,
    pub plaid_client_id: String,
    pub plaid_secret: String,
    /// 32-byte AES key for encrypting Plaid access tokens. Optional so the
    /// server still boots without it (M1); M2 handlers error clearly if unset.
    pub token_encryption_key: Option<[u8; 32]>,
    /// Public URL Plaid should POST webhooks to, if configured.
    pub plaid_webhook_url: Option<String>,
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
        let token_encryption_key = parse_encryption_key()?;
        let plaid_webhook_url = non_empty(std::env::var("PLAID_WEBHOOK_URL").ok());

        Ok(Self {
            database_url,
            bind_addr,
            plaid_env,
            plaid_client_id,
            plaid_secret,
            token_encryption_key,
            plaid_webhook_url,
        })
    }
}

/// Decode `TOKEN_ENCRYPTION_KEY` (base64) into exactly 32 bytes. Absent/empty is
/// allowed (returns `None`); present-but-wrong-size is a hard error so we don't
/// silently run with a bad key.
fn parse_encryption_key() -> anyhow::Result<Option<[u8; 32]>> {
    let Some(raw) = non_empty(std::env::var("TOKEN_ENCRYPTION_KEY").ok()) else {
        return Ok(None);
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| anyhow::anyhow!("TOKEN_ENCRYPTION_KEY is not valid base64: {e}"))?;
    let key: [u8; 32] = bytes.try_into().map_err(|v: Vec<u8>| {
        anyhow::anyhow!(
            "TOKEN_ENCRYPTION_KEY must decode to 32 bytes, got {} (generate: openssl rand -base64 32)",
            v.len()
        )
    })?;
    Ok(Some(key))
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("missing required env var: {key}"))
}

/// Treat empty/whitespace strings as absent.
fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
