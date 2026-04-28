//! POL-03 — selector allowlist (`Any` admits all; RawCall skips this gate).

mod common;

use common::{
    ADDR_A, ADDR_B, addr, cat_of, decision_contract_call, decision_raw_call,
    permissive_policy, SEL_APPROVE, SEL_OTHER, SEL_TRANSFER,
};
use executor_policy::evaluate;
use std::collections::HashMap;

#[test]
fn selector_in_explicit_allow_passes_step_3() {
    // ADDR_B has [transfer, approve] only.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_B), SEL_APPROVE),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn selector_any_admits_all_selectors() {
    // ADDR_A has [Any] — any 4-byte selector is allowed.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_A), SEL_OTHER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn selector_not_in_allow_returns_selector_not_allowed() {
    // ADDR_B doesn't have SEL_OTHER → deny.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_B), SEL_OTHER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "selector_not_allowed");
}

#[test]
fn raw_call_skips_selector_check_per_d06() {
    // RawCall to ADDR_A is allowed by raw_call.allow (Any selector). The
    // selector check would NOT find SEL_OTHER on ADDR_A's selectors list
    // either (which has [Any] — but routing is via raw_call exclusively
    // for RawCall variant per D-06).
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_A), Some(SEL_OTHER)),
        &mut tally,
    );
    // Should NOT be selector_not_allowed; should be allow (raw_call
    // Any-allowed).
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn native_transfer_with_no_selector_skips_selector_check() {
    // NativeTransfer has selector = None and is not RawCall — selector
    // dimension is simply not exercised; pass through if value=0.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &common::decision_native_transfer(31337, addr(ADDR_A), alloy_primitives::U256::ZERO),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn selector_check_skipped_when_chain_contract_has_no_subtable() {
    // ADDR_A is in contracts but has no selectors subtable in this custom
    // policy → check denies (deny-by-default semantics).
    let mut p = permissive_policy();
    let key = executor_policy::ChainContract::new(31337, addr(ADDR_B));
    p.selectors_by_chain_contract.remove(&key);
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_B), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "selector_not_allowed");
}
