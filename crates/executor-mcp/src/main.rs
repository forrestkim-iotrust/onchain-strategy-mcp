#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]

use anyhow::Result;
use executor_mcp::{ExecutorServer, config, logging};
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config::load()?;
    logging::init(&cfg)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        state_path = cfg.state.path.as_str(),
        "executor-mcp starting"
    );
    let service = ExecutorServer::new(&cfg.state)?.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
