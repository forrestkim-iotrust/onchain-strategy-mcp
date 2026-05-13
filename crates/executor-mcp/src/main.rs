#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]

use anyhow::Result;
use clap::{Parser, Subcommand};
use executor_mcp::{ExecutorServer, config, init, logging};
use rmcp::{ServiceExt, transport::stdio};

/// v1.3: clap-based CLI. Default with no subcommand keeps Phase 1's
/// stdio MCP server behaviour intact.
#[derive(Parser, Debug)]
#[command(name = "executor-mcp", version, about = "onchain-strategy-mcp runtime")]
struct Cli {
    /// Optional path to config.toml. Honoured by the inner `config::load`
    /// flow when no subcommand (or `serve`) is invoked.
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Scaffold `./.local/` and store a fresh burner key in the OS keychain.
    Init {
        /// Overwrite an existing `./.local/config.toml`.
        #[arg(long)]
        force: bool,
        /// Skip interactive prompts (CI / smoke tests).
        #[arg(long)]
        non_interactive: bool,
    },
    /// Run the stdio MCP server (default behaviour when no subcommand
    /// is given).
    Serve,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init {
            force,
            non_interactive,
        }) => init::run(init::InitOptions {
            force,
            non_interactive,
        }),
        Some(Command::Serve) | None => run_serve(),
    }
}

fn run_serve() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let cfg = config::load()?;
        logging::init(&cfg)?;
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            state_path = cfg.state.path.as_str(),
            evm_rpc = cfg.evm.rpc_url.as_str(),
            evm_call_timeout_ms = cfg.evm.call_timeout_ms,
            "executor-mcp starting"
        );
        let server = ExecutorServer::from_config(&cfg)?;
        let service = (*server).clone().serve(stdio()).await?;
        service.waiting().await?;
        Ok::<(), anyhow::Error>(())
    })
}
