//! Token Guard CLI binary entry point.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tokenguard_lib::cli::run().await
}
