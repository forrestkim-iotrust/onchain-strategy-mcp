//! v1.3: `executor-mcp deploy-delegate` subcommand.
//!
//! Deploys the BatchExec EIP-7702 delegate target via the canonical
//! Arachnid CREATE2 deployer (`0x4e59…956C`). The init code, salt, and
//! deployer are constants in `executor_signer::delegate` — the resulting
//! address is deterministic per chain and shared across every install.
//!
//! Flow:
//!   1. Connect to RPC, read chain id.
//!   2. If predicted address already has code → noop, exit 0.
//!   3. If Arachnid deployer is missing on chain → error with manual
//!      install guidance.
//!   4. Else: load signer (same backend resolution as `serve`), build
//!      `tx { to: ARACHNID_DEPLOYER, data: salt || init_code }`, send,
//!      wait for receipt, verify code is now present.
//!
//! `--dry-run` stops after step 3.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::time::Duration;

use alloy::{
    network::TransactionBuilder,
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
};
use alloy::transports::http::reqwest::Url;
use alloy_primitives::{Bytes, TxKind};
use anyhow::{Context, Result, bail};
use executor_signer::{
    ARACHNID_DEPLOYER, LocalSignerHandle, deploy_calldata, predicted_delegate_address,
};

use crate::config;

pub struct DeployOptions {
    pub rpc_url: Option<String>,
    pub chain_id: Option<u64>,
    pub dry_run: bool,
}

pub fn run(opts: DeployOptions) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_inner(opts))
}

async fn run_inner(opts: DeployOptions) -> Result<()> {
    // Resolve RPC URL: CLI flag > config > error.
    let cfg = config::load().ok();
    let rpc_url = match opts.rpc_url.clone() {
        Some(u) => u,
        None => cfg
            .as_ref()
            .map(|c| c.evm.rpc_url.clone())
            .filter(|u| !u.is_empty())
            .context(
                "no --rpc-url provided and no [evm].rpc_url in config — pass --rpc-url <url>",
            )?,
    };
    let parsed_url = Url::parse(&rpc_url).with_context(|| format!("parsing --rpc-url {rpc_url}"))?;
    let provider = ProviderBuilder::new().connect_http(parsed_url.clone());

    let chain_id = match opts.chain_id {
        Some(id) => id,
        None => provider
            .get_chain_id()
            .await
            .context("fetching chain id from RPC")?,
    };

    let predicted = predicted_delegate_address();
    println!("BatchExec delegate (CREATE2):");
    println!("  predicted address : {predicted}");
    println!("  chain id          : {chain_id}");
    println!("  rpc               : {rpc_url}");
    println!("  deployer          : {ARACHNID_DEPLOYER}");
    println!();

    // (a) already deployed?
    let predicted_code = provider
        .get_code_at(predicted)
        .await
        .context("eth_getCode(predicted)")?;
    if !predicted_code.is_empty() {
        println!(
            "BatchExec already deployed at {predicted} on chain {chain_id}. Nothing to do."
        );
        return Ok(());
    }

    // (b) Arachnid deployer present?
    let deployer_code = provider
        .get_code_at(ARACHNID_DEPLOYER)
        .await
        .context("eth_getCode(ARACHNID_DEPLOYER)")?;
    if deployer_code.is_empty() {
        bail!(
            "Arachnid CREATE2 deployer is missing at {ARACHNID_DEPLOYER} on chain {chain_id}.\n\
             This is rare (pre-2020 forks / custom rollups). To unblock, deploy the Arachnid\n\
             proxy via its Nick-method transaction first, then re-run this command. See:\n  \
             https://github.com/Arachnid/deterministic-deployment-proxy"
        );
    }

    if opts.dry_run {
        println!(
            "[dry-run] Would deploy BatchExec to {predicted} via {ARACHNID_DEPLOYER}\n\
             [dry-run] init code: {} bytes, salt+init calldata: {} bytes\n\
             [dry-run] Re-run without --dry-run to broadcast.",
            executor_signer::BATCH_EXEC_INIT_CODE.len(),
            32 + executor_signer::BATCH_EXEC_INIT_CODE.len(),
        );
        return Ok(());
    }

    // (c) load signer using same resolution as `serve`.
    let cfg = cfg.context("[signer] config missing — run `executor-mcp init` first")?;
    let signer_config = cfg
        .signer_config()
        .map_err(|e| anyhow::anyhow!("loading [signer] config: {e}"))?
        .context("[signer] not configured — run `executor-mcp init` first")?;
    let signer = LocalSignerHandle::resolve(&signer_config, chain_id)
        .map_err(|e| anyhow::anyhow!("resolving signer: {e}"))?;
    let signer_address = signer.signer_address();
    println!("  signer            : {signer_address}");

    // (d) build + send.
    let calldata: Bytes = deploy_calldata().into();
    let tx = TransactionRequest::default()
        .with_to(ARACHNID_DEPLOYER)
        .with_input(calldata)
        .with_from(signer_address);
    // Sanity: ensure the request encodes as a `to` call (not contract creation).
    debug_assert!(matches!(tx.to, Some(TxKind::Call(_))));

    println!();
    println!("Broadcasting deploy tx...");
    let pending = signer
        .broadcast(&rpc_url, tx)
        .await
        .map_err(|e| anyhow::anyhow!("broadcast failed: {e}"))?;
    let tx_hash = pending.tx_hash;
    println!("  tx hash           : {tx_hash}");

    let receipt_timeout = Duration::from_millis(signer_config.receipt_timeout_ms);
    let receipt = signer
        .wait_for_receipt(pending, receipt_timeout)
        .await
        .map_err(|e| anyhow::anyhow!("waiting for receipt: {e}"))?;
    println!(
        "  receipt status    : {}  (gas used: {})",
        receipt.receipt_status.as_str(),
        receipt.gas_used
    );
    if receipt.receipt_status != executor_signer::LocalReceiptStatus::Success {
        bail!("deploy tx reverted — see {tx_hash}");
    }

    // (e) verify code is now present.
    let post_code = provider
        .get_code_at(predicted)
        .await
        .context("eth_getCode(predicted) after deploy")?;
    if post_code.is_empty() {
        bail!(
            "deploy receipt was success but no code at {predicted} — \
             check that the Arachnid deployer is canonical on chain {chain_id}"
        );
    }

    println!();
    println!("BatchExec deployed.");
    println!("  address           : {predicted}");
    println!("  chain id          : {chain_id}");
    println!("  tx hash           : {tx_hash}");
    println!("  code size         : {} bytes", post_code.len());
    println!();
    println!("No config change needed — the runtime auto-resolves this address.");

    Ok(())
}
