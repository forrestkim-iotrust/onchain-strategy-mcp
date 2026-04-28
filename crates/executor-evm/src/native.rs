//! Native (chain-base-asset) read helpers — Phase 4 D-07.
//!
//! Two thin wrappers around alloy's `Provider::get_balance` /
//! `Provider::get_block_number`. NO ABI involved — these are direct JSON-RPC
//! calls (`eth_getBalance` / `eth_blockNumber`).
//!
//! Output convention (D-03):
//! - `balance` → JSON String (wei, base-10, no `0x` prefix). U256 fits no JS
//!   Number, so NEVER cast.
//! - `blockNumber` → JSON Number. Block heights fit u64 / f64 within all
//!   foreseeable lifetimes; JS can consume them as Number safely.
//!
//! `chainId` is intentionally **NOT** exposed via the JS sandbox — Phase-5
//! policy boundary owns chain identity (D-07). For host-side use the
//! [`fetch_chain_id`] helper which the orchestrator caches on
//! `ExecutorServer`.

use std::str::FromStr;
use std::sync::Arc;

use alloy::providers::{DynProvider, Provider};
use alloy_primitives::Address;

use crate::read::BlockTag;
use crate::{EvmConfig, EvmError};

/// Phase 5 D-17 — fetch the connected chain's id via `eth_chainId`. The
/// orchestrator caches the value on `ExecutorServer.chain_id_cell`. Errors
/// surface as [`EvmError::Transport`] per the Phase-4 wire taxonomy.
pub async fn fetch_chain_id(provider: &Arc<DynProvider>) -> Result<u64, EvmError> {
    provider
        .get_chain_id()
        .await
        .map_err(|e| EvmError::Transport {
            detail_for_log: format!("get_chain_id: {e}"),
        })
}

/// `ctx.evm.readNative.balance(account, blockTag?)` — returns wei as decimal
/// string. `blockTag` defaults to [`BlockTag::Latest`] in the host binding;
/// callers must supply it explicitly here.
pub async fn native_balance(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    account: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    let addr = parse_address(account)?;
    let block_id = block_tag.to_block_id();
    let call = provider.get_balance(addr).block_id(block_id);
    let bal = match tokio::time::timeout(cfg.call_timeout, call).await {
        Err(_) => return Err(EvmError::Timeout),
        Ok(Err(e)) => {
            return Err(EvmError::Transport {
                detail_for_log: format!("eth_getBalance: {e}"),
            });
        }
        Ok(Ok(b)) => b,
    };
    // U256: Display = base-10. Decimal-string per D-03.
    Ok(serde_json::Value::String(bal.to_string()))
}

/// `ctx.evm.readNative.blockNumber()` — current head block number as JSON
/// Number.
pub async fn native_block_number(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
) -> Result<serde_json::Value, EvmError> {
    let call = provider.get_block_number();
    let n = match tokio::time::timeout(cfg.call_timeout, call).await {
        Err(_) => return Err(EvmError::Timeout),
        Ok(Err(e)) => {
            return Err(EvmError::Transport {
                detail_for_log: format!("eth_blockNumber: {e}"),
            });
        }
        Ok(Ok(n)) => n,
    };
    Ok(serde_json::Value::Number(n.into()))
}

/// Lenient address parse (mirrors `dyn_abi`'s convention — accept lowercase
/// or EIP-55, no checksum strictness; the action validator at the MCP
/// boundary owns checksum strictness for D-09).
fn parse_address(s: &str) -> Result<Address, EvmError> {
    Address::from_str(s).map_err(|e| EvmError::Encode {
        category: std::borrow::Cow::Borrowed("bad_address"),
        detail_for_log: format!("address parse: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;

    #[test]
    fn native_balance_emits_decimal_string_for_uint256_smoke() {
        // Pin the contract: U256 stringifies to base-10 with no `0x` prefix
        // and no leading zero. The native_balance helper relies on this.
        let v = U256::from(1_000_000u64).to_string();
        assert_eq!(v, "1000000");
        assert!(!v.starts_with("0x"));
    }

    #[test]
    fn native_balance_emits_decimal_string_for_max_u256() {
        // 2^256 - 1 has 78 digits and would NEVER fit JS Number (53-bit
        // mantissa). Decimal-string-via-Display is the only correct shape.
        let max = U256::MAX.to_string();
        assert_eq!(max.len(), 78);
        assert!(max.bytes().all(|b| b.is_ascii_digit()));
    }

    #[test]
    fn parse_address_accepts_lowercase_and_eip55() {
        let lower = "0x0000000000000000000000000000000000000001";
        let mixed = "0x000000000000000000000000000000000000dEaD";
        assert!(parse_address(lower).is_ok());
        assert!(parse_address(mixed).is_ok());
    }

    #[test]
    fn parse_address_rejects_garbage() {
        let err = parse_address("not-an-address").unwrap_err();
        match err {
            EvmError::Encode { category, .. } => assert_eq!(category, "bad_address"),
            other => panic!("expected Encode(bad_address), got {other:?}"),
        }
    }
}
