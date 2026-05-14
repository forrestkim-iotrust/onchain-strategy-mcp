//! v1.11 Track A — `data.hint` contract test for every typed error
//! constructor exported from `executor_mcp::errors`.
//!
//! v1.4 §8 promised: "Every error response carries `{kind, message, hint}`
//! where `hint` names a concrete next call. Empty hint is a v1.4 lint
//! failure." This test enforces that promise structurally — every typed
//! constructor below MUST emit `data.hint` as a non-empty string.
//!
//! Adding a new typed constructor to `errors.rs` without adding it here is
//! the intended failure mode: the new constructor stays unwitnessed until a
//! row is added below.

use std::borrow::Cow;

use executor_evm::{EvmError, SimulationFailReason};
use executor_mcp::errors::{
    invalid_params, lineage_no_active_version, lineage_not_found, malformed_lineage_id,
    malformed_run_id, malformed_strategy_id, malformed_strategy_name, map_evm_error,
    map_policy_error, map_runtime_error, map_simulation_error, map_state_error,
    policy_not_loaded, run_not_found, storage_error, strategy_by_name_not_found,
    strategy_deleted, strategy_invalid_output, strategy_not_found, strategy_runtime_error,
    trigger_not_found, unimplemented_err, unknown_action, unknown_embedded_resource,
    unsupported_resource_uri,
};
use executor_policy::DecisionVerdict;
use executor_state::StateError;
use rmcp::ErrorData as McpError;
use strategy_js::RuntimeError;

/// Asserts the structured-error contract for a single constructor result.
///
/// - `data` is present (every typed error carries structured data)
/// - `data.hint` is a non-empty string (v1.4 §8 promise)
/// - `data.kind` is a non-empty string (agents dispatch on `kind`)
#[track_caller]
fn assert_hint_contract(label: &str, err: &McpError) {
    let data = err
        .data
        .as_ref()
        .unwrap_or_else(|| panic!("{label}: data must be present"));
    let hint = data["hint"].as_str().unwrap_or_else(|| {
        panic!("{label}: data.hint must be a JSON string, got {data}")
    });
    assert!(
        !hint.is_empty(),
        "{label}: data.hint must be non-empty (v1.4 §8): {data}"
    );
    let kind = data["kind"].as_str().unwrap_or_else(|| {
        panic!("{label}: data.kind must be a JSON string, got {data}")
    });
    assert!(
        !kind.is_empty(),
        "{label}: data.kind must be non-empty: {data}"
    );
}

#[test]
fn every_typed_constructor_carries_non_empty_hint() {
    // ───── caller-supplied-hint constructors ─────
    assert_hint_contract(
        "unimplemented_err",
        &unimplemented_err("foo_tool", 9, "see docs://roadmap"),
    );
    assert_hint_contract(
        "unknown_action",
        &unknown_action(
            "00".repeat(32).as_str(),
            "bogus",
            &["execute".into(), "rebalance".into()],
            "list available actions via strategy://{id}",
        ),
    );
    assert_hint_contract(
        "strategy_deleted",
        &strategy_deleted("abc", "re-register the strategy first"),
    );
    assert_hint_contract(
        "strategy_runtime_error",
        &strategy_runtime_error("exception", "boom", "rid", "fix the JS exception"),
    );
    assert_hint_contract(
        "strategy_invalid_output",
        &strategy_invalid_output("got promise", "rid", "return Action[] or \"noop\""),
    );

    // ───── self-deriving-hint constructors ─────
    assert_hint_contract("invalid_params (generic)", &invalid_params("bad input"));
    assert_hint_contract(
        "invalid_params (address-specialized)",
        &invalid_params("expected address but got abcd"),
    );
    assert_hint_contract("storage_error", &storage_error("disk full"));
    assert_hint_contract("policy_not_loaded", &policy_not_loaded("rid"));

    // map_runtime_error covers every RuntimeError variant.
    assert_hint_contract(
        "map_runtime_error/Timeout",
        &map_runtime_error(RuntimeError::Timeout, "rid"),
    );
    assert_hint_contract(
        "map_runtime_error/Oom",
        &map_runtime_error(RuntimeError::Oom, "rid"),
    );
    assert_hint_contract(
        "map_runtime_error/StackOverflow",
        &map_runtime_error(RuntimeError::StackOverflow, "rid"),
    );
    assert_hint_contract(
        "map_runtime_error/Exception",
        &map_runtime_error(RuntimeError::Exception("boom".into()), "rid"),
    );
    assert_hint_contract(
        "map_runtime_error/EngineInit",
        &map_runtime_error(RuntimeError::EngineInit("rt fail".into()), "rid"),
    );
    assert_hint_contract(
        "map_runtime_error/InvalidOutput",
        &map_runtime_error(
            RuntimeError::InvalidOutput {
                detail: "got number".into(),
            },
            "rid",
        ),
    );

    // map_evm_error directly (also exercised by RuntimeError::Evm).
    assert_hint_contract(
        "map_evm_error/Transport",
        &map_evm_error(
            EvmError::Transport {
                detail_for_log: "Reqwest::Error(boom)".into(),
            },
            "rid",
        ),
    );
    assert_hint_contract(
        "map_evm_error/Decode",
        &map_evm_error(
            EvmError::Decode {
                category: Cow::Borrowed("abi_decode_output"),
                detail_for_log: "type mismatch".into(),
            },
            "rid",
        ),
    );
    assert_hint_contract(
        "map_evm_error/Revert",
        &map_evm_error(
            EvmError::Revert {
                reason: "ERC20: insufficient balance".into(),
                detail_for_log: "0x08c379a0...".into(),
            },
            "rid",
        ),
    );
    assert_hint_contract(
        "map_evm_error/Timeout",
        &map_evm_error(EvmError::Timeout, "rid"),
    );
    assert_hint_contract(
        "map_evm_error/Encode",
        &map_evm_error(
            EvmError::Encode {
                category: Cow::Borrowed("type_mismatch"),
                detail_for_log: "raw".into(),
            },
            "rid",
        ),
    );

    // map_simulation_error: every fail reason.
    assert_hint_contract(
        "map_simulation_error/Revert",
        &map_simulation_error(
            &SimulationFailReason::Revert {
                decoded: Some("ERC20: insufficient balance".into()),
            },
            0,
            "rid",
        ),
    );
    assert_hint_contract(
        "map_simulation_error/Transport",
        &map_simulation_error(&SimulationFailReason::Transport, 0, "rid"),
    );
    assert_hint_contract(
        "map_simulation_error/Timeout",
        &map_simulation_error(&SimulationFailReason::Timeout, 0, "rid"),
    );

    // map_policy_error: every rule taxonomy.
    for rule in [
        "chain_not_allowed",
        "contract_not_allowed",
        "selector_not_allowed",
        "native_value_exceeds",
        "erc20_spend_exceeds",
        "raw_call_denied",
    ] {
        let verdict = DecisionVerdict::Deny {
            rule: Cow::Owned(rule.to_string()),
            detail: "stub".into(),
        };
        assert_hint_contract(
            &format!("map_policy_error/{rule}"),
            &map_policy_error(&verdict, 0, "rid"),
        );
    }

    // map_state_error: every variant.
    assert_hint_contract(
        "map_state_error/NotFound(strategy)",
        &map_state_error(StateError::NotFound("strategy abc".into())),
    );
    assert_hint_contract(
        "map_state_error/NotFound(run)",
        &map_state_error(StateError::NotFound("run xyz".into())),
    );
    assert_hint_contract(
        "map_state_error/NotFound(trigger)",
        &map_state_error(StateError::NotFound("trigger xyz".into())),
    );
    assert_hint_contract(
        "map_state_error/NotFound(opaque)",
        &map_state_error(StateError::NotFound("frobnicator 9".into())),
    );
    assert_hint_contract(
        "map_state_error/NameConflict",
        &map_state_error(StateError::NameConflict {
            attempted_name: "arb".into(),
            existing_strategy_id: "abc".into(),
            existing_source_hash: "h".into(),
            existing_created_at: "2026-01-01T00:00:00Z".into(),
        }),
    );
    assert_hint_contract(
        "map_state_error/InvalidInput",
        &map_state_error(StateError::InvalidInput("source too big".into())),
    );
    assert_hint_contract(
        "map_state_error/SerializationError",
        &map_state_error(StateError::SerializationError("encode bug".into())),
    );
    assert_hint_contract(
        "map_state_error/Storage",
        &map_state_error(StateError::Storage("sqlite boom".into())),
    );

    // ───── v1.11 Track A new resource_not_found constructors ─────
    assert_hint_contract(
        "malformed_strategy_id",
        &malformed_strategy_id("strategy://bogus"),
    );
    assert_hint_contract("malformed_run_id", &malformed_run_id("journal://bogus"));
    assert_hint_contract(
        "malformed_lineage_id",
        &malformed_lineage_id("strategy://lineage/"),
    );
    assert_hint_contract(
        "malformed_strategy_name",
        &malformed_strategy_name("strategy://by-name/"),
    );
    assert_hint_contract(
        "strategy_not_found",
        &strategy_not_found(&format!("strategy://{}", "0".repeat(64))),
    );
    assert_hint_contract(
        "strategy_by_name_not_found",
        &strategy_by_name_not_found("strategy://by-name/foo", "foo"),
    );
    assert_hint_contract(
        "run_not_found",
        &run_not_found("execution://01ARZ3NDEKTSV4RRFFQ69G5FAV"),
    );
    assert_hint_contract("trigger_not_found", &trigger_not_found("trigger://bogus"));
    assert_hint_contract(
        "lineage_no_active_version",
        &lineage_no_active_version("strategy://lineage/foo", "foo"),
    );
    assert_hint_contract(
        "lineage_not_found",
        &lineage_not_found("strategy://lineage/foo/history", "foo"),
    );
    assert_hint_contract(
        "unknown_embedded_resource",
        &unknown_embedded_resource("examples://strategies/bogus", &["yield-snapshot", "eth-funnel"]),
    );
    assert_hint_contract(
        "unsupported_resource_uri",
        &unsupported_resource_uri("foo://bar"),
    );
}

// ─────────── Targeted assertions for the v1.11 Track A regression ───────────
//
// These two errors are the headline v1.11 regression case: v1.4 §8 promised
// `journal://malformed` returns a hint pointing at `execution://list`, but
// the direct `McpError::resource_not_found` call site shipped an empty hint.
// The constructors now MUST surface the recovery URI in the hint text.

#[test]
fn malformed_run_id_hint_mentions_execution_list() {
    let err = malformed_run_id("journal://bogus");
    let hint = err.data.as_ref().unwrap()["hint"].as_str().unwrap();
    assert!(
        hint.contains("execution://list"),
        "malformed_run_id hint must reference execution://list so the agent \
         can recover in one hop: {hint}"
    );
}

#[test]
fn malformed_strategy_id_hint_mentions_strategy_list() {
    let err = malformed_strategy_id("strategy://bogus");
    let hint = err.data.as_ref().unwrap()["hint"].as_str().unwrap();
    assert!(
        hint.contains("strategy://list"),
        "malformed_strategy_id hint must reference strategy://list so the \
         agent can recover in one hop: {hint}"
    );
}
