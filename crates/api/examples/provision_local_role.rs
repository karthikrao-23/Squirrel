//! Dev helper: make local Postgres mirror production so Row-Level Security
//! actually enforces on your machine.
//!
//! The default local role (`taxloss`) is a SUPERUSER, and superusers bypass RLS
//! even with FORCE — so locally the policies are a no-op. This applies all
//! migrations and provisions a **non-superuser, DML-only** runtime role
//! (`squirrel_local`), the same shape as the Cloud Run runtime role. Point the
//! app at that role (with `RUN_MIGRATIONS=false`, since it can't run DDL) and RLS
//! binds locally exactly as it does in prod.
//!
//! Run as the owner (superuser):
//!   DATABASE_URL=postgres://taxloss:taxloss@localhost:5432/taxloss \
//!     cargo run -p api --example provision_local_role

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL to the owner connection");
    // Role/password default to the local dev values `run.sh` uses; overridable so
    // `run.sh` stays the single source of truth.
    let role = std::env::var("LOCAL_DB_ROLE").unwrap_or_else(|_| "squirrel_local".into());
    let password =
        std::env::var("LOCAL_DB_PASSWORD").unwrap_or_else(|_| "squirrel_local_pw".into());

    let pool = db::connect(&url).await?;
    db::run_migrations(&pool).await?;
    db::provision_runtime_role(&pool, &role, &password).await?;
    println!("OK: migrations applied and non-superuser role `{role}` provisioned.");
    Ok(())
}
