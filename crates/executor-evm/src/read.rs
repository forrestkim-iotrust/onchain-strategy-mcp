//! `read_contract` — Phase 4 D-05 entry point.
//!
//! Task 1 lands the input/output types and a placeholder body; Task 2 lands
//! the real eth_call lifecycle.

use std::sync::Arc;

use alloy::providers::DynProvider;

use crate::{EvmConfig, EvmError};

/// Input shape mirroring the JS-facing `ctx.evm.readContract` signature
/// (Phase 4 D-05). The strategy-js host binding builds this from the JS
/// argument object and stringifies array-form `abi` to canonical JSON.
#[derive(Debug, Clone)]
pub struct ReadContractInput {
    pub address: String,
    /// ABI as canonical JSON. The host stringifies array-form `abi` before
    /// constructing this struct so the journal records a stable representation.
    pub abi_json: String,
    pub function: String,
    pub args: Vec<serde_json::Value>,
    pub block_tag: BlockTag,
}

/// Phase 4 supports `latest` / `pending` / explicit block number. `safe` /
/// `finalized` are deferred until a strategy actually requests them.
#[derive(Debug, Clone, Copy, Default)]
pub enum BlockTag {
    #[default]
    Latest,
    Pending,
    Number(u64),
}

/// Resolve overload, encode args, eth_call with timeout, decode output.
/// Returns the decoded output as a `serde_json::Value` per Phase 4 D-03.
///
/// Task 1 ships a placeholder body — Task 2 lands the full RESEARCH 9-step
/// flow against an anvil-deployed Counter contract.
pub async fn read_contract(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    input: ReadContractInput,
) -> Result<serde_json::Value, EvmError> {
    let _ = (provider, cfg, input);
    Err(EvmError::Config {
        detail_for_log: "Task 2 lands the read_contract body".into(),
    })
}
