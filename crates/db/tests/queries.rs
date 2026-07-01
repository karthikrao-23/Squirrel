//! Integration tests for the query layer.
//!
//! `#[sqlx::test]` provisions a fresh, isolated database per test and runs the
//! workspace migrations into it, so these exercise the real SQL (upserts, joins,
//! the transactional lot replace) against PostgreSQL. They need a reachable
//! Postgres server via `DATABASE_URL` (docker-compose locally; a service in CI).

use chrono::NaiveDate;
use db::queries;
use rust_decimal_macros::dec;
use sqlx::PgPool;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[sqlx::test(migrations = "../../migrations")]
async fn ensure_default_is_idempotent(pool: PgPool) -> sqlx::Result<()> {
    let first = queries::users::ensure_default(&pool).await?;
    let second = queries::users::ensure_default(&pool).await?;
    assert_eq!(first.id, second.id, "should reuse the single user");
    assert_eq!(first.filing_status, "single");
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn update_profile_round_trips(pool: PgPool) -> sqlx::Result<()> {
    let user = queries::users::ensure_default(&pool).await?;
    let updated =
        queries::users::update_profile(&pool, user.id, "married_filing_jointly", dec!(250000))
            .await?;
    assert_eq!(updated.filing_status, "married_filing_jointly");
    assert_eq!(updated.taxable_income, dec!(250000));
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn holdings_upsert_is_idempotent_per_account_security(pool: PgPool) -> sqlx::Result<()> {
    let user = queries::users::ensure_default(&pool).await?;
    let item =
        queries::plaid_items::upsert(&pool, user.id, "item_1", b"enc", Some("ins_1")).await?;
    let acct = queries::accounts::upsert(
        &pool,
        user.id,
        item.id,
        "acct_1",
        "Brokerage",
        None,
        Some("investment"),
        Some("brokerage"),
    )
    .await?;
    let sec = queries::securities::upsert(
        &pool,
        "sec_1",
        Some("AAPL"),
        Some("Apple Inc"),
        None,
        Some("equity"),
        Some(dec!(190)),
        None,
        Some("USD"),
    )
    .await?;

    queries::holdings::upsert(
        &pool,
        user.id,
        acct,
        sec,
        dec!(10),
        Some(dec!(190)),
        None,
        Some(dec!(1900)),
        Some(dec!(1000)),
        Some("USD"),
    )
    .await?;
    // Re-upsert the same (account, security) with a new quantity.
    queries::holdings::upsert(
        &pool,
        user.id,
        acct,
        sec,
        dec!(20),
        None,
        None,
        None,
        None,
        None,
    )
    .await?;

    let holdings = queries::holdings::list_with_security(&pool, user.id).await?;
    assert_eq!(holdings.len(), 1, "upsert must not duplicate the pair");
    assert_eq!(holdings[0].ticker.as_deref(), Some("AAPL"));
    assert_eq!(holdings[0].quantity, dec!(20));
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn transaction_insert_ignores_duplicates(pool: PgPool) -> sqlx::Result<()> {
    let user = queries::users::ensure_default(&pool).await?;
    let item = queries::plaid_items::upsert(&pool, user.id, "item_1", b"enc", None).await?;
    let acct = queries::accounts::upsert(
        &pool,
        user.id,
        item.id,
        "acct_1",
        "Brokerage",
        None,
        None,
        None,
    )
    .await?;

    let tx = queries::transactions::NewTransaction {
        user_id: user.id,
        account_id: acct,
        security_id: None,
        plaid_investment_transaction_id: "tx_1",
        transaction_type: Some("buy"),
        subtype: None,
        quantity: Some(dec!(1)),
        price: Some(dec!(5)),
        amount: Some(dec!(5)),
        fees: None,
        date: date("2024-01-01"),
        name: None,
        currency: Some("USD"),
    };

    assert!(
        queries::transactions::insert_ignore(&pool, &tx).await?,
        "first insert is new"
    );
    assert!(
        !queries::transactions::insert_ignore(&pool, &tx).await?,
        "duplicate is ignored"
    );

    let list = queries::transactions::list(&pool, user.id, 100).await?;
    assert_eq!(list.len(), 1);
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn tax_lots_replace_is_atomic_and_overwrites(pool: PgPool) -> sqlx::Result<()> {
    let user = queries::users::ensure_default(&pool).await?;
    let item = queries::plaid_items::upsert(&pool, user.id, "item_1", b"enc", None).await?;
    let acct = queries::accounts::upsert(
        &pool,
        user.id,
        item.id,
        "acct_1",
        "Brokerage",
        None,
        None,
        None,
    )
    .await?;
    let sec = queries::securities::upsert(
        &pool,
        "sec_1",
        Some("AAPL"),
        None,
        None,
        None,
        Some(dec!(190)),
        None,
        None,
    )
    .await?;

    let lots = vec![db::queries::tax_lots::NewLot {
        account_id: acct,
        security_id: sec,
        open_date: date("2020-01-01"),
        original_quantity: dec!(10),
        remaining_quantity: dec!(10),
        cost_basis_per_share: dec!(5),
        source_transaction_id: None,
    }];
    let n = queries::tax_lots::replace_for_user(&pool, user.id, &lots).await?;
    assert_eq!(n, 1);

    let priced = queries::tax_lots::list_open_with_price(&pool, user.id).await?;
    assert_eq!(priced.len(), 1);
    assert_eq!(priced[0].close_price, Some(dec!(190)));
    assert_eq!(priced[0].cost_basis_per_share, dec!(5));

    // Replacing with an empty set clears the user's lots.
    queries::tax_lots::replace_for_user(&pool, user.id, &[]).await?;
    assert!(queries::tax_lots::list_with_security(&pool, user.id)
        .await?
        .is_empty());
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_open_with_account_returns_account_info_scoped_by_user(
    pool: PgPool,
) -> sqlx::Result<()> {
    // Securities are shared across users, so seed one and reuse it.
    let sec = queries::securities::upsert(
        &pool,
        "sec_1",
        Some("AAPL"),
        Some("Apple Inc"),
        None,
        Some("equity"),
        Some(dec!(190)),
        None,
        Some("USD"),
    )
    .await?;

    // Seed two distinct users, each with their own account + open lot.
    let alice = queries::users::create(&pool, "alice@example.com", "hash").await?;
    let bob = queries::users::create(&pool, "bob@example.com", "hash").await?;

    let seed_lot = |user_id, acct_name: &'static str, subtype: Option<&'static str>| {
        let pool = pool.clone();
        async move {
            let item =
                queries::plaid_items::upsert(&pool, user_id, "item", b"enc", Some("ins_1")).await?;
            let acct = queries::accounts::upsert(
                &pool,
                user_id,
                item.id,
                "acct",
                acct_name,
                None,
                Some("investment"),
                subtype,
            )
            .await?;
            let lots = vec![db::queries::tax_lots::NewLot {
                account_id: acct,
                security_id: sec,
                open_date: date("2020-01-01"),
                original_quantity: dec!(10),
                remaining_quantity: dec!(10),
                cost_basis_per_share: dec!(5),
                source_transaction_id: None,
            }];
            queries::tax_lots::replace_for_user(&pool, user_id, &lots).await?;
            Ok::<_, sqlx::Error>(())
        }
    };

    seed_lot(alice.id, "Alice Brokerage", Some("brokerage")).await?;
    seed_lot(bob.id, "Bob IRA", Some("ira")).await?;

    let alice_lots = queries::tax_lots::list_open_with_account(&pool, alice.id).await?;
    assert_eq!(alice_lots.len(), 1, "alice sees only her own lot");
    let lot = &alice_lots[0];
    assert_eq!(lot.account_name, "Alice Brokerage");
    assert_eq!(lot.account_subtype.as_deref(), Some("brokerage"));
    assert_eq!(lot.ticker.as_deref(), Some("AAPL"));
    assert_eq!(lot.remaining_quantity, dec!(10));
    assert_eq!(lot.close_price, Some(dec!(190)));

    // Scoping: nothing of Bob's leaks into Alice's result, and vice versa.
    assert!(alice_lots.iter().all(|l| l.account_name != "Bob IRA"));
    let bob_lots = queries::tax_lots::list_open_with_account(&pool, bob.id).await?;
    assert_eq!(bob_lots.len(), 1);
    assert_eq!(bob_lots[0].account_name, "Bob IRA");
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn snapshots_upsert_idempotent_and_history_ordered_per_user(
    pool: PgPool,
) -> sqlx::Result<()> {
    let user = queries::users::ensure_default(&pool).await?;
    // A second user to prove history is scoped per user.
    let other = queries::users::create(&pool, "other@example.com", "hash").await?;

    // Two upserts for the same (user, day, scope): the second must overwrite.
    queries::snapshots::upsert(
        &pool,
        user.id,
        date("2026-01-01"),
        "total",
        dec!(1000),
        dec!(800),
    )
    .await?;
    queries::snapshots::upsert(
        &pool,
        user.id,
        date("2026-01-01"),
        "total",
        dec!(1500),
        dec!(900),
    )
    .await?;
    // An earlier day, inserted after the later one, to prove ordering by as_of.
    queries::snapshots::upsert(
        &pool,
        user.id,
        date("2025-12-31"),
        "total",
        dec!(500),
        dec!(400),
    )
    .await?;
    // A different scope on the same day is a distinct row.
    queries::snapshots::upsert(
        &pool,
        user.id,
        date("2026-01-01"),
        "retirement",
        dec!(200),
        dec!(150),
    )
    .await?;

    // The other user's snapshot must not leak into our history.
    queries::snapshots::upsert(
        &pool,
        other.id,
        date("2026-01-01"),
        "total",
        dec!(9999),
        dec!(9999),
    )
    .await?;

    let history = queries::snapshots::history(&pool, user.id, "total").await?;
    assert_eq!(
        history.len(),
        2,
        "same (user, day) upsert must not duplicate"
    );
    // Ordered by as_of ascending.
    assert_eq!(history[0].as_of, date("2025-12-31"));
    assert_eq!(history[1].as_of, date("2026-01-01"));
    // The second upsert's values won.
    assert_eq!(history[1].market_value, dec!(1500));
    assert_eq!(history[1].cost_basis, dec!(900));

    // Scope filters: the "retirement" row doesn't appear in "total" history.
    let retirement = queries::snapshots::history(&pool, user.id, "retirement").await?;
    assert_eq!(retirement.len(), 1);
    assert_eq!(retirement[0].market_value, dec!(200));

    let other_history = queries::snapshots::history(&pool, other.id, "total").await?;
    assert_eq!(other_history.len(), 1, "history is scoped per user");
    assert_eq!(other_history[0].market_value, dec!(9999));
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn alerts_dedup_until_read(pool: PgPool) -> sqlx::Result<()> {
    let user = queries::users::ensure_default(&pool).await?;
    let payload = serde_json::json!({"saving": 100});

    let first = queries::alerts::create_if_absent(
        &pool,
        user.id,
        "harvestable_loss",
        None,
        "Loss",
        "msg",
        payload.clone(),
    )
    .await?;
    assert!(first.is_some(), "first alert is created");

    let dup = queries::alerts::create_if_absent(
        &pool,
        user.id,
        "harvestable_loss",
        None,
        "Loss",
        "msg",
        payload.clone(),
    )
    .await?;
    assert!(dup.is_none(), "duplicate unread alert is suppressed");

    // Once read, the same condition can alert again.
    assert!(queries::alerts::mark_read(&pool, user.id, first.unwrap().id).await?);
    let again = queries::alerts::create_if_absent(
        &pool,
        user.id,
        "harvestable_loss",
        None,
        "Loss",
        "msg",
        payload,
    )
    .await?;
    assert!(again.is_some());

    assert_eq!(queries::alerts::list(&pool, user.id, false).await?.len(), 2);
    assert_eq!(queries::alerts::list(&pool, user.id, true).await?.len(), 1);
    Ok(())
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_plaid_item_cascades_and_is_user_scoped(pool: PgPool) -> sqlx::Result<()> {
    let sec = queries::securities::upsert(
        &pool,
        "sec_1",
        Some("AAPL"),
        None,
        None,
        None,
        Some(dec!(190)),
        None,
        None,
    )
    .await?;
    let alice = queries::users::create(&pool, "alice@example.com", "hash").await?;
    let bob = queries::users::create(&pool, "bob@example.com", "hash").await?;

    // Alice connected the same institution twice (the duplicate); Bob once.
    let dup =
        queries::plaid_items::upsert(&pool, alice.id, "item_dup", b"enc", Some("ins_1")).await?;
    let keep =
        queries::plaid_items::upsert(&pool, alice.id, "item_keep", b"enc", Some("ins_1")).await?;
    let bob_item =
        queries::plaid_items::upsert(&pool, bob.id, "item_bob", b"enc", Some("ins_1")).await?;

    let dup_acct = queries::accounts::upsert(
        &pool,
        alice.id,
        dup.id,
        "acct_dup",
        "Duplicate IRA",
        None,
        Some("investment"),
        Some("ira"),
    )
    .await?;
    let keep_acct = queries::accounts::upsert(
        &pool,
        alice.id,
        keep.id,
        "acct_keep",
        "Brokerage",
        None,
        Some("investment"),
        Some("brokerage"),
    )
    .await?;

    // A lot under the duplicate connection's account — must cascade away.
    queries::tax_lots::replace_for_user(
        &pool,
        alice.id,
        &[db::queries::tax_lots::NewLot {
            account_id: dup_acct,
            security_id: sec,
            open_date: date("2020-01-01"),
            original_quantity: dec!(10),
            remaining_quantity: dec!(10),
            cost_basis_per_share: dec!(5),
            source_transaction_id: None,
        }],
    )
    .await?;

    // Bob can't delete Alice's connection (user-scoped): no rows affected.
    assert_eq!(
        queries::plaid_items::delete(&pool, bob.id, dup.id).await?,
        0
    );
    assert!(queries::plaid_items::find_by_id(&pool, alice.id, dup.id)
        .await?
        .is_some());

    // Alice removes the duplicate.
    assert_eq!(
        queries::plaid_items::delete(&pool, alice.id, dup.id).await?,
        1
    );

    // Its account and lot cascaded away; the other connection is untouched.
    let accounts = queries::accounts::list(&pool, alice.id).await?;
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, keep_acct);
    assert!(queries::tax_lots::list_with_security(&pool, alice.id)
        .await?
        .is_empty());
    assert!(queries::plaid_items::find_by_id(&pool, alice.id, dup.id)
        .await?
        .is_none());

    // Bob's connection is entirely unaffected.
    assert!(queries::plaid_items::find_by_id(&pool, bob.id, bob_item.id)
        .await?
        .is_some());
    Ok(())
}
