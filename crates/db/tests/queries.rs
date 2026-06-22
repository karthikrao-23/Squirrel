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
