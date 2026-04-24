#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]

use anyhow::Result;
use executor_mcp::{config, logging};

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config::load()?;
    logging::init(&cfg)?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "executor-mcp starting");
    // Task 2 will add: ExecutorServer::new().serve(stdio()).await?.waiting().await?
    Ok(())
}
