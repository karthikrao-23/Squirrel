//! Database access layer: connection pool, migrations, and typed models.
//! Uses SQLx with compile-time-checked queries against PostgreSQL.

use log::LevelFilter;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::{ConnectOptions, Postgres, Transaction};
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

pub mod models;
pub mod queries;

/// Begin a transaction scoped to `user_id` for Postgres Row-Level Security.
///
/// Sets the `app.user_id` GUC **transaction-locally** (via `set_config(_, _, true)`,
/// which — unlike `SET LOCAL` — accepts a bind parameter, so the id can never be
/// interpolated). Every RLS policy compares `user_id` against this GUC, so all
/// tenant queries run on the returned transaction see only that user's rows, and
/// the setting reverts when the tx ends — a pooled connection never carries one
/// tenant's identity into the next request.
///
/// Callers MUST run their tenant queries on this transaction and `commit()` it
/// (drop = rollback). A tenant query run without this (missing GUC) matches no
/// rows: RLS fails **closed**.
pub async fn begin_as_user(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Transaction<'static, Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Begin a transaction that **bypasses** RLS, for trusted, genuinely cross-tenant
/// system work — e.g. the Plaid per-app capacity count that spans all users, or
/// the webhook path that arrives without an authenticated user. The bypass GUC is
/// only ever set here, from server-controlled code, never from user input.
pub async fn begin_system(pool: &PgPool) -> sqlx::Result<Transaction<'static, Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.rls_bypass', 'on', true)")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Create a connection pool to PostgreSQL.
///
/// The pool is cloneable and cheap to share (it's an `Arc` internally), so the
/// Axum app holds one and hands clones to each request handler.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    // Clamp statement logging to WARN so `RUST_LOG=debug` can't dump bound
    // params — which include password hashes and session/Plaid tokens — into the
    // logs. sqlx logs successful statements at DEBUG by default.
    let connect_options = PgConnectOptions::from_str(database_url)?
        .log_statements(LevelFilter::Warn)
        .log_slow_statements(LevelFilter::Warn, Duration::from_secs(1));

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(connect_options)
        .await?;
    Ok(pool)
}

/// Run all pending migrations embedded from the workspace `migrations/` dir.
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    tracing::info!("migrations applied");
    Ok(())
}

/// Provision (or reconcile) the least-privilege **runtime** role used by the
/// Cloud Run service, and grant it DML-only access to the schema this pool's
/// owner just migrated.
///
/// Must be run on a pool connected as the schema **owner** (the migration role):
/// the runtime role is created as a *native* Postgres role — deliberately NOT via
/// the Cloud SQL admin API, so it is **not** a member of `cloudsqlsuperuser` — and
/// is granted only `SELECT/INSERT/UPDATE/DELETE` (plus sequence usage). It owns
/// nothing, so it cannot `CREATE`/`ALTER`/`DROP`. `ALTER DEFAULT PRIVILEGES` makes
/// future owner-created tables inherit the same DML grants.
///
/// Idempotent: safe to re-run on every migration. The role name and password come
/// from trusted deploy config (env/Secret Manager), not user input; the name is
/// still validated as a plain SQL identifier and the password is single-quote
/// escaped before interpolation (neither can be a bind parameter in DDL).
pub async fn provision_runtime_role(
    pool: &PgPool,
    role: &str,
    password: &str,
) -> anyhow::Result<()> {
    validate_ident(role)?;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await?;
    validate_ident(&db)?;
    let pw = password.replace('\'', "''");

    // Create the login role if absent, then (re)sync its password.
    let create = format!(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{role}') THEN \
             CREATE ROLE \"{role}\" LOGIN; \
           END IF; \
         END $$;"
    );
    sqlx::query(&create).execute(pool).await?;
    sqlx::query(&format!("ALTER ROLE \"{role}\" WITH LOGIN PASSWORD '{pw}'"))
        .execute(pool)
        .await?;

    // DML-only grants on the current schema + sequences, plus defaults for any
    // tables future migrations (run as this owner) create.
    let grants = [
        format!("GRANT CONNECT ON DATABASE \"{db}\" TO \"{role}\""),
        format!("GRANT USAGE ON SCHEMA public TO \"{role}\""),
        format!("GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO \"{role}\""),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO \"{role}\""),
        format!("ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"{role}\""),
        format!("ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO \"{role}\""),
    ];
    for stmt in grants {
        sqlx::query(&stmt).execute(pool).await?;
    }
    tracing::info!(role, "runtime role provisioned with DML-only privileges");
    Ok(())
}

/// Reject anything that isn't a conservative, unquoted-safe SQL identifier
/// (lower/upper alnum + underscore, not starting with a digit, ≤63 bytes). Used
/// for values that can't be bound (role/database names in DDL).
fn validate_ident(s: &str) -> anyhow::Result<()> {
    let ok = !s.is_empty()
        && s.len() <= 63
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(())
    } else {
        anyhow::bail!("unsafe SQL identifier: {s:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::validate_ident;

    #[test]
    fn accepts_plain_identifiers() {
        for ok in ["squirrel_app", "sq_runtime", "DB_user1", "a"] {
            assert!(validate_ident(ok).is_ok(), "{ok} should be allowed");
        }
    }

    #[test]
    fn rejects_injection_and_oddities() {
        for bad in [
            "",
            "1leading_digit",
            "has space",
            "evil; DROP TABLE users",
            "quote\"name",
            "x'y",
            &"a".repeat(64),
        ] {
            assert!(validate_ident(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
