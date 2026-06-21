//! Entry point: load config, connect to Postgres, run migrations, and serve the
//! Axum app.

mod config;
mod error;
mod routes;
mod state;

use config::Config;
use state::AppState;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load `.env` (ignored if absent) before reading config.
    let _ = dotenvy::dotenv();
    init_tracing();

    let config = Config::from_env()?;
    tracing::info!(plaid_env = ?config.plaid_env, "starting api");

    let pool = db::connect(&config.database_url).await?;
    db::run_migrations(&pool).await?;

    let state = AppState::new(pool, config.clone());
    let app = routes::router(state).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();
}
