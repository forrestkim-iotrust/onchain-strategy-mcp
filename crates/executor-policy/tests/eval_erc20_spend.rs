//! POL-05 — ERC20 cumulative spend cap (D-16).

mod common;

use alloy_primitives::U256;
use common::{
    ADDR_A, ADDR_B, addr, cat_of, decision_contract_call, decision_erc20_approve,
    decision_erc20_transfer, permissive_policy, SEL_TRANSFER,
};
use executor_policy::evaluate;
use std::collections::HashMap;

#[test]
fn erc20_single_transfer_under_cap_passes_and_increments_tally() {
    let p = permissive_policy();
    let token = addr(ADDR_B);
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_erc20_transfer(31337, token, U256::from(100_000u64), 0),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
    // Tally now reflects the running spend.
    assert_eq!(
        tally.get(&(31337u64, token)).copied().unwrap(),
        U256::from(100_000u64)
    );
}

#[test]
fn erc20_transfer_plus_approve_sums_against_cap() {
    let p = permissive_policy();
    let token = addr(ADDR_B);
    let mut tally = HashMap::new();
    // 600_000 transfer
    let v = evaluate(
        &p,
        &decision_erc20_transfer(31337, token, U256::from(600_000u64), 0),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
    // 300_000 approve → cumulative = 900_000 < 1_000_000 cap
    let v = evaluate(
        &p,
        &decision_erc20_approve(31337, token, U256::from(300_000u64), 1),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
    assert_eq!(
        tally.get(&(31337u64, token)).copied().unwrap(),
        U256::from(900_000u64)
    );
}

#[test]
fn erc20_second_action_pushing_over_cap_returns_erc20_spend_exceeds() {
    let p = permissive_policy();
    let token = addr(ADDR_B);
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_erc20_transfer(31337, token, U256::from(600_000u64), 0),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
    // Second action would push cumulative to 1_200_000 > 1_000_000 cap.
    let v = evaluate(
        &p,
        &decision_erc20_approve(31337, token, U256::from(600_000u64), 1),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "erc20_spend_exceeds");
    // Tally was NOT incremented on deny.
    assert_eq!(
        tally.get(&(31337u64, token)).copied().unwrap(),
        U256::from(600_000u64),
        "deny verdict must not mutate tally"
    );
}

#[test]
fn non_erc20_actions_do_not_mutate_tally() {
    let p = permissive_policy();
    let mut tally = HashMap::new();
    // ContractCall to ADDR_A (Any selector — passes); not Erc20.
    let v = evaluate(
        &p,
        &decision_contract_call(31337, addr(ADDR_A), SEL_TRANSFER),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
    assert!(tally.is_empty(), "non-Erc20 action mutated tally: {tally:?}");
}

#[test]
fn erc20_with_no_cap_entry_allows_all() {
    // Researcher A-7: cap absent → uncapped on that token. Drop the cap
    // entry, then verify a giant transfer passes.
    let mut p = permissive_policy();
    p.erc20_spend_by_chain_token.clear();
    let token = addr(ADDR_B);
    let mut tally = HashMap::new();
    let huge = U256::from_str_radix("100000000000000000000000", 10).unwrap();
    let v = evaluate(
        &p,
        &decision_erc20_transfer(31337, token, huge, 0),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
    // Tally still updates so future actions can see the running total even
    // if there's no cap to enforce.
    assert_eq!(tally.get(&(31337u64, token)).copied().unwrap(), huge);
}

#[test]
fn erc20_single_transfer_at_exact_cap_passes() {
    let p = permissive_policy();
    let token = addr(ADDR_B);
    let mut tally = HashMap::new();
    let cap = U256::from(1_000_000u64);
    let v = evaluate(
        &p,
        &decision_erc20_transfer(31337, token, cap, 0),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn erc20_single_transfer_one_over_cap_denies() {
    let p = permissive_policy();
    let token = addr(ADDR_B);
    let mut tally = HashMap::new();
    let over = U256::from(1_000_001u64);
    let v = evaluate(
        &p,
        &decision_erc20_transfer(31337, token, over, 0),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "erc20_spend_exceeds");
}
