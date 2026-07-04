//! Thin client for the Plaid REST API (Investments product).
//!
//! We call Plaid's HTTP API directly via `reqwest` rather than depend on an
//! unofficial SDK. Every Plaid request is a POST whose JSON body carries the
//! `client_id` + `secret`; endpoint-specific fields are added per call. The
//! endpoint methods live in focused modules (`link`, `holdings`,
//! `transactions`, `webhooks`) and hang off [`PlaidClient`].

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

pub mod holdings;
pub mod link;
pub mod models;
pub mod transactions;
pub mod webhooks;

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

    /// Strictly parse a `PLAID_ENV` value. Unlike a lenient parser, an
    /// unrecognized value is an **error**, not a silent fall-back to sandbox —
    /// so a typo (`prodcution`) can't quietly downgrade which Plaid environment
    /// we talk to. The empty/unset case is handled by the caller (config).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sandbox" => Ok(PlaidEnv::Sandbox),
            "production" => Ok(PlaidEnv::Production),
            other => Err(format!(
                "invalid PLAID_ENV '{other}' (expected 'sandbox' or 'production')"
            )),
        }
    }
}

#[derive(Debug, Error)]
pub enum PlaidError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to decode plaid response: {0}")]
    Decode(String),
    /// A structured error returned by Plaid (non-2xx with an error body).
    #[error("plaid api error [{error_code}]: {error_message}")]
    Api {
        error_type: String,
        error_code: String,
        error_message: String,
    },
}

/// The error body Plaid returns on a non-2xx response.
#[derive(Debug, serde::Deserialize)]
struct PlaidApiErrorBody {
    #[serde(default)]
    error_type: String,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    error_message: String,
}

#[cfg(test)]
mod tests {
    use super::PlaidEnv;

    #[test]
    fn parses_known_envs_and_rejects_typos() {
        assert_eq!(PlaidEnv::parse("sandbox").unwrap(), PlaidEnv::Sandbox);
        assert_eq!(
            PlaidEnv::parse(" PRODUCTION ").unwrap(),
            PlaidEnv::Production
        );
        assert!(PlaidEnv::parse("prodcution").is_err());
        assert!(PlaidEnv::parse("").is_err());
    }
}

/// Wraps an endpoint request with the credentials Plaid expects in every body.
/// `#[serde(flatten)]` merges the endpoint-specific fields in alongside the
/// auth fields, so each endpoint only defines its own parameters.
#[derive(Serialize)]
struct Authed<'a, T> {
    client_id: &'a str,
    secret: &'a str,
    #[serde(flatten)]
    inner: T,
}

/// Holds credentials and a reusable HTTP client (cheap to clone — `reqwest`'s
/// client is internally reference-counted).
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

    /// The Plaid `client_id` these credentials belong to. Used to tag items with
    /// the app that created them so later calls reuse the matching credentials.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// True when no credentials are configured — handlers use this to return a
    /// clear error instead of a confusing 400 from Plaid.
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty() && !self.secret.is_empty()
    }

    /// POST `path` with `req` serialized into the body (plus auth), returning
    /// the deserialized success response. Generic over request/response types so
    /// every endpoint reuses the same transport + error handling.
    pub(crate) async fn post<Req, Res>(&self, path: &str, req: Req) -> Result<Res, PlaidError>
    where
        Req: Serialize,
        Res: DeserializeOwned,
    {
        let url = format!("{}{}", self.env.base_url(), path);
        let body = Authed {
            client_id: &self.client_id,
            secret: &self.secret,
            inner: req,
        };

        let resp = self.http.post(&url).json(&body).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;

        if status.is_success() {
            serde_json::from_slice(&bytes).map_err(|e| PlaidError::Decode(e.to_string()))
        } else {
            // Try to surface Plaid's structured error; fall back to raw text.
            match serde_json::from_slice::<PlaidApiErrorBody>(&bytes) {
                Ok(e) => Err(PlaidError::Api {
                    error_type: e.error_type,
                    error_code: e.error_code,
                    error_message: e.error_message,
                }),
                Err(_) => Err(PlaidError::Api {
                    error_type: "UNKNOWN".into(),
                    error_code: status.as_u16().to_string(),
                    error_message: String::from_utf8_lossy(&bytes).into_owned(),
                }),
            }
        }
    }
}
