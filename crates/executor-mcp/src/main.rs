#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]

use anyhow::Result;
use clap::{Parser, Subcommand};
use executor_mcp::{ExecutorServer, config, deploy_delegate, init, logging, web};
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

    /// v1.6 Track 6A: disable the local web UI. Equivalent to
    /// `OSMCP_NO_UI=1`. When set the MCP stdio server still boots normally.
    #[arg(long, global = true)]
    no_ui: bool,

    /// v1.6 Track 6A: override the UI bind port. Default is 8473 with
    /// fallback to the next free port; an explicit value here is a hard
    /// bind (no fallback) — fail loudly if it's already taken.
    #[arg(long, global = true)]
    ui_port: Option<u16>,

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
    /// Deploy the BatchExec EIP-7702 delegate via CREATE2 (one-time, per
    /// chain). The contract address is deterministic — everyone on the
    /// chain shares the same predicted address.
    DeployDelegate {
        /// RPC URL. Falls back to `[evm].rpc_url` from config when omitted.
        #[arg(long)]
        rpc_url: Option<String>,
        /// Override the chain id. Derived from the RPC when omitted.
        #[arg(long)]
        chain_id: Option<u64>,
        /// Check predicted address + deployer presence without broadcasting.
        #[arg(long)]
        dry_run: bool,
    },
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
        Some(Command::DeployDelegate {
            rpc_url,
            chain_id,
            dry_run,
        }) => deploy_delegate::run(deploy_delegate::DeployOptions {
            rpc_url,
            chain_id,
            dry_run,
        }),
        Some(Command::Serve) | None => run_serve(cli.no_ui, cli.ui_port),
    }
}

fn run_serve(no_ui: bool, ui_port: Option<u16>) -> Result<()> {
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
        // v1.6 Track 6A: spawn the local web UI in a sibling tokio task.
        // The MCP stdio server keeps running regardless of UI bind result —
        // a port collision logs a warn but is non-fatal.
        //
        // v1.6 Track 6C: also wire a provider + EVM config into the UI so
        // `/api/portfolio` can run the idle balance walk. `build_provider`
        // does no network IO, so this is safe to call eagerly at boot.
        let evm_config = cfg.evm_config().map_err(|e| {
            anyhow::anyhow!("parsing [evm] config for UI: {}", e.detail_for_log())
        })?;
        let provider = match executor_evm::build_provider(&evm_config) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(
                    error = %e.detail_for_log(),
                    "ui: provider construction failed — portfolio balance walk disabled"
                );
                None
            }
        };
        let ui_opts = web::WebUiOptions::from_env_and_config(
            cfg.evm.simulation_from.clone(),
            None,
            no_ui,
            ui_port,
            provider,
            evm_config,
            // v1.7 (`ctx.price.usd`): hand the UI the same `price_cache`
            // the orchestrator uses so view + idle walker share entries.
            Some(server.price_cache.clone()),
        );
        match web::spawn(server.state_handle(), ui_opts).await {
            Ok(Some((addr, _handle))) => {
                tracing::info!(
                    bound = %addr,
                    "ui http server spawned",
                );
            }
            Ok(None) => {
                // disabled — no-op
            }
            Err(e) => {
                tracing::warn!(error = %e, "ui http server failed to bind — continuing without UI");
            }
        }
        let service = (*server).clone().serve(stdio()).await?;
        service.waiting().await?;
        Ok::<(), anyhow::Error>(())
    })
}
