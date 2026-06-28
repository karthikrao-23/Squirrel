//! Thin entry point — all logic lives in the library (`api::serve`) so it can be
//! reused by integration tests.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `api migrate` applies migrations (as the schema owner) and provisions the
    // DML-only runtime role, then exits; no args runs the HTTP server.
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => api::migrate().await,
        _ => api::serve().await,
    }
}
