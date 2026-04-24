//! Golden JSON Schema snapshots for tool / prompt input structs.
//!
//! Each test generates the current `schemars::schema_for!` output for a public
//! input struct and compares it against a committed golden file under
//! `tests/schemas/<Name>.json`. Run
//!
//! ```text
//! UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots
//! ```
//!
//! to create or refresh the goldens after an intentional schema change.
//! Accidental drift fails the test so Phase 2+ must consciously bump the
//! contract.

use executor_core::schema::{
    execution::ExecutionIdInput,
    policy::PolicyUpdateInput,
    prompt_args::{ReviewEvmStrategyArgs, WriteEvmStrategyArgs},
    strategy::{StrategyIdInput, StrategyRegisterInput, StrategyRunOnceInput},
};
use schemars::schema_for;

fn assert_schema_matches_golden<S: serde::Serialize>(name: &str, schema: S) {
    let actual = serde_json::to_string_pretty(&schema).expect("serialize schema");
    let path = format!("tests/schemas/{name}.json");

    if std::env::var("UPDATE_SCHEMAS").is_ok() {
        std::fs::write(&path, format!("{actual}\n")).expect("write golden file");
        return;
    }

    let expected = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => panic!(
            "missing golden file: {path}. Run `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots` to create it."
        ),
    };

    assert_eq!(
        actual.trim(),
        expected.trim(),
        "schema drift for {name}. Inspect the diff; if the change is intentional run UPDATE_SCHEMAS=1 to refresh the golden."
    );
}

#[test]
fn strategy_register_input_schema_stable() {
    assert_schema_matches_golden("StrategyRegisterInput", schema_for!(StrategyRegisterInput));
}

#[test]
fn strategy_id_input_schema_stable() {
    assert_schema_matches_golden("StrategyIdInput", schema_for!(StrategyIdInput));
}

#[test]
fn strategy_run_once_input_schema_stable() {
    assert_schema_matches_golden("StrategyRunOnceInput", schema_for!(StrategyRunOnceInput));
}

#[test]
fn execution_id_input_schema_stable() {
    assert_schema_matches_golden("ExecutionIdInput", schema_for!(ExecutionIdInput));
}

#[test]
fn policy_update_input_schema_stable() {
    assert_schema_matches_golden("PolicyUpdateInput", schema_for!(PolicyUpdateInput));
}

#[test]
fn write_evm_strategy_args_schema_stable() {
    assert_schema_matches_golden("WriteEvmStrategyArgs", schema_for!(WriteEvmStrategyArgs));
}

#[test]
fn review_evm_strategy_args_schema_stable() {
    assert_schema_matches_golden("ReviewEvmStrategyArgs", schema_for!(ReviewEvmStrategyArgs));
}
