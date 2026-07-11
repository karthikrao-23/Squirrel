//! Row-Level Security: prove tenant isolation is enforced by Postgres itself,
//! not merely by the app's `WHERE user_id = $1`. Each test runs on an isolated
//! database (`#[sqlx::test]` creates it and applies every migration, including
//! `0010_rls`), so nothing here touches a real database.
//!
//! IMPORTANT: RLS is bypassed for SUPERUSER and BYPASSRLS roles, *even with
//! FORCE*. The `#[sqlx::test]` pool connects as the local `taxloss` superuser, so
//! the enforcement checks below run through a purpose-made **non-superuser** role
//! (`restricted_pool`) — mirroring production, where the app's DML-only runtime
//! role is neither a superuser nor `BYPASSRLS`. Connecting the app as a superuser
//! would silently defeat RLS.
#![allow(clippy::explicit_auto_deref)]

use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// A pool connected to the same (temp) database as `admin`, but as a freshly
/// created **non-superuser** role — the only way RLS policies actually bind.
async fn restricted_pool(admin: &PgPool) -> PgPool {
    let role = format!("rls_test_{}", Uuid::new_v4().simple());
    for stmt in [
        format!("CREATE ROLE \"{role}\" LOGIN PASSWORD 'pw' NOSUPERUSER NOBYPASSRLS"),
        format!("GRANT USAGE ON SCHEMA public TO \"{role}\""),
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO \"{role}\""
        ),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO \"{role}\""),
    ] {
        sqlx::query(&stmt).execute(admin).await.unwrap();
    }

    // Reuse the admin URL's host/port + this test's temp database, but connect as
    // the restricted role.
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
    let at = base.rfind('@').unwrap();
    let host_port = &base[at + 1..base[at..].find('/').unwrap() + at];
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(admin)
        .await
        .unwrap();
    PgPool::connect(&format!("postgres://{role}:pw@{host_port}/{db}"))
        .await
        .unwrap()
}

/// Insert one alert owned by `user_id`, inside that user's tenant transaction
/// (so the RLS `WITH CHECK` is satisfied).
async fn seed_alert(pool: &PgPool, user_id: Uuid, title: &str) {
    let mut tx = db::begin_as_user(pool, user_id).await.unwrap();
    db::queries::alerts::create_if_absent(
        &mut tx,
        user_id,
        "approaching_long_term",
        None,
        title,
        "msg",
        json!({}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn rls_isolates_tenants(admin: PgPool) {
    // Users are created via the admin pool (the `users` table has no RLS).
    let a = db::queries::users::create(&admin, "a@test.com", "h")
        .await
        .unwrap();
    let b = db::queries::users::create(&admin, "b@test.com", "h")
        .await
        .unwrap();

    let pool = restricted_pool(&admin).await;
    // Seeding runs as the restricted role, so the RLS WITH CHECK is exercised on
    // the way in too.
    seed_alert(&pool, a.id, "A alert").await;
    seed_alert(&pool, b.id, "B alert").await;

    // Scoped by their own GUC, each user sees exactly their own alert.
    let mut tx = db::begin_as_user(&pool, a.id).await.unwrap();
    let a_alerts = db::queries::alerts::list(&mut tx, a.id, false)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(a_alerts.len(), 1);
    assert_eq!(a_alerts[0].title, "A alert");

    // Cross-tenant READ is blocked: A's GUC is set, but the app-level filter names
    // B. RLS (GUC = A) intersects with the filter (user_id = B) to nothing — so
    // even a query that passed the wrong id cannot leak another tenant's rows.
    let mut tx = db::begin_as_user(&pool, a.id).await.unwrap();
    let leaked = db::queries::alerts::list(&mut tx, b.id, false)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        leaked.is_empty(),
        "A must not read B's alerts even when the app filter names B"
    );

    // FAIL-CLOSED: a tenant query on a raw connection with no `app.user_id` set
    // returns zero rows.
    let mut conn = pool.acquire().await.unwrap();
    let none = db::queries::alerts::list(&mut conn, a.id, false)
        .await
        .unwrap();
    assert!(
        none.is_empty(),
        "a tenant query without app.user_id must fail closed (no rows)"
    );
    drop(conn);

    // Cross-tenant WRITE is blocked: A's GUC is set, but the row is owned by B.
    // The RLS `WITH CHECK` rejects the insert.
    let mut tx = db::begin_as_user(&pool, a.id).await.unwrap();
    let res = db::queries::alerts::create_if_absent(
        &mut tx,
        b.id,
        "approaching_long_term",
        None,
        "evil",
        "msg",
        json!({}),
    )
    .await;
    assert!(
        res.is_err(),
        "A must not insert a row owned by B (RLS WITH CHECK)"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn system_bypass_sees_all_tenants(admin: PgPool) {
    let a = db::queries::users::create(&admin, "a@test.com", "h")
        .await
        .unwrap();
    let b = db::queries::users::create(&admin, "b@test.com", "h")
        .await
        .unwrap();

    let pool = restricted_pool(&admin).await;
    seed_alert(&pool, a.id, "A alert").await;
    seed_alert(&pool, b.id, "B alert").await;

    // Sanity: as the restricted role, a plain connection (no GUC) sees nothing.
    let mut conn = pool.acquire().await.unwrap();
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM alerts")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(visible, 0, "no GUC ⇒ RLS hides every row");
    drop(conn);

    // `begin_system` sets the bypass GUC for trusted cross-tenant work, so it sees
    // every tenant's rows (this is what the Plaid capacity count / webhook use).
    let mut tx = db::begin_system(&pool).await.unwrap();
    let all: i64 = sqlx::query_scalar("SELECT count(*) FROM alerts")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(all, 2, "system bypass should see every tenant's rows");
}
