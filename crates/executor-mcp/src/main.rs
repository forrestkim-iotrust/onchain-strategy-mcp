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
        evm_rpc = cfg.evm.rpc_url.as_str(),
        evm_call_timeout_ms = cfg.evm.call_timeout_ms,
        "executor-mcp starting"
    );
    // Phase 4: build with full Config so [evm] section is honored. Provider
    // itself is lazy — first ctx.evm.* call constructs it.
    let server = ExecutorServer::from_config(&cfg)?;
    // `ExecutorServer: Clone` — inner Arc fields are shared. Pass an owned
    // clone to rmcp's `serve` (which takes Self by value) while the original
    // Arc keeps the dispatcher's Weak upgrade-able.
    let service = (*server).clone().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
