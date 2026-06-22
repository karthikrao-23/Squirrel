//! Thin entry point — all logic lives in the library (`api::serve`) so it can be
//! reused by integration tests.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    api::serve().await
}
