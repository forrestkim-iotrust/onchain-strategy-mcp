//! Shared helpers for the per-dimension `eval_*` integration test files.
//!
//! Pattern mirrors `crates/executor-evm/tests/common/mod.rs` (Phase 4 D-14
//! AnvilFixture pattern). Pure-function — no anvil, no async.

#![allow(dead_code, unreachable_pub)]

use alloy_primitives::{Address, U256};
use executor_policy::{
    ChainContract, Decision, DecisionVerdict, LoadedPolicy, NormalizedActionKindCopy,
    SelectorPattern,
};

/// Test addr 1 (lowercase form — accepted via lenient EIP-55).
pub const ADDR_A: &str = "0x5fbdb2315678afecb367f032d93f642f64180aa3";
/// Test addr 2.
pub const ADDR_B: &str = "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512";
/// Foreign addr — never in any allowlist.
pub const ADDR_C: &str = "0x000000000000000000000000000000000000dead";

/// Test selectors (canonical ERC20).
pub const SEL_TRANSFER: [u8; 4] = [0xa9, 0x05, 0x9c, 0xbb];
pub const SEL_APPROVE: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];
pub const SEL_OTHER: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];

pub fn addr(s: &str) -> Address {
    Address::parse_checksummed(s, None)
        .ok()
        .or_else(|| s.parse().ok())
        .unwrap_or_else(|| panic!("test addr {s} unparseable"))
}

/// Extract the rule name from a Deny verdict, "allow" otherwise.
pub fn cat_of(v: &DecisionVerdict) -> &str {
    match v {
        DecisionVerdict::Allow => "allow",
        DecisionVerdict::Deny { rule, .. } => rule.as_ref(),
    }
}

/// Permissive policy: chain 31337, contracts {ADDR_A, ADDR_B},
/// selectors[ADDR_A]=Any, selectors[ADDR_B]={transfer, approve},
/// native cap = 1000 ETH, erc20 cap on ADDR_B = 1_000_000 (small for tests),
/// raw_call allow_global = false; raw_call entry { chain 31337, ADDR_A, Any }.
pub fn permissive_policy() -> LoadedPolicy {
    let a = addr(ADDR_A);
    let b = addr(ADDR_B);
    let mut p = LoadedPolicy {
        chains_allow: vec![31337],
        ..LoadedPolicy::default()
    };
    p.contracts_by_chain.insert(31337, vec![a, b]);
    p.selectors_by_chain_contract
        .insert(ChainContract::new(31337, a), vec![SelectorPattern::Any]);
    p.selectors_by_chain_contract.insert(
        ChainContract::new(31337, b),
        vec![
            SelectorPattern::Specific(SEL_TRANSFER),
            SelectorPattern::Specific(SEL_APPROVE),
        ],
    );
    // 1000 ETH (1e21 wei).
    p.native_value_by_chain
        .insert(31337, U256::from(1_000_000_000_000_000_000_000u128));
    // Small ERC20 cap (1_000_000 base units) so tests can saturate cleanly.
    p.erc20_spend_by_chain_token
        .insert(ChainContract::new(31337, b), U256::from(1_000_000u64));
    p.raw_call_allow_global = false;
    p.raw_call_allow.push(executor_policy::RawCallAllowResolved {
        chain: 31337,
        contract: a,
        selector: SelectorPattern::Any,
    });
    p
}

pub fn decision_contract_call(chain_id: u64, to: Address, sel: [u8; 4]) -> Decision {
    Decision {
        chain_id,
        action_index: 0,
        action_kind: NormalizedActionKindCopy::ContractCall,
        to,
        selector: Some(sel),
        native_value: U256::ZERO,
        erc20_amount: None,
    }
}

pub fn decision_native_transfer(chain_id: u64, to: Address, value: U256) -> Decision {
    Decision {
        chain_id,
        action_index: 0,
        action_kind: NormalizedActionKindCopy::NativeTransfer,
        to,
        selector: None,
        native_value: value,
        erc20_amount: None,
    }
}

pub fn decision_erc20_transfer(
    chain_id: u64,
    token: Address,
    amount: U256,
    action_index: u32,
) -> Decision {
    Decision {
        chain_id,
        action_index,
        action_kind: NormalizedActionKindCopy::Erc20Transfer,
        to: token,
        selector: Some(SEL_TRANSFER),
        native_value: U256::ZERO,
        erc20_amount: Some(amount),
    }
}

pub fn decision_erc20_approve(
    chain_id: u64,
    token: Address,
    amount: U256,
    action_index: u32,
) -> Decision {
    Decision {
        chain_id,
        action_index,
        action_kind: NormalizedActionKindCopy::Erc20Approve,
        to: token,
        selector: Some(SEL_APPROVE),
        native_value: U256::ZERO,
        erc20_amount: Some(amount),
    }
}

pub fn decision_raw_call(
    chain_id: u64,
    to: Address,
    selector: Option<[u8; 4]>,
) -> Decision {
    Decision {
        chain_id,
        action_index: 0,
        action_kind: NormalizedActionKindCopy::RawCall,
        to,
        selector,
        native_value: U256::ZERO,
        erc20_amount: None,
    }
}
