//! Shared application state handed to every request handler. Cloning is cheap:
//! the pool is reference-counted and the Plaid client wraps a reusable HTTP
//! client.

use plaid::PlaidClient;
use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub plaid: PlaidClient,
    pub config: Config,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        let plaid = PlaidClient::new(
            config.plaid_env,
            config.plaid_client_id.clone(),
            config.plaid_secret.clone(),
        );
        Self { db, plaid, config }
    }
}
