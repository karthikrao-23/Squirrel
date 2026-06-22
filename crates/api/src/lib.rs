//! TaxLossApp HTTP API.
//!
//! Exposed as a library (not just a binary) so integration tests can build the
//! same router and app state the binary uses and drive it with in-process
//! requests. `main.rs` is a thin wrapper over [`serve`].

pub mod alert_engine;
pub mod config;
pub mod crypto;
pub mod email;
pub mod error;
pub mod lots;
pub mod routes;
pub mod state;
pub mod sync;

use axum::Router;
use config::Config;
use state::AppState;
use tokio_cron_scheduler::{Job, JobScheduler};
use tower_http::trace::TraceLayer;

/// Build the fully-wired Axum app (routes + middleware) for a given state. Shared
/// by the binary and by integration tests.
pub fn build_app(state: AppState) -> Router {
    routes::router(state).layer(TraceLayer::new_for_http())
}

/// Load config, connect to Postgres, run migrations, and serve until shutdown.
pub async fn serve() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    init_tracing();

    let config = Config::from_env()?;
    tracing::info!(plaid_env = ?config.plaid_env, "starting api");

    let pool = db::connect(&config.database_url).await?;
    db::run_migrations(&pool).await?;

    let bind_addr = config.bind_addr.clone();
    let state = AppState::new(pool, config);

    // Background scheduler: periodically refresh prices, evaluate alert rules,
    // and email any new alerts. Kept alive for the process lifetime.
    let _scheduler = start_scheduler(state.clone()).await?;

    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!(addr = %bind_addr, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Start the cron job that runs the alert cycle on `config.alert_cron`.
async fn start_scheduler(state: AppState) -> anyhow::Result<JobScheduler> {
    let scheduler = JobScheduler::new().await?;
    let cron = state.config.alert_cron.clone();
    let job = Job::new_async(cron.as_str(), move |_uuid, _lock| {
        let state = state.clone();
        Box::pin(async move {
            if let Err(e) = alert_engine::run_cycle(&state).await {
                tracing::error!(error = %e, "scheduled alert cycle failed");
            }
        })
    })?;
    scheduler.add(job).await?;
    scheduler.start().await?;
    tracing::info!(cron = %cron, "alert scheduler started");
    Ok(scheduler)
}

/// Initialize tracing from `RUST_LOG` (defaults to `info`). Safe to call once.
pub fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
}
