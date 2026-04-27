//! ERC20 read helpers — Phase 4 D-06.
//!
//! Six thin wrappers around [`crate::read::read_contract`] that bundle a
//! canonical OpenZeppelin-compatible ABI fragment (`balanceOf`, `allowance`,
//! `decimals`, `symbol`, `name`, `totalSupply`). Selector-stable across all
//! major ERC20 implementations.
//!
//! The agent never has to supply an ABI; the helpers feed [`ERC20_ABI`] into
//! `read_contract` and return the same `serde_json::Value` shape per
//! D-03 (decimal-string for U256 outputs, JS Number for `decimals`, JS string
//! for `symbol` / `name`).

use std::sync::Arc;

use alloy::providers::DynProvider;

use crate::read::{BlockTag, ReadContractInput, read_contract};
use crate::{EvmConfig, EvmError};

/// Canonical OpenZeppelin-compatible ERC20 ABI fragment. Bundled as a static
/// string so strategies never have to supply one. Includes the six read
/// functions the helpers expose. Selector-stable across implementations.
pub const ERC20_ABI: &str = r#"[
    {"type":"function","name":"balanceOf","inputs":[{"name":"account","type":"address"}],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
    {"type":"function","name":"allowance","inputs":[{"name":"owner","type":"address"},{"name":"spender","type":"address"}],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
    {"type":"function","name":"decimals","inputs":[],"outputs":[{"name":"","type":"uint8"}],"stateMutability":"view"},
    {"type":"function","name":"symbol","inputs":[],"outputs":[{"name":"","type":"string"}],"stateMutability":"view"},
    {"type":"function","name":"name","inputs":[],"outputs":[{"name":"","type":"string"}],"stateMutability":"view"},
    {"type":"function","name":"totalSupply","inputs":[],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"}
]"#;

/// `ctx.evm.readErc20.balanceOf(token, account, blockTag?)` — wei decimal
/// string per D-03.
pub async fn erc20_balance_of(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    token: &str,
    account: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    read_contract(
        provider,
        cfg,
        ReadContractInput {
            address: token.to_string(),
            abi_json: ERC20_ABI.to_string(),
            function: "balanceOf".to_string(),
            args: vec![serde_json::Value::String(account.to_string())],
            block_tag,
        },
    )
    .await
}

/// `ctx.evm.readErc20.allowance(token, owner, spender, blockTag?)` — wei
/// decimal string.
pub async fn erc20_allowance(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    token: &str,
    owner: &str,
    spender: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    read_contract(
        provider,
        cfg,
        ReadContractInput {
            address: token.to_string(),
            abi_json: ERC20_ABI.to_string(),
            function: "allowance".to_string(),
            args: vec![
                serde_json::Value::String(owner.to_string()),
                serde_json::Value::String(spender.to_string()),
            ],
            block_tag,
        },
    )
    .await
}

/// `ctx.evm.readErc20.decimals(token, blockTag?)` — JSON Number (uint8 fits).
pub async fn erc20_decimals(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    token: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    read_contract(
        provider,
        cfg,
        ReadContractInput {
            address: token.to_string(),
            abi_json: ERC20_ABI.to_string(),
            function: "decimals".to_string(),
            args: vec![],
            block_tag,
        },
    )
    .await
}

/// `ctx.evm.readErc20.symbol(token, blockTag?)` — JSON string.
pub async fn erc20_symbol(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    token: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    read_contract(
        provider,
        cfg,
        ReadContractInput {
            address: token.to_string(),
            abi_json: ERC20_ABI.to_string(),
            function: "symbol".to_string(),
            args: vec![],
            block_tag,
        },
    )
    .await
}

/// `ctx.evm.readErc20.name(token, blockTag?)` — JSON string.
pub async fn erc20_name(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    token: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    read_contract(
        provider,
        cfg,
        ReadContractInput {
            address: token.to_string(),
            abi_json: ERC20_ABI.to_string(),
            function: "name".to_string(),
            args: vec![],
            block_tag,
        },
    )
    .await
}

/// `ctx.evm.readErc20.totalSupply(token, blockTag?)` — wei decimal string.
pub async fn erc20_total_supply(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    token: &str,
    block_tag: BlockTag,
) -> Result<serde_json::Value, EvmError> {
    read_contract(
        provider,
        cfg,
        ReadContractInput {
            address: token.to_string(),
            abi_json: ERC20_ABI.to_string(),
            function: "totalSupply".to_string(),
            args: vec![],
            block_tag,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_json_abi::JsonAbi;

    #[test]
    fn erc20_abi_parses_and_contains_six_functions() {
        let abi: JsonAbi =
            serde_json::from_str(ERC20_ABI).expect("ERC20_ABI must be valid JsonAbi");
        for f in [
            "balanceOf",
            "allowance",
            "decimals",
            "symbol",
            "name",
            "totalSupply",
        ] {
            let fns = abi
                .function(f)
                .unwrap_or_else(|| panic!("ERC20_ABI missing function: {f}"));
            assert!(
                !fns.is_empty(),
                "ERC20_ABI function `{f}` resolves to empty overload set"
            );
        }
    }

    #[test]
    fn erc20_abi_balanceof_signature_matches_canonical_selector() {
        // Canonical ERC20 balanceOf selector is 0x70a08231. We rely on
        // alloy_json_abi::Function::selector() to compute it from the bundled
        // fragment — if the fragment drifts from the canonical signature
        // (e.g. arg renamed to a non-empty internal type), the selector
        // would change and this test catches it.
        let abi: JsonAbi = serde_json::from_str(ERC20_ABI).unwrap();
        let f = &abi.function("balanceOf").unwrap()[0];
        assert_eq!(format!("0x{:x}", f.selector()), "0x70a08231");
    }

    #[test]
    fn erc20_abi_decimals_returns_uint8_per_oz_convention() {
        let abi: JsonAbi = serde_json::from_str(ERC20_ABI).unwrap();
        let f = &abi.function("decimals").unwrap()[0];
        assert_eq!(f.outputs.len(), 1);
        assert_eq!(f.outputs[0].ty, "uint8");
    }
}
