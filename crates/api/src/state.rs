//! Shared application state handed to every request handler. Cloning is cheap:
//! the pool is reference-counted and the Plaid client wraps a reusable HTTP
//! client.

use std::sync::Arc;

use plaid::PlaidClient;
use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub plaid: PlaidClient,
    pub config: Config,
    /// A real argon2 hash of a throwaway password, built once at startup. The
    /// login path verifies against it when the email is unknown so that
    /// "no such user" costs the same as "wrong password" (timing parity).
    pub dummy_password_hash: Arc<str>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        let plaid = PlaidClient::new(
            config.plaid_env,
            config.plaid_client_id.clone(),
            config.plaid_secret.clone(),
        );
        let dummy_password_hash = Arc::from(crate::auth::password::dummy_hash());
        Self {
            db,
            plaid,
            config,
            dummy_password_hash,
        }
    }
}
