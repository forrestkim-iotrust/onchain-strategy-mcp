//! Phase 5 D-06 — 6-dimension deny-by-default policy evaluator.
//!
//! Cheap-first short-circuit ordering (D-06 / D-07):
//!
//! 1. POL-01 — `chain` (chain_id ∈ allowlist)
//! 2. POL-02 — `contract` (decision.to ∈ contracts.<chain> allow list)
//! 3. POL-06 — `raw_call` gate (only for `RawCall` variant — exclusive with
//!    the selector check per D-06)
//! 4. POL-03 — `selector` (only for non-RawCall variants)
//! 5. POL-04 — `native_value` (only when `decision.native_value > 0`)
//! 6. POL-05 — `erc20_spend` cumulative (D-16 — only for `Erc20Transfer` /
//!    `Erc20Approve`; tally INCREMENTS only on `Allow`)
//!
//! Stable violation taxonomy (locked, used as `data.rule` per D-08):
//! `chain_not_allowed`, `contract_not_allowed`, `selector_not_allowed`,
//! `native_value_exceeds`, `erc20_spend_exceeds`, `raw_call_denied`.
//!
//! ## D-16 ERC20 cumulative tally
//!
//! `erc20_tally` is a `HashMap<(chain_id, token), U256>` owned by the
//! orchestrator (Plan 05-04) and reset per run. On `Allow` for an
//! `Erc20Transfer` / `Erc20Approve` action, the running total is incremented
//! BEFORE the next action is evaluated. Subsequent ERC20 actions to the same
//! `(chain, token)` see the cumulative running total against the per-token cap.

use crate::decision::{Decision, DecisionVerdict, NormalizedActionKindCopy};
use crate::model::LoadedPolicy;
use alloy_primitives::{Address, U256};
use std::borrow::Cow;
use std::collections::HashMap;

/// Evaluate one normalized action against the policy.
///
/// Mutates `erc20_tally` only on `Allow` for `Erc20Transfer` / `Erc20Approve`
/// actions. Returns the first deny verdict (cheap-first short-circuit) or
/// `Allow` when every dimension passes / is skipped.
///
/// **Wire-safety (MR-01):** all `Deny.detail` strings start with a stable
/// taxonomy prefix (`"chain "`, `"contract "`, `"selector "`, `"native value "`,
/// `"cumulative spend "`, `"raw_call "`) and never embed raw alloy / serde /
/// toml text. The wire factory `executor_mcp::errors::map_policy_error`
/// (Plan 05-03 Task 3) prepends `"policy violation: "`.
pub fn evaluate(
    policy: &LoadedPolicy,
    decision: &Decision,
    erc20_tally: &mut HashMap<(u64, Address), U256>,
) -> DecisionVerdict {
    // 1. POL-01 — chain.
    if !policy.allows_chain(decision.chain_id) {
        return DecisionVerdict::Deny {
            rule: Cow::Borrowed("chain_not_allowed"),
            detail: format!("chain {} not in policy allowlist", decision.chain_id),
        };
    }

    // 2. POL-02 — contract.
    if !policy.allows_contract(decision.chain_id, &decision.to) {
        return DecisionVerdict::Deny {
            rule: Cow::Borrowed("contract_not_allowed"),
            detail: format!(
                "contract {} not allowed on chain {}",
                decision.to, decision.chain_id
            ),
        };
    }

    // 3 / 3'. POL-06 (RawCall gate) and POL-03 (selector) are mutually
    // exclusive per D-06: RawCall actions go through raw_call ONLY; every
    // other variant goes through the selector check.
    if matches!(decision.action_kind, NormalizedActionKindCopy::RawCall) {
        if !policy.raw_call_allows(decision.chain_id, &decision.to, decision.selector.as_ref())
        {
            let sel_str = match decision.selector {
                Some(s) => format!("0x{:02x}{:02x}{:02x}{:02x}", s[0], s[1], s[2], s[3]),
                None => "<none>".into(),
            };
            return DecisionVerdict::Deny {
                rule: Cow::Borrowed("raw_call_denied"),
                detail: format!(
                    "raw_call to {} selector {} not in policy allowlist",
                    decision.to, sel_str
                ),
            };
        }
    } else if let Some(sel) = decision.selector
        && !policy.allows_selector(decision.chain_id, &decision.to, &sel)
    {
        let hex = format!("0x{:02x}{:02x}{:02x}{:02x}", sel[0], sel[1], sel[2], sel[3]);
        return DecisionVerdict::Deny {
            rule: Cow::Borrowed("selector_not_allowed"),
            detail: format!(
                "selector {hex} not allowed for {} on chain {}",
                decision.to, decision.chain_id
            ),
        };
    }
    // selector == None for non-RawCall is only NativeTransfer (no calldata) —
    // the selector dimension simply does not apply.

    // 5. POL-04 — native value cap.
    if decision.native_value > U256::ZERO {
        let cap = policy.native_value_cap(decision.chain_id);
        if decision.native_value > cap {
            return DecisionVerdict::Deny {
                rule: Cow::Borrowed("native_value_exceeds"),
                detail: format!(
                    "native value {} exceeds per-action cap {} on chain {}",
                    decision.native_value, cap, decision.chain_id
                ),
            };
        }
    }

    // 6. POL-05 — ERC20 cumulative spend (D-16). Erc20Transfer/Approve only.
    let is_erc20 = matches!(
        decision.action_kind,
        NormalizedActionKindCopy::Erc20Transfer | NormalizedActionKindCopy::Erc20Approve
    );
    if is_erc20
        && let Some(amount) = decision.erc20_amount
    {
        let key = (decision.chain_id, decision.to);
        let running = erc20_tally.get(&key).copied().unwrap_or(U256::ZERO);
        let next = running.saturating_add(amount);
        if let Some(cap) = policy.erc20_spend_cap(decision.chain_id, &decision.to)
            && next > cap
        {
            return DecisionVerdict::Deny {
                rule: Cow::Borrowed("erc20_spend_exceeds"),
                detail: format!(
                    "cumulative spend of token {} exceeds per-run cap {}",
                    decision.to, cap
                ),
            };
        }
        // (cap absent → no limit at this token; documented researcher A-7.)
        erc20_tally.insert(key, next);
    }

    DecisionVerdict::Allow
}
