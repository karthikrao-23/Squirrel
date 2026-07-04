//! Shared application state handed to every request handler. Cloning is cheap:
//! the pool is reference-counted and the Plaid client wraps a reusable HTTP
//! client.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use sqlx::PgPool;

use crate::config::Config;
use crate::plaid_clients::PlaidClients;
use crate::webhook_verify::WebhookVerifier;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// The configured Plaid apps. New connections shard across them by capacity;
    /// each item is served by the app that created it.
    pub plaid: PlaidClients,
    pub config: Config,
    /// A real argon2 hash of a throwaway password, built once at startup. The
    /// login path verifies against it when the email is unknown so that
    /// "no such user" costs the same as "wrong password" (timing parity).
    pub dummy_password_hash: Arc<str>,
    /// Verifies + caches keys for incoming Plaid webhooks.
    pub webhook_verifier: WebhookVerifier,
    /// Plaid item ids with a sync currently in flight — used to dedupe a burst
    /// of webhooks for the same item into a single sync.
    syncing_items: Arc<Mutex<HashSet<String>>>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        let plaid = PlaidClients::new(config.plaid_env, &config.plaid_credentials());
        let dummy_password_hash = Arc::from(crate::auth::password::dummy_hash());
        Self {
            db,
            plaid,
            config,
            dummy_password_hash,
            webhook_verifier: WebhookVerifier::new(),
            syncing_items: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Try to claim a sync slot for `item_id`. Returns `true` if claimed (caller
    /// must [`release_sync`](Self::release_sync) when done), `false` if a sync for
    /// that item is already in flight.
    pub fn try_claim_sync(&self, item_id: &str) -> bool {
        self.syncing_items
            .lock()
            .expect("sync set lock")
            .insert(item_id.to_string())
    }

    pub fn release_sync(&self, item_id: &str) {
        self.syncing_items
            .lock()
            .expect("sync set lock")
            .remove(item_id);
    }
}
