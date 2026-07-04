//! A registry of Plaid apps (client_id/secret pairs).
//!
//! Plaid limits how many live Items a single Plaid app may hold, so to grow past
//! that ceiling the deployment can configure several apps. New connections route
//! to an app with spare capacity; every later call for an Item (sync, remove,
//! webhook verification) must reuse the exact credentials that created it, so
//! items are tagged with their app's `client_id` and resolved back here.

use plaid::{PlaidClient, PlaidEnv};

/// Ordered set of configured Plaid apps. The first is the **primary**: it owns
/// legacy items whose `plaid_client_id` is NULL (they predate multi-app support).
#[derive(Clone)]
pub struct PlaidClients {
    clients: Vec<PlaidClient>,
}

impl PlaidClients {
    /// Build from ordered `(client_id, secret)` pairs. Empty pairs are dropped;
    /// if none remain, a single unconfigured client is kept so [`primary`] always
    /// resolves (handlers gate on [`is_configured`] first).
    ///
    /// [`primary`]: PlaidClients::primary
    /// [`is_configured`]: PlaidClients::is_configured
    pub fn new(env: PlaidEnv, creds: &[(String, String)]) -> Self {
        let mut clients: Vec<PlaidClient> = creds
            .iter()
            .filter(|(id, secret)| !id.is_empty() && !secret.is_empty())
            .map(|(id, secret)| PlaidClient::new(env, id.clone(), secret.clone()))
            .collect();
        if clients.is_empty() {
            clients.push(PlaidClient::new(env, String::new(), String::new()));
        }
        Self { clients }
    }

    /// Any app is fully configured (has credentials).
    pub fn is_configured(&self) -> bool {
        self.clients.iter().any(PlaidClient::is_configured)
    }

    /// The Plaid environment (shared by all apps).
    pub fn env(&self) -> PlaidEnv {
        self.clients[0].env()
    }

    /// The primary app — legacy (NULL `plaid_client_id`) items belong to it, and
    /// it's the default when an item's `client_id` is unknown.
    pub fn primary(&self) -> &PlaidClient {
        &self.clients[0]
    }

    /// Resolve the app that owns an item. `None` (legacy) or an unrecognized
    /// `client_id` falls back to the primary app.
    pub fn for_item(&self, client_id: Option<&str>) -> &PlaidClient {
        match client_id {
            Some(id) => self
                .clients
                .iter()
                .find(|c| c.client_id() == id)
                .unwrap_or_else(|| self.primary()),
            None => self.primary(),
        }
    }

    /// The configured apps, in priority order (primary first). Capacity selection
    /// and webhook verification iterate this.
    pub fn configured(&self) -> impl Iterator<Item = &PlaidClient> {
        self.clients.iter().filter(|c| c.is_configured())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn empty_config_is_unconfigured_but_has_a_primary() {
        let clients = PlaidClients::new(PlaidEnv::Sandbox, &creds(&[]));
        assert!(!clients.is_configured());
        assert_eq!(clients.primary().client_id(), "");
        assert_eq!(clients.configured().count(), 0);
    }

    #[test]
    fn drops_half_set_pairs_and_keeps_order() {
        let clients = PlaidClients::new(
            PlaidEnv::Production,
            &creds(&[("a", "s1"), ("", "s2"), ("c", "s3")]),
        );
        assert!(clients.is_configured());
        let ids: Vec<_> = clients.configured().map(|c| c.client_id()).collect();
        assert_eq!(ids, vec!["a", "c"]); // the empty client_id pair is dropped
        assert_eq!(clients.primary().client_id(), "a");
    }

    #[test]
    fn for_item_resolves_by_client_id_and_falls_back_to_primary() {
        let clients = PlaidClients::new(PlaidEnv::Production, &creds(&[("a", "s1"), ("b", "s2")]));
        assert_eq!(clients.for_item(Some("b")).client_id(), "b");
        // Legacy (None) and unknown ids fall back to the primary app.
        assert_eq!(clients.for_item(None).client_id(), "a");
        assert_eq!(clients.for_item(Some("zzz")).client_id(), "a");
    }
}
