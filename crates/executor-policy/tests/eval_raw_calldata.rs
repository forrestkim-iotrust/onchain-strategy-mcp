//! POL-06 — raw_call gate (deny by default).

mod common;

use common::{
    ADDR_A, ADDR_B, ADDR_C, addr, cat_of, decision_raw_call, permissive_policy, SEL_OTHER,
    SEL_TRANSFER,
};
use executor_policy::{RawCallAllowResolved, SelectorPattern, evaluate};
use std::collections::HashMap;

#[test]
fn raw_call_denied_when_not_in_allowlist_returns_raw_call_denied() {
    // ADDR_B is in contracts but NOT in raw_call.allow.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_B), Some(SEL_TRANSFER)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "raw_call_denied");
}

#[test]
fn raw_call_allow_global_admits_all_raw() {
    let mut p = permissive_policy();
    p.raw_call_allow_global = true;
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_B), Some(SEL_OTHER)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn raw_call_specific_entry_admits_one_combination() {
    let mut p = permissive_policy();
    p.raw_call_allow.clear();
    p.raw_call_allow.push(RawCallAllowResolved {
        chain: 31337,
        contract: addr(ADDR_B),
        selector: SelectorPattern::Specific(SEL_TRANSFER),
    });
    let mut tally = HashMap::new();

    // Match: chain + contract + exact selector.
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_B), Some(SEL_TRANSFER)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");

    // Same contract, different selector → deny.
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_B), Some(SEL_OTHER)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "raw_call_denied");
}

#[test]
fn raw_call_any_selector_admits_all_selectors_at_contract() {
    // ADDR_A in permissive policy has Any.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_A), Some(SEL_OTHER)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");
}

#[test]
fn raw_call_with_none_selector_requires_any_or_global() {
    // ADDR_A has Any → admits None too.
    let p = permissive_policy();
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_A), None),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "allow");

    // Specific entry with None calldata → denied.
    let mut p2 = permissive_policy();
    p2.raw_call_allow.clear();
    p2.raw_call_allow.push(RawCallAllowResolved {
        chain: 31337,
        contract: addr(ADDR_A),
        selector: SelectorPattern::Specific(SEL_TRANSFER),
    });
    let v = evaluate(
        &p2,
        &decision_raw_call(31337, addr(ADDR_A), None),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "raw_call_denied");
}

#[test]
fn raw_call_to_unknown_contract_denied() {
    // Even with allow_global = false, a contract not in raw_call.allow gets
    // denied. (Note ADDR_C is not in contracts allow either, so contract
    // check denies first.)
    let mut p = permissive_policy();
    // Add ADDR_C to contracts list to bypass POL-02 and exercise POL-06.
    p.contracts_by_chain
        .get_mut(&31337)
        .unwrap()
        .push(addr(ADDR_C));
    let mut tally = HashMap::new();
    let v = evaluate(
        &p,
        &decision_raw_call(31337, addr(ADDR_C), Some(SEL_TRANSFER)),
        &mut tally,
    );
    assert_eq!(cat_of(&v), "raw_call_denied");
}
