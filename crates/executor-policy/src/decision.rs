//! [`Decision`] / [`DecisionVerdict`] — alloy-free types passed across the
//! `executor-mcp` -> `executor-policy` boundary (Phase 5 D-01 / D-20).
//!
//! The orchestrator (Plan 05-04) constructs a `Decision` from
//! `executor_evm::normalize::NormalizedAction` plus the cached `chain_id`,
//! then calls `executor_policy::eval` (Plan 05-03 — body lands later).
//! Conversion happens at the orchestrator so this crate stays alloy-free.

use alloy_primitives::{Address, U256};
use std::borrow::Cow;

/// Mirror of `executor_evm::normalize::NormalizedActionKind`.
///
/// Duplicated here to keep `executor-policy` alloy-free per D-20. The
/// orchestrator (executor-mcp::tools::strategy_run) maps the executor-evm
/// enum to this `*Copy` enum 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedActionKindCopy {
    ContractCall,
    RawCall,
    Erc20Transfer,
    Erc20Approve,
    NativeTransfer,
}

/// Input shape for the policy evaluator (Plan 05-03 lands the body).
#[derive(Debug, Clone)]
pub struct Decision {
    pub chain_id: u64,
    pub action_index: u32,
    pub action_kind: NormalizedActionKindCopy,
    /// Recipient / target contract address.
    pub to: Address,
    /// 4-byte calldata selector. `None` for native-coin transfers and
    /// sub-4-byte raw calls (P-4).
    pub selector: Option<[u8; 4]>,
    /// Native-coin value attached to the call (POL-04 cap input).
    pub native_value: U256,
    /// ERC20 amount for `Erc20Transfer` / `Erc20Approve` (POL-05 cap input).
    /// `None` for non-ERC20 actions.
    pub erc20_amount: Option<U256>,
}

/// Verdict returned by the policy evaluator (Plan 05-03 lands the body).
#[derive(Debug, Clone)]
pub enum DecisionVerdict {
    Allow,
    Deny {
        rule: Cow<'static, str>,
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_struct_is_alloy_primitives_only() {
        // Compile-time test: this constructor must work with only
        // alloy-primitives types. If it ever requires the umbrella `alloy`
        // crate, the build will break (D-20 contract).
        let d = Decision {
            chain_id: 31337,
            action_index: 0,
            action_kind: NormalizedActionKindCopy::ContractCall,
            to: Address::ZERO,
            selector: Some([0xa9, 0x05, 0x9c, 0xbb]),
            native_value: U256::ZERO,
            erc20_amount: Some(U256::from(1000u64)),
        };
        assert_eq!(d.chain_id, 31337);
        assert_eq!(d.action_index, 0);
        assert_eq!(d.action_kind, NormalizedActionKindCopy::ContractCall);
        assert_eq!(d.selector, Some([0xa9, 0x05, 0x9c, 0xbb]));
    }

    #[test]
    fn verdict_allow_and_deny_are_constructible() {
        let _ = DecisionVerdict::Allow;
        let v = DecisionVerdict::Deny {
            rule: Cow::Borrowed("contract_not_allowed"),
            detail: "0x...".into(),
        };
        match v {
            DecisionVerdict::Deny { rule, .. } => {
                assert_eq!(rule.as_ref(), "contract_not_allowed");
            }
            _ => panic!("expected Deny"),
        }
    }
}
