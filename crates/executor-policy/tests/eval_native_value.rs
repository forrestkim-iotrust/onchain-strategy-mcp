//! POL-04 — per-action native value cap.

mod common;

use alloy_primitives::U256;
use common::{ADDR_A, addr, cat_of, decision_native_transfer, permissive_policy};
use executor_policy::{LoadedPolicy, evaluate};
use std::collections::HashMap;

#[test]
fn native_value_zero_skips_step_5_regardless_of_cap() {
    // Even with cap=0 (no entry), value=0 always passes.
    let mut p = permissive_policy();
    p.native_value_by_chain.clear();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_native_transfer(31337, addr(ADDR_A), U256::ZERO),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn native_value_below_cap_passes() {
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_native_transfer(31337, addr(ADDR_A), U256::from(1_000_000_000u64)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn native_value_above_cap_returns_native_value_exceeds() {
    let p = permissive_policy();
    let mut tally = HashMap::new();
    // 2000 ETH > 1000 ETH cap.
    let v = evaluate(
        &p,
        &decision_native_transfer(
            31337,
            addr(ADDR_A),
            U256::from(2_000_000_000_000_000_000_000u128),
        ),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "native_value_exceeds");
}

#[test]
fn native_value_with_no_chain_entry_treats_cap_as_zero() {
    // Chain in allowlist + contract in allow + selector skipped (None) +
    // BUT native_value_by_chain has no entry → cap = 0 → any non-zero value
    // denies.
    let mut p = permissive_policy();
    p.native_value_by_chain.clear();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_native_transfer(31337, addr(ADDR_A), U256::from(1u64)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "native_value_exceeds");
}

#[test]
fn native_value_at_exact_cap_passes() {
    // Boundary check: value == cap is allowed (`> cap`, not `>=`).
    let p = permissive_policy();
    let cap = U256::from(1_000_000_000_000_000_000_000u128);
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_native_transfer(31337, addr(ADDR_A), cap),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn native_value_check_does_not_run_when_chain_denied() {
    // Short-circuit: chain not in allowlist short-circuits before native value.
    let p = LoadedPolicy::default();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_native_transfer(
            31337,
            addr(ADDR_A),
            U256::from(2_000_000_000_000_000_000_000u128),
        ),
        &mut tally,
    );
    // Chain check runs first.
    assert_eq!(cat_of(&v), "chain_not_allowed");
}
