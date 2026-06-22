//! Application configuration loaded from environment variables (via a `.env`
//! file in development). Fails fast at startup if required values are missing.

use base64::Engine;
use plaid::PlaidEnv;
use rust_decimal::Decimal;

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
    /// SMTP settings for emailing alerts; `None` disables email (in-app only).
    pub smtp: Option<SmtpConfig>,
    /// Cron schedule for the alert job (6-field: sec min hour dom mon dow).
    pub alert_cron: String,
    /// Suppress alerts whose estimated tax impact is below this (noise floor).
    pub alert_min_tax_saving: Decimal,
    /// Flag gains within this many days of becoming long-term.
    pub alert_approaching_window_days: i64,
}

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub to: String,
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
        let smtp = parse_smtp();
        let alert_cron = non_empty(std::env::var("ALERT_CRON").ok())
            .unwrap_or_else(|| "0 0 * * * *".to_string());
        let alert_min_tax_saving = non_empty(std::env::var("ALERT_MIN_SAVING").ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| Decimal::new(50, 0));
        let alert_approaching_window_days =
            non_empty(std::env::var("ALERT_APPROACHING_WINDOW_DAYS").ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(30);

        Ok(Self {
            database_url,
            bind_addr,
            plaid_env,
            plaid_client_id,
            plaid_secret,
            token_encryption_key,
            plaid_webhook_url,
            smtp,
            alert_cron,
            alert_min_tax_saving,
            alert_approaching_window_days,
        })
    }
}

/// Build SMTP config only when a host *and* a recipient are present — otherwise
/// email is disabled and alerts stay in-app only.
fn parse_smtp() -> Option<SmtpConfig> {
    let host = non_empty(std::env::var("SMTP_HOST").ok())?;
    let to = non_empty(std::env::var("ALERT_EMAIL_TO").ok())?;
    let port = std::env::var("SMTP_PORT")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(587);
    let from = non_empty(std::env::var("SMTP_FROM").ok())
        .unwrap_or_else(|| "alerts@taxlossapp.local".to_string());
    Some(SmtpConfig {
        host,
        port,
        username: non_empty(std::env::var("SMTP_USERNAME").ok()),
        password: non_empty(std::env::var("SMTP_PASSWORD").ok()),
        from,
        to,
    })
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
