//! POL-01 — chain allowlist + cross-cutting short-circuit + wire-safety.

mod common;

use common::{
    ADDR_A, ADDR_C, addr, cat_of, decision_contract_call, decision_native_transfer,
    decision_raw_call, permissive_policy, SEL_TRANSFER,
};
use alloy_primitives::U256;
use executor_policy::{DecisionVerdict, LoadedPolicy, evaluate};
use std::collections::HashMap;

#[test]
fn chain_in_allow_passes_first_step() {
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_A), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn chain_not_in_allow_returns_chain_not_allowed() {
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(999, addr(ADDR_A), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "chain_not_allowed");
}

#[test]
fn empty_chains_allow_denies_all_chains() {
    let p = LoadedPolicy::default();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_A), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "chain_not_allowed");
}

#[test]
fn evaluate_short_circuits_at_first_deny_chain() {
    // Empty policy → bad chain; even though contracts/selectors/value would
    // also deny, the first deny is `chain_not_allowed`.
    let p = LoadedPolicy::default();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_C), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "chain_not_allowed");
}

#[test]
fn deny_detail_strings_use_stable_taxonomy_prefixes() {
    // MR-01 lock: each rule's detail starts with a stable prefix.
    let p = permissive_policy();
    let mut tally = HashMap::new();

    // chain
    let v = evaluate(
        &p,
        &decision_contract_call(999, addr(ADDR_A), SEL_TRANSFER),
        &mut tally,
    );
    let detail = match v {
        DecisionVerdict::Deny { detail, .. } => detail,
        _ => panic!("expected Deny"),
    };
    assert!(detail.starts_with("chain "), "chain detail: {detail}");

    // contract
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_C), SEL_TRANSFER),
        &mut tally,
    );
    let detail = match v {
        DecisionVerdict::Deny { detail, .. } => detail,
        _ => panic!("expected Deny"),
    };
    assert!(
        detail.starts_with("contract "),
        "contract detail: {detail}"
    );

    // selector — use ADDR_B which has only transfer/approve, not SEL_OTHER.
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(common::ADDR_B), common::SEL_OTHER),
        &mut tally,
    );
    let detail = match v {
        DecisionVerdict::Deny { detail, .. } => detail,
        _ => panic!("expected Deny"),
    };
    assert!(
        detail.starts_with("selector "),
        "selector detail: {detail}"
    );

    // native_value (native transfer with value above cap on a permitted addr).
    let v = evaluate(
        &p,
        &decision_native_transfer(
            31337,
            addr(ADDR_A),
            U256::from(2_000_000_000_000_000_000_000u128), // 2000 ETH > 1000 cap
        ),
        &mut tally,
    );
    let detail = match v {
        DecisionVerdict::Deny { detail, .. } => detail,
        _ => panic!("expected Deny"),
    };
    assert!(
        detail.starts_with("native value "),
        "native_value detail: {detail}"
    );

    // erc20_spend_exceeds — single transfer above cap on ADDR_B.
    let cap = U256::from(1_000_000u64);
    let over = cap + U256::from(1u64);
    let v = evaluate(
        &p,
        &common::decision_erc20_transfer(31337, addr(common::ADDR_B), over, 0),
        &mut tally,
    );
    let detail = match v {
        DecisionVerdict::Deny { detail, .. } => detail,
        _ => panic!("expected Deny"),
    };
    assert!(
        detail.starts_with("cumulative spend "),
        "erc20 detail: {detail}"
    );

    // raw_call_denied — RawCall to a contract NOT in raw_call.allow.
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(common::ADDR_B), Some(common::SEL_OTHER)),
        &mut tally,
    );
    let detail = match v {
        DecisionVerdict::Deny { detail, .. } => detail,
        _ => panic!("expected Deny"),
    };
    assert!(
        detail.starts_with("raw_call "),
        "raw_call detail: {detail}"
    );
}
