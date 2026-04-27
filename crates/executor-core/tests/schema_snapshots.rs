//! Golden JSON Schema snapshots for tool / prompt input + response structs.
//!
//! Each test generates the current `schemars::schema_for!` output for a public
//! type and compares it against a committed golden file under
//! `tests/schemas/<Name>.json`. Run
//!
//! ```text
//! UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots
//! ```
//!
//! to create or refresh the goldens after an intentional schema change.
//! Accidental drift fails the test so future phases must consciously bump the
//! contract.

use executor_core::schema::{
    execution::{
        ExecutionGetResponse, ExecutionIdInput, JournalActionOutcome, RunStatus, StrategyOutcome,
        StrategyRunResponse,
    },
    policy::PolicyUpdateInput,
    prompt_args::{ReviewEvmStrategyArgs, WriteEvmStrategyArgs},
    strategy::{
        StrategyDeleteResponse, StrategyGetInput, StrategyGetResponse, StrategyIdInput,
        StrategyListResponse, StrategyRegisterInput, StrategyRegisterResponse,
        StrategyRunInput,
    },
};
use schemars::schema_for;
use std::collections::BTreeSet;

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
fn strategy_run_input_schema_stable() {
    assert_schema_matches_golden("StrategyRunInput", schema_for!(StrategyRunInput));
}

#[test]
fn strategy_run_response_schema_stable() {
    assert_schema_matches_golden("StrategyRunResponse", schema_for!(StrategyRunResponse));
}

#[test]
fn strategy_outcome_schema_stable() {
    assert_schema_matches_golden("StrategyOutcome", schema_for!(StrategyOutcome));
}

#[test]
fn strategy_get_input_schema_stable() {
    assert_schema_matches_golden("StrategyGetInput", schema_for!(StrategyGetInput));
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

#[test]
fn run_status_schema_stable() {
    assert_schema_matches_golden("RunStatus", schema_for!(RunStatus));
}

#[test]
fn strategy_register_response_schema_stable() {
    assert_schema_matches_golden(
        "StrategyRegisterResponse",
        schema_for!(StrategyRegisterResponse),
    );
}

#[test]
fn strategy_list_response_schema_stable() {
    assert_schema_matches_golden("StrategyListResponse", schema_for!(StrategyListResponse));
}

#[test]
fn strategy_get_response_schema_stable() {
    assert_schema_matches_golden("StrategyGetResponse", schema_for!(StrategyGetResponse));
}

#[test]
fn strategy_delete_response_schema_stable() {
    assert_schema_matches_golden(
        "StrategyDeleteResponse",
        schema_for!(StrategyDeleteResponse),
    );
}

#[test]
fn execution_get_response_schema_stable() {
    assert_schema_matches_golden("ExecutionGetResponse", schema_for!(ExecutionGetResponse));
}

#[test]
fn journal_action_outcome_schema_stable() {
    assert_schema_matches_golden(
        "JournalActionOutcome",
        schema_for!(JournalActionOutcome),
    );
}

/// D-06 future-lock: golden must enumerate all 6 wire names so Phase 5
/// (`simulation_failure`, `policy_denied`) cannot regress the contract.
///
/// schemars 1.x emits `oneOf:[{enum:[…]}, {const:…}, …]` rather than a flat
/// `enum[]`. Walk both shapes (mirrors 02-03 SUMMARY:39 walker pattern).
#[test]
fn journal_action_outcome_includes_future_variants() {
    let raw = std::fs::read_to_string("tests/schemas/JournalActionOutcome.json")
        .expect("read JournalActionOutcome.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse golden");

    let mut found: BTreeSet<String> = BTreeSet::new();
    fn walk(v: &serde_json::Value, found: &mut BTreeSet<String>) {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Array(arr)) = map.get("enum") {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            found.insert(s.to_string());
                        }
                    }
                }
                if let Some(serde_json::Value::String(s)) = map.get("const") {
                    found.insert(s.clone());
                }
                for (_k, child) in map {
                    walk(child, found);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    walk(child, found);
                }
            }
            _ => {}
        }
    }
    walk(&v, &mut found);

    let expected: BTreeSet<String> = [
        "noop",
        "actions",
        "validation_error",
        "runtime_error",
        "simulation_failure",
        "policy_denied",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        found, expected,
        "JournalActionOutcome golden must enumerate all 6 future-locked wire names"
    );
}
