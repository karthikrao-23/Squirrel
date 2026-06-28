//! Application configuration loaded from environment variables (via a `.env`
//! file in development). Fails fast at startup if required values are missing.

use base64::Engine;
use plaid::PlaidEnv;
use rust_decimal::Decimal;

/// Deployment environment — the single source of truth for security posture.
/// `cookie_secure`, the prod startup guard, the sandbox-connect gate, and the
/// scheduler default all derive from this (never from `PLAID_ENV`), so a Plaid
/// config change can't accidentally move the security posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Staging,
    Production,
}

impl AppEnv {
    /// Strict parse — an unrecognized value is a hard boot error. Unset is
    /// handled by the caller (defaults to development for local convenience).
    fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "development" => Ok(AppEnv::Development),
            "staging" => Ok(AppEnv::Staging),
            "production" => Ok(AppEnv::Production),
            other => Err(anyhow::anyhow!(
                "invalid APP_ENV '{other}' (expected development|staging|production)"
            )),
        }
    }

    pub fn is_development(self) -> bool {
        matches!(self, AppEnv::Development)
    }
}

// NOTE: deliberately no `Debug`/`Serialize` derive — `Config` holds secrets
// (Plaid creds, encryption key, internal token) and must never be logged or
// serialized wholesale.
#[derive(Clone)]
pub struct Config {
    pub app_env: AppEnv,
    pub database_url: String,
    pub bind_addr: String,
    /// Cloud Run injects `PORT`; when set it wins over `bind_addr`.
    pub port: Option<u16>,
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
    /// Whether session cookies get the `Secure` attribute (and the `__Host-`
    /// name prefix). False only in local development (plain HTTP). Derived from
    /// `APP_ENV`: anything other than `development` is treated as secure.
    /// (Part C will promote this to a strict `APP_ENV` enum + startup guard.)
    pub cookie_secure: bool,
    /// The app's own origin (e.g. `https://squirrel.example`), used by the CSRF
    /// guard to reject cross-site mutating requests. `None` in dev skips the
    /// Origin comparison (the required custom header still applies).
    pub app_origin: Option<String>,
    /// Whether the in-process cron scheduler runs. Off in prod (Cloud Scheduler
    /// drives the cycle via the internal endpoint); defaults on in development.
    pub scheduler_enabled: bool,
    /// Bearer token for the internal endpoint (`/api/internal/*`). Fallback when
    /// Cloud Run OIDC isn't used; `None` means the endpoint is closed.
    pub internal_api_token: Option<String>,
    /// Directory of built SPA assets to serve from the binary (set in the
    /// container). `None` in dev, where Vite serves the frontend instead.
    pub static_dir: Option<String>,
    /// Whether the server applies migrations on boot. True by default (local/dev
    /// convenience). Set false in production, where the runtime DB role is
    /// DML-only and migrations are applied separately by the `migrate` job as the
    /// schema owner (see `deploy/migrate.sh`).
    pub run_migrations: bool,
}

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    /// Optional dev-only fallback recipient. Real alert mail goes to each user's
    /// own address (`user.email`); this is never used to send a user's alerts.
    pub to: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        // APP_ENV is the posture source of truth. Unset → development (so local
        // dev needs no config); a *present but unrecognized* value is fatal.
        let app_env = match non_empty(std::env::var("APP_ENV").ok()) {
            Some(v) => AppEnv::parse(&v)?,
            None => AppEnv::Development,
        };
        let database_url = required("DATABASE_URL")?;
        let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let port = match non_empty(std::env::var("PORT").ok()) {
            Some(v) => Some(
                v.parse()
                    .map_err(|_| anyhow::anyhow!("PORT must be a valid port number, got '{v}'"))?,
            ),
            None => None,
        };
        // Strict: a PLAID_ENV typo is fatal, not a silent downgrade. Unset
        // defaults to sandbox (the safe environment).
        let plaid_env = match non_empty(std::env::var("PLAID_ENV").ok()) {
            Some(v) => PlaidEnv::parse(&v).map_err(|e| anyhow::anyhow!(e))?,
            None => PlaidEnv::Sandbox,
        };
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
        // Posture derives from APP_ENV: cookies are Secure (and `__Host-`) for
        // anything other than local development.
        let cookie_secure = !app_env.is_development();
        let app_origin = non_empty(std::env::var("APP_ORIGIN").ok());
        // Scheduler defaults on only in development; an explicit SCHEDULER_ENABLED
        // overrides either way (prod sets it false; Cloud Scheduler drives prod).
        let scheduler_enabled = match non_empty(std::env::var("SCHEDULER_ENABLED").ok()) {
            Some(v) => parse_bool(&v)?,
            None => app_env.is_development(),
        };
        let internal_api_token = non_empty(std::env::var("INTERNAL_API_TOKEN").ok());
        let static_dir = non_empty(std::env::var("STATIC_DIR").ok());
        // Auto-migrate on boot by default; deploy.sh sets RUN_MIGRATIONS=false so
        // the DML-only runtime role never needs DDL.
        let run_migrations = match non_empty(std::env::var("RUN_MIGRATIONS").ok()) {
            Some(v) => parse_bool(&v)?,
            None => true,
        };

        let config = Self {
            app_env,
            database_url,
            bind_addr,
            port,
            plaid_env,
            plaid_client_id,
            plaid_secret,
            token_encryption_key,
            plaid_webhook_url,
            smtp,
            alert_cron,
            alert_min_tax_saving,
            alert_approaching_window_days,
            cookie_secure,
            app_origin,
            scheduler_enabled,
            internal_api_token,
            static_dir,
            run_migrations,
        };
        config.validate_for_prod()?;
        Ok(config)
    }

    /// Fail fast on a production misconfiguration: secrets that *must* be present
    /// and a posture that *must* be secure. Only enforced when `APP_ENV=production`
    /// so dev/test stay frictionless.
    fn validate_for_prod(&self) -> anyhow::Result<()> {
        if self.app_env != AppEnv::Production {
            return Ok(());
        }
        let mut missing = Vec::new();
        if self.token_encryption_key.is_none() {
            missing.push("TOKEN_ENCRYPTION_KEY");
        }
        if self.plaid_client_id.is_empty() {
            missing.push("PLAID_CLIENT_ID");
        }
        if self.plaid_secret.is_empty() {
            missing.push("PLAID_SECRET");
        }
        if self.internal_api_token.is_none() {
            missing.push("INTERNAL_API_TOKEN");
        }
        // Required so the CSRF guard's Origin/Referer check is actually active in
        // production — without it the guard silently falls back to the custom
        // header alone (see `auth::csrf`).
        if self.app_origin.is_none() {
            missing.push("APP_ORIGIN");
        }
        if !missing.is_empty() {
            return Err(anyhow::anyhow!(
                "APP_ENV=production but required secrets are missing: {}",
                missing.join(", ")
            ));
        }
        // Belt-and-suspenders: cookie_secure is derived from app_env, so this
        // can't be false in production — but assert it so the invariant is loud.
        if !self.cookie_secure {
            return Err(anyhow::anyhow!(
                "APP_ENV=production requires secure cookies (cookie_secure must be true)"
            ));
        }
        Ok(())
    }
}

/// Parse a permissive boolean (`true/false/1/0/yes/no/on/off`).
fn parse_bool(s: &str) -> anyhow::Result<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(anyhow::anyhow!("expected a boolean, got '{other}'")),
    }
}

/// Build SMTP config when a host is present — otherwise email is disabled and
/// alerts stay in-app only. The recipient is per-user (`user.email`), so we no
/// longer require a global `ALERT_EMAIL_TO`; it survives only as a dev fallback.
fn parse_smtp() -> Option<SmtpConfig> {
    let host = non_empty(std::env::var("SMTP_HOST").ok())?;
    let to = non_empty(std::env::var("ALERT_EMAIL_TO").ok());
    let port = std::env::var("SMTP_PORT")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(587);
    let from = non_empty(std::env::var("SMTP_FROM").ok())
        .unwrap_or_else(|| "alerts@squirrel.local".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_env_parses_strictly() {
        assert_eq!(AppEnv::parse("production").unwrap(), AppEnv::Production);
        assert_eq!(AppEnv::parse(" Development ").unwrap(), AppEnv::Development);
        assert!(AppEnv::parse("prod").is_err());
        assert!(AppEnv::parse("").is_err());
    }

    /// A minimal production config with everything required present.
    fn prod_config() -> Config {
        Config {
            app_env: AppEnv::Production,
            database_url: "postgres://x".into(),
            bind_addr: "0.0.0.0:8080".into(),
            port: None,
            plaid_env: PlaidEnv::Production,
            plaid_client_id: "id".into(),
            plaid_secret: "secret".into(),
            token_encryption_key: Some([0u8; 32]),
            plaid_webhook_url: None,
            smtp: None,
            alert_cron: "0 0 * * * *".into(),
            alert_min_tax_saving: Decimal::new(50, 0),
            alert_approaching_window_days: 30,
            cookie_secure: true,
            app_origin: Some("https://squirrel.example".into()),
            scheduler_enabled: false,
            internal_api_token: Some("tok".into()),
            static_dir: None,
            run_migrations: false,
        }
    }

    #[test]
    fn prod_guard_passes_when_complete() {
        assert!(prod_config().validate_for_prod().is_ok());
    }

    #[test]
    fn prod_guard_fails_on_missing_secret() {
        let mut c = prod_config();
        c.internal_api_token = None;
        let err = c.validate_for_prod().unwrap_err().to_string();
        assert!(err.contains("INTERNAL_API_TOKEN"), "{err}");
    }

    #[test]
    fn prod_guard_fails_on_missing_app_origin() {
        let mut c = prod_config();
        c.app_origin = None;
        let err = c.validate_for_prod().unwrap_err().to_string();
        assert!(err.contains("APP_ORIGIN"), "{err}");
    }

    #[test]
    fn prod_guard_fails_on_insecure_cookies() {
        let mut c = prod_config();
        c.cookie_secure = false;
        assert!(c.validate_for_prod().is_err());
    }

    #[test]
    fn non_prod_is_not_guarded() {
        let mut c = prod_config();
        c.app_env = AppEnv::Development;
        c.token_encryption_key = None;
        c.internal_api_token = None;
        assert!(c.validate_for_prod().is_ok());
    }
}
