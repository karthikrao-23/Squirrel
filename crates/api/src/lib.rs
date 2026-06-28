//! Squirrel HTTP API.
//!
//! Exposed as a library (not just a binary) so integration tests can build the
//! same router and app state the binary uses and drive it with in-process
//! requests. `main.rs` is a thin wrapper over [`serve`].

pub mod alert_engine;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod email;
pub mod error;
pub mod lots;
pub mod routes;
pub mod state;
pub mod sync;
pub mod webhook_verify;

use std::time::Duration;

use axum::http::{HeaderName, HeaderValue};
use axum::Router;
use config::Config;
use state::AppState;
use tokio_cron_scheduler::{Job, JobScheduler};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// Cap on request body size. argon2 sits on unauthenticated routes, so an
/// unbounded body is a cheap amplification lever; 64 KiB is ample for our JSON.
const MAX_BODY_BYTES: usize = 64 * 1024;
/// Hard per-request timeout, so a slow/stuck request can't pin a connection.
///
/// 60s (not 15s) because the Plaid connect/exchange path runs the **initial
/// portfolio sync inline** — fetch holdings + page ~24 months of transactions +
/// rebuild lots — which legitimately exceeds 15s for a real account. The argon2
/// DoS surface is bounded separately by the body limit + per-IP rate limiting,
/// and Cloud Run's max-instances cap, so a longer request ceiling is safe here.
/// (A future async/background sync would let this drop back down.)
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Content-Security-Policy for the same-origin SPA. The bundle is served from
/// our origin (`'self'`); inline styles are allowed because the UI uses React
/// `style=` attributes. `frame-ancestors 'none'` blocks clickjacking.
const CSP: &str = "default-src 'self'; base-uri 'self'; form-action 'self'; \
     frame-ancestors 'none'; object-src 'none'; img-src 'self' data:; \
     style-src 'self' 'unsafe-inline'; script-src 'self'";
/// 2 years, with subdomains. Only emitted when serving over HTTPS (prod/staging).
const HSTS: &str = "max-age=63072000; includeSubDomains";

/// Build the fully-wired Axum app (routes + middleware) for a given state. Shared
/// by the binary and by integration tests.
///
/// Layer order (outermost first): security headers → trace → timeout →
/// body-limit → CSRF guard → routes. The body limit therefore rejects oversized
/// payloads *before* any handler (and any argon2 hash) runs.
pub fn build_app(state: AppState) -> Router {
    let cookie_secure = state.config.cookie_secure;
    let static_dir = state.config.static_dir.clone();

    let mut routed = routes::router(state.clone());

    // Serve the built SPA from the binary when STATIC_DIR is set (container).
    // Mounted as the fallback — *after* `/api` and `/health` — so unknown SPA
    // client routes fall back to index.html, while unknown `/api/*` paths still
    // hit the JSON-404 catch-all (they never reach here). ServeDir rejects `../`.
    if let Some(dir) = static_dir {
        let index = std::path::Path::new(&dir).join("index.html");
        let serve = ServeDir::new(&dir).fallback(ServeFile::new(index));
        routed = routed.fallback_service(serve);
    }

    let app = routed
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::csrf::csrf_guard,
        ))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(static_header("x-content-type-options", "nosniff"))
        .layer(static_header("referrer-policy", "same-origin"))
        .layer(static_header("content-security-policy", CSP));

    // HSTS only makes sense over HTTPS — emitting it on the dev HTTP origin would
    // wrongly pin the browser to https for localhost.
    if cookie_secure {
        app.layer(static_header("strict-transport-security", HSTS))
    } else {
        app
    }
}

/// A layer that sets a fixed response header (only if the handler didn't set one).
fn static_header(name: &'static str, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::if_not_present(
        HeaderName::from_static(name),
        HeaderValue::from_static(value),
    )
}

/// Load config, connect to Postgres, run migrations, and serve until shutdown.
pub async fn serve() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    init_tracing();

    let config = Config::from_env()?;
    tracing::info!(env = ?config.app_env, plaid_env = ?config.plaid_env, "starting api");

    let pool = db::connect(&config.database_url).await?;
    db::run_migrations(&pool).await?;

    // Cloud Run injects PORT; honor it over BIND_ADDR when present.
    let bind_addr = match config.port {
        Some(port) => format!("0.0.0.0:{port}"),
        None => config.bind_addr.clone(),
    };
    let scheduler_enabled = config.scheduler_enabled;
    let state = AppState::new(pool, config);

    // Background scheduler: periodically refresh prices, evaluate alert rules,
    // and email new alerts. Disabled in prod (Cloud Scheduler hits the internal
    // endpoint instead); kept alive for the process lifetime when enabled.
    let _scheduler = if scheduler_enabled {
        Some(start_scheduler(state.clone()).await?)
    } else {
        tracing::info!("in-process scheduler disabled (driven externally)");
        None
    };

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
            if let Err(e) = alert_engine::run_cycle_all_users(&state).await {
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
