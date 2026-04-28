//! POL-02 — contract allowlist (deny-by-default; missing subtable → deny).

mod common;

use common::{ADDR_A, ADDR_C, addr, cat_of, decision_contract_call, permissive_policy, SEL_TRANSFER};
use executor_policy::{LoadedPolicy, evaluate};
use std::collections::HashMap;

#[test]
fn contract_in_allow_passes_step_2() {
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
fn contract_not_in_allow_returns_contract_not_allowed() {
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_C), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "contract_not_allowed");
}

#[test]
fn contracts_subtable_missing_for_chain_returns_contract_not_allowed() {
    // Defense-in-depth: in-memory policy with chain in allow but no
    // contracts subtable (load.rs Pitfall P-10 prevents this from disk;
    // tests cover programmatic constructors).
    let mut p = LoadedPolicy {
        chains_allow: vec![31337],
        ..LoadedPolicy::default()
    };
    // Note: contracts_by_chain is intentionally empty.
    p.raw_call_allow_global = false;
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_A), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "contract_not_allowed");
}
