//! Sync-layer integration test driven by a **non-Plaid** provider.
//!
//! This is the payoff of the `BrokerageProvider` abstraction: `sync_item` can be
//! exercised end-to-end — decrypt token → fetch holdings/transactions → persist →
//! rebuild tax lots — against an in-memory `MockProvider`, with no HTTP and no
//! Plaid credentials. `#[sqlx::test]` runs it on an isolated database; the pool
//! connects as the local superuser, which bypasses RLS, so the plain queries here
//! see the rows sync writes.

use async_trait::async_trait;
use broker::{
    BrokerAccount, BrokerHolding, BrokerSecurity, BrokerTransaction, BrokerageProvider,
    HoldingsSnapshot, ProviderError, TransactionBatch,
};
use chrono::{Duration, NaiveDate, Utc};
use rust_decimal_macros::dec;
use sqlx::PgPool;

fn mock_security() -> BrokerSecurity {
    BrokerSecurity {
        external_id: "sec-1".into(),
        ticker: Some("AAPL".into()),
        name: Some("Apple Inc.".into()),
        cusip: None,
        security_type: Some("equity".into()),
        close_price: Some(dec!(5)),
        close_price_as_of: None,
        currency: Some("USD".into()),
    }
}

/// A provider that returns a fixed one-account, one-security, one-buy portfolio.
struct MockProvider;

#[async_trait]
impl BrokerageProvider for MockProvider {
    fn name(&self) -> &'static str {
        "mock"
    }

    async fn fetch_holdings(&self, _access_token: &str) -> Result<HoldingsSnapshot, ProviderError> {
        Ok(HoldingsSnapshot {
            institution_id: Some("ins_mock".into()),
            accounts: vec![BrokerAccount {
                external_id: "acct-1".into(),
                name: "Brokerage".into(),
                official_name: None,
                account_type: Some("investment".into()),
                subtype: Some("brokerage".into()),
                current_balance: Some(dec!(50)),
                currency: Some("USD".into()),
            }],
            securities: vec![mock_security()],
            holdings: vec![BrokerHolding {
                account_external_id: "acct-1".into(),
                security_external_id: "sec-1".into(),
                quantity: dec!(10),
                price: Some(dec!(5)),
                price_as_of: None,
                value: Some(dec!(50)),
                cost_basis: Some(dec!(50)),
                currency: Some("USD".into()),
            }],
        })
    }

    async fn fetch_transactions(
        &self,
        _access_token: &str,
        _start: NaiveDate,
        _end: NaiveDate,
    ) -> Result<TransactionBatch, ProviderError> {
        // A single buy of the 10 shares we hold — reconstructs into one open lot
        // that matches the holding (so no reconciliation trimming/topping-up).
        let date = Utc::now().date_naive() - Duration::days(30);
        Ok(TransactionBatch {
            securities: vec![mock_security()],
            transactions: vec![BrokerTransaction {
                external_id: "txn-1".into(),
                account_external_id: "acct-1".into(),
                security_external_id: Some("sec-1".into()),
                txn_type: Some("buy".into()),
                subtype: None,
                quantity: Some(dec!(10)),
                price: Some(dec!(5)),
                amount: Some(dec!(-50)),
                fees: None,
                date,
                name: Some("BUY AAPL".into()),
                currency: Some("USD".into()),
            }],
        })
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn sync_item_persists_and_rebuilds_lots_via_provider(pool: PgPool) {
    let user = db::queries::users::create(&pool, "sync@test.com", "h")
        .await
        .unwrap();

    // Seed a linked item with an encrypted (mock) access token — the provider
    // ignores the token, but sync still decrypts it, exercising that path.
    let key = [9u8; 32];
    let token = api::crypto::encrypt(&key, b"access-mock").unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let item = db::queries::plaid_items::upsert(&mut conn, user.id, "item-1", &token, None, "")
        .await
        .unwrap();

    let summary = api::sync::sync_item(&mut conn, &MockProvider, &key, &item)
        .await
        .unwrap();
    assert_eq!(summary.accounts, 1);
    assert_eq!(summary.securities, 1);
    assert_eq!(summary.holdings, 1);
    assert_eq!(summary.transactions_inserted, 1);

    // Holdings persisted...
    let positions = db::queries::holdings::positions_for_user(&mut conn, user.id)
        .await
        .unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, dec!(10));

    // ...and the buy reconstructed into exactly one open tax lot.
    let lots = db::queries::tax_lots::list_open_with_price(&mut conn, user.id)
        .await
        .unwrap();
    assert_eq!(
        lots.len(),
        1,
        "the buy should reconstruct into one open lot"
    );
    assert_eq!(lots[0].remaining_quantity, dec!(10));

    // Re-running is idempotent: the same transaction id is not inserted twice.
    let again = api::sync::sync_item(&mut conn, &MockProvider, &key, &item)
        .await
        .unwrap();
    assert_eq!(again.transactions_inserted, 0);
}
