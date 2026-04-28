//! Structured MCP errors.
//!
//! Phase 1 ships `unimplemented_err(tool, phase)` so the still-phase-gated
//! tools (`strategy_run_once`, `policy_update`) return a uniform,
//! machine-readable shape instead of a free-form error string. Agents key off
//! `data.code == "unimplemented"` + `data.phase` + `data.tool` to plan
//! follow-up work.
//!
//! Phase 2 adds storage-layer error mapping + validation helpers:
//! - `STORAGE_NOT_FOUND` (-32014) — strategy / run miss
//! - `STORAGE_NAME_CONFLICT` (-32015) — `strategy_register` active-name collision
//! - `STORAGE_ERROR` (-32016) — generic SQLite / I/O failure
//! - `INVALID_PARAMS` (-32602) — JSON-RPC standard, used for D-09 violations
//!
//! Wire code: **-32010** (unimplemented) verified against rmcp 1.5
//! `model::ErrorCode(pub i32)` tuple struct — cf. RESEARCH.md RESOLVED #4.

use executor_evm::{EvmError, SimulationFailReason};
use executor_state::StateError;
use rmcp::{ErrorData as McpError, model::ErrorCode};
use serde_json::json;
use strategy_js::RuntimeError;

/// JSON-RPC 2.0 server-defined range: `-32000..-32099`.
/// We carve out `-32010` for "unimplemented feature".
const UNIMPLEMENTED_CODE: ErrorCode = ErrorCode(-32010);

/// Storage-layer: resource not found (strategy, run).
pub const STORAGE_NOT_FOUND: ErrorCode = ErrorCode(-32014);
/// Storage-layer: active name collision on `strategy_register` with different source.
pub const STORAGE_NAME_CONFLICT: ErrorCode = ErrorCode(-32015);
/// Storage-layer: generic SQLite / I/O failure.
pub const STORAGE_ERROR: ErrorCode = ErrorCode(-32016);
/// JSON-RPC 2.0 standard "Invalid params" — used for D-09 validation failures.
pub const INVALID_PARAMS: ErrorCode = ErrorCode(-32602);

// ─────────── Phase 3 (D-07) ───────────

/// Strategy is soft-deleted (`strategy_run` short-circuit before insert_run).
pub const STRATEGY_DELETED: ErrorCode = ErrorCode(-32011);
/// Sandbox-level failure: timeout / OOM / stack_overflow / exception / engine_init.
pub const STRATEGY_RUNTIME_ERROR: ErrorCode = ErrorCode(-32017);
/// Strategy returned a value that is not `"noop"` / `Action[]`,
/// returned a Promise (D-10), or violated Shape B (D-05).
pub const STRATEGY_INVALID_OUTPUT: ErrorCode = ErrorCode(-32018);

/// Build a `STRATEGY_DELETED` (-32011) error. Carries `data.code = "strategy_deleted"`
/// and `data.strategy_id` so agents can re-register before retrying.
pub fn strategy_deleted(strategy_id: &str) -> McpError {
    McpError::new(
        STRATEGY_DELETED,
        format!("strategy {strategy_id} is soft-deleted; cannot run"),
        Some(json!({
            "code": "strategy_deleted",
            "strategy_id": strategy_id,
        })),
    )
}

/// Build a `STRATEGY_RUNTIME_ERROR` (-32017) error with a typed `data.kind`
/// so agents can dispatch on the runtime failure mode without parsing the
/// free-form message.
pub fn strategy_runtime_error(
    kind: &'static str,
    detail: impl Into<String>,
    run_id: &str,
) -> McpError {
    let detail = detail.into();
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        format!("strategy runtime error ({kind}): {detail}"),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": kind,
            "detail": detail,
            "run_id": run_id,
        })),
    )
}

/// Build a `STRATEGY_INVALID_OUTPUT` (-32018) error. `data.detail` carries
/// the rejection reason; `data.run_id` references the run row whose journal
/// captured the validation failure.
pub fn strategy_invalid_output(detail: impl Into<String>, run_id: &str) -> McpError {
    let detail = detail.into();
    McpError::new(
        STRATEGY_INVALID_OUTPUT,
        format!("strategy invalid output: {detail}"),
        Some(json!({
            "code": "strategy_invalid_output",
            "detail": detail,
            "run_id": run_id,
        })),
    )
}

/// Classify a [`RuntimeError`] into the appropriate MCP error envelope.
/// `InvalidOutput` becomes `-32018`; everything else becomes `-32017` with
/// a typed `data.kind` field for agent dispatch.
///
/// `EngineInit` failures are rare host-level problems (rquickjs Runtime
/// construction failed); we surface them as `kind = "exception"` so agents
/// only need to handle four runtime-error kinds (`timeout`, `oom`,
/// `stack_overflow`, `exception`).
pub fn map_runtime_error(e: RuntimeError, run_id: &str) -> McpError {
    match e {
        RuntimeError::Timeout => {
            strategy_runtime_error("timeout", "wall-clock budget exceeded", run_id)
        }
        RuntimeError::Oom => strategy_runtime_error("oom", "heap budget exceeded", run_id),
        RuntimeError::StackOverflow => {
            strategy_runtime_error("stack_overflow", "max stack size exceeded", run_id)
        }
        RuntimeError::Exception(msg) => strategy_runtime_error("exception", msg, run_id),
        RuntimeError::EngineInit(msg) => {
            strategy_runtime_error("exception", format!("engine init: {msg}"), run_id)
        }
        RuntimeError::InvalidOutput { detail } => strategy_invalid_output(detail, run_id),
        // Phase 4 D-12: EVM errors get the extended data.kind taxonomy and
        // wire-safe stable strings (HR/MR-01 carry-forward).
        RuntimeError::Evm(evm_err) => map_evm_error(evm_err, run_id),
    }
}

/// Map an [`executor_evm::EvmError`] onto a `STRATEGY_RUNTIME_ERROR (-32017)`
/// with the Phase 4 D-12 `data.kind` taxonomy
/// (`evm_rpc_error` / `evm_decode_error` / `evm_revert`). Raw alloy /
/// reqwest text NEVER reaches the wire — it goes to `tracing::warn!`
/// (mirrors the Phase-3 `map_state_error` storage_error pattern at
/// `errors.rs:170`; carries forward HR/MR-01).
pub fn map_evm_error(e: EvmError, run_id: &str) -> McpError {
    let kind = e.data_kind();
    let detail_log = e.detail_for_log().to_string();
    // EvmError::Display is wire-safe (Phase 4 D-12) — the typed taxonomy
    // strings live in the per-variant Display impls.
    let stable = e.to_string();
    tracing::warn!(detail = %detail_log, kind = %kind, run_id = %run_id, "evm error");
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        stable.clone(),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": kind,
            "detail": stable,
            "run_id": run_id,
        })),
    )
}

/// Phase 5 D-08 — emit `-32017 STRATEGY_RUNTIME_ERROR` with `data.kind =
/// "simulation_failure"` for a failed `simulate_one` outcome (per Phase 4
/// D-12 reuse precedent — no new wire codes).
///
/// Wire shape (LOCKED — Plan 05-04 stdio test pins this):
/// - `error.code` = `-32017` STRATEGY_RUNTIME_ERROR.
/// - `data.code` = `"strategy_runtime_error"` (string canon).
/// - `data.kind` = `"simulation_failure"` (Phase 5 D-08; distinguishes from
///   `"exception"` / `"timeout"` / `"evm_*"` so agents can dispatch).
/// - `data.fail_reason` ∈ `{"revert", "transport", "timeout"}`.
/// - `data.action_index`: zero-based index of the failing action in the
///   strategy's action array.
/// - `data.decoded_revert`: SANITIZED revert reason (string) or `null`. The
///   sanitizer ([`executor_evm::read::sanitize_revert_reason`], WR-04) runs
///   at `simulate_one` BEFORE this factory sees the data — attacker-controllable
///   text is the only string that may survive to wire and only after
///   sanitization (control-char strip + 256-byte cap).
/// - `data.detail` mirrors `error.message` and starts with `"simulation failed: "`.
/// - `data.run_id`: the run row whose journal captured the denial.
///
/// MR-01 carry-forward: NO raw alloy / reqwest text reaches the wire. The
/// `SimulationOutcome::Fail::raw_for_log` field is consumed by `tracing::warn!`
/// at the simulate site; this factory only sees the typed `SimulationFailReason`.
pub fn map_simulation_error(
    reason: &SimulationFailReason,
    action_index: u32,
    run_id: &str,
) -> McpError {
    let (fail_reason, decoded_revert) = match reason {
        SimulationFailReason::Revert { decoded } => ("revert", decoded.clone()),
        SimulationFailReason::Transport => ("transport", None),
        SimulationFailReason::Timeout => ("timeout", None),
    };
    let detail = match (fail_reason, decoded_revert.as_deref()) {
        ("revert", Some(d)) => format!("simulation failed: evm revert: {d}"),
        ("revert", None) => "simulation failed: evm revert: unknown".to_string(),
        ("transport", _) => "simulation failed: evm rpc error: transport".to_string(),
        ("timeout", _) => "simulation failed: evm rpc error: timeout".to_string(),
        // The match above is exhaustive — `fail_reason` is one of three
        // const string literals. The compiler can't prove this, so use a
        // safe fallback rather than `unreachable!()`.
        _ => "simulation failed".to_string(),
    };
    tracing::warn!(
        action_index,
        run_id,
        fail_reason,
        "simulation denial",
    );
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        detail.clone(),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": "simulation_failure",
            "fail_reason": fail_reason,
            "action_index": action_index,
            "decoded_revert": decoded_revert,
            "detail": detail,
            "run_id": run_id,
        })),
    )
}

/// Build an `unimplemented` error for `tool_name`, pointing agents at the
/// phase where it will land.
pub fn unimplemented_err(tool_name: &'static str, phase: u8) -> McpError {
    McpError::new(
        UNIMPLEMENTED_CODE,
        format!("{tool_name} is not implemented yet (lands in Phase {phase})"),
        Some(json!({
            "code": "unimplemented",
            "tool": tool_name,
            "phase": phase,
            "hint": format!("will be implemented when Phase {phase} lands"),
        })),
    )
}

/// Map a storage-layer [`StateError`] to its MCP wire code + structured `data`
/// payload. Agents key off `data.code` (string) for stable matching; the
/// numeric `error.code` is provided for JSON-RPC clients.
pub fn map_state_error(e: StateError) -> McpError {
    match e {
        StateError::NotFound(what) => McpError::new(
            STORAGE_NOT_FOUND,
            format!("not found: {what}"),
            Some(json!({ "code": "not_found", "resource": what })),
        ),
        StateError::NameConflict {
            attempted_name,
            existing_strategy_id,
            existing_source_hash: _,
            existing_created_at,
        } => McpError::new(
            STORAGE_NAME_CONFLICT,
            format!(
                "strategy name '{attempted_name}' already used by strategy_id={existing_strategy_id} \
                 (created {existing_created_at}); soft-delete that strategy to reuse the name, \
                 or choose a different name"
            ),
            Some(json!({
                "code": "name_conflict",
                "attempted_name": attempted_name,
                "existing_strategy_id": existing_strategy_id,
                "existing_created_at": existing_created_at,
            })),
        ),
        StateError::InvalidInput(msg) => invalid_params(msg),
        StateError::SerializationError(msg) => {
            // MR-03: serde failure on a journal payload. Same wire-leak
            // discipline as Storage — raw text to tracing, stable taxonomy
            // string on the wire.
            tracing::warn!(detail = %msg, "journal payload serialization failed");
            McpError::new(
                STORAGE_ERROR,
                "journal payload serialization failed".to_string(),
                Some(json!({
                    "code": "storage_error",
                    "detail": "journal payload serialization failed",
                })),
            )
        }
        StateError::Storage(msg) => {
            // MR-01: Do NOT echo raw rusqlite text (constraint names, table
            // names, SQLite-internal phrasing) onto the wire — it leaks
            // schema details. Route the raw text to `tracing::warn!` for
            // operator forensics, and surface a stable taxonomy string in
            // `data.detail` so agent dispatch on `data.code == "storage_error"`
            // remains robust.
            tracing::warn!(detail = %msg, "storage error");
            McpError::new(
                STORAGE_ERROR,
                "storage backend error".to_string(),
                Some(json!({ "code": "storage_error", "detail": "storage backend error" })),
            )
        }
    }
}

/// Build an `invalid_params` error (JSON-RPC -32602) from a free-form message.
pub fn invalid_params(msg: impl Into<String>) -> McpError {
    let msg = msg.into();
    McpError::new(
        INVALID_PARAMS,
        msg.clone(),
        Some(json!({ "code": "invalid_params", "detail": msg })),
    )
}

/// Build a generic `storage_error` (-32016) from a free-form message. Used for
/// non-`StateError` failures (`spawn_blocking` join, JSON serialisation, etc.)
/// so all storage-path errors carry a uniform `data.code == "storage_error"`.
pub fn storage_error(msg: impl Into<String>) -> McpError {
    let msg = msg.into();
    McpError::new(
        STORAGE_ERROR,
        format!("storage error: {msg}"),
        Some(json!({ "code": "storage_error", "detail": msg })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_structured_data() {
        let e = unimplemented_err("strategy_register", 2);
        assert_eq!(e.code, UNIMPLEMENTED_CODE);
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "unimplemented");
        assert_eq!(data["tool"], "strategy_register");
        assert_eq!(data["phase"], 2);
    }

    #[test]
    fn map_state_error_not_found_uses_32014() {
        let e = map_state_error(StateError::NotFound("foo".into()));
        assert_eq!(e.code, STORAGE_NOT_FOUND);
        assert_eq!(e.code, ErrorCode(-32014));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "not_found");
        assert_eq!(data["resource"], "foo");
    }

    #[test]
    fn map_state_error_name_conflict_carries_existing_fields() {
        let e = map_state_error(StateError::NameConflict {
            attempted_name: "arb".into(),
            existing_strategy_id: "abc".into(),
            existing_source_hash: "abc".into(),
            existing_created_at: "2026-01-01T00:00:00Z".into(),
        });
        assert_eq!(e.code, STORAGE_NAME_CONFLICT);
        assert_eq!(e.code, ErrorCode(-32015));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "name_conflict");
        assert_eq!(data["attempted_name"], "arb");
        assert_eq!(data["existing_strategy_id"], "abc");
        assert_eq!(data["existing_created_at"], "2026-01-01T00:00:00Z");
        assert!(
            e.message.contains("strategy name 'arb' already used by strategy_id=abc"),
            "message missing canonical phrase: {}",
            e.message
        );
    }

    #[test]
    fn map_state_error_storage_uses_32016() {
        let e = map_state_error(StateError::Storage("boom".into()));
        assert_eq!(e.code, STORAGE_ERROR);
        assert_eq!(e.code, ErrorCode(-32016));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "storage_error");
        // MR-01: raw rusqlite text MUST NOT appear on the wire. The detail
        // is a stable taxonomy string; the raw text goes to tracing only.
        assert!(
            !e.message.contains("boom"),
            "raw rusqlite text leaked to wire: {}",
            e.message
        );
        assert!(
            !data["detail"].as_str().unwrap_or("").contains("boom"),
            "raw rusqlite text leaked to data.detail: {}",
            data["detail"]
        );
        assert_eq!(data["detail"], "storage backend error");
    }

    #[test]
    fn strategy_deleted_uses_32011() {
        let e = strategy_deleted("abc");
        assert_eq!(e.code, STRATEGY_DELETED);
        assert_eq!(e.code, ErrorCode(-32011));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_deleted");
        assert_eq!(data["strategy_id"], "abc");
    }

    #[test]
    fn strategy_invalid_output_uses_32018_carries_run_id_and_detail() {
        let e = strategy_invalid_output("got number", "01ARZ123");
        assert_eq!(e.code, STRATEGY_INVALID_OUTPUT);
        assert_eq!(e.code, ErrorCode(-32018));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_invalid_output");
        assert_eq!(data["detail"], "got number");
        assert_eq!(data["run_id"], "01ARZ123");
    }

    #[test]
    fn strategy_runtime_error_timeout_uses_32017_with_kind_timeout() {
        let e = strategy_runtime_error("timeout", "wall clock", "01ARZ123");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.code, ErrorCode(-32017));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_runtime_error");
        assert_eq!(data["kind"], "timeout");
        assert_eq!(data["detail"], "wall clock");
        assert_eq!(data["run_id"], "01ARZ123");
    }

    #[test]
    fn map_runtime_error_classifies_each_variant() {
        // Timeout
        let e = map_runtime_error(RuntimeError::Timeout, "rid");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["kind"], "timeout");
        // Oom
        let e = map_runtime_error(RuntimeError::Oom, "rid");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["kind"], "oom");
        // StackOverflow
        let e = map_runtime_error(RuntimeError::StackOverflow, "rid");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["kind"], "stack_overflow");
        // Exception
        let e = map_runtime_error(RuntimeError::Exception("boom".into()), "rid");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["kind"], "exception");
        assert_eq!(e.data.as_ref().unwrap()["detail"], "boom");
        // EngineInit → mapped to "exception"
        let e = map_runtime_error(RuntimeError::EngineInit("rt fail".into()), "rid");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["kind"], "exception");
        assert!(
            e.data
                .as_ref()
                .unwrap()["detail"]
                .as_str()
                .is_some_and(|s| s.contains("engine init"))
        );
        // InvalidOutput → -32018, no kind
        let e = map_runtime_error(
            RuntimeError::InvalidOutput {
                detail: "promise return".into(),
            },
            "rid",
        );
        assert_eq!(e.code, STRATEGY_INVALID_OUTPUT);
        assert_eq!(e.data.as_ref().unwrap()["code"], "strategy_invalid_output");
        assert_eq!(e.data.as_ref().unwrap()["detail"], "promise return");
        assert_eq!(e.data.as_ref().unwrap()["run_id"], "rid");
    }

    #[test]
    fn map_runtime_error_classifies_evm_kinds() {
        // Transport → evm_rpc_error
        let e = map_runtime_error(
            RuntimeError::Evm(EvmError::Transport {
                detail_for_log: "Reqwest::Error(boom)".into(),
            }),
            "rid",
        );
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        let data = e.data.as_ref().expect("data");
        assert_eq!(data["kind"], "evm_rpc_error");
        assert_eq!(data["detail"], "evm rpc error: transport");
        // MR-01: NO raw alloy/transport text on the wire.
        assert!(!e.message.contains("Reqwest"));
        assert!(!data["detail"].as_str().unwrap().contains("Reqwest"));
        assert!(!data["detail"].as_str().unwrap().contains("boom"));

        // Decode → evm_decode_error
        let e = map_runtime_error(
            RuntimeError::Evm(EvmError::Decode {
                category: std::borrow::Cow::Borrowed("abi_decode_output"),
                detail_for_log: "alloy_dyn_abi::Error::TypeMismatch".into(),
            }),
            "rid",
        );
        assert_eq!(e.data.as_ref().unwrap()["kind"], "evm_decode_error");
        assert_eq!(
            e.data.as_ref().unwrap()["detail"],
            "evm decode error: abi_decode_output"
        );
        assert!(!e.message.contains("alloy_dyn_abi"));

        // Revert → evm_revert (decoded reason on wire, raw bytes only in log)
        let e = map_runtime_error(
            RuntimeError::Evm(EvmError::Revert {
                reason: "ERC20: insufficient balance".into(),
                detail_for_log: "0x08c379a0...".into(),
            }),
            "rid",
        );
        assert_eq!(e.data.as_ref().unwrap()["kind"], "evm_revert");
        assert_eq!(
            e.data.as_ref().unwrap()["detail"],
            "evm revert: ERC20: insufficient balance"
        );
        assert!(!e.message.contains("0x08c379a0"));

        // Timeout → evm_rpc_error
        let e = map_runtime_error(RuntimeError::Evm(EvmError::Timeout), "rid");
        assert_eq!(e.data.as_ref().unwrap()["kind"], "evm_rpc_error");
        assert_eq!(e.data.as_ref().unwrap()["detail"], "evm rpc error: timeout");

        // Encode (not on the public taxonomy boundary, but flows through
        // map_runtime_error too) → evm_decode_error
        let e = map_runtime_error(
            RuntimeError::Evm(EvmError::Encode {
                category: std::borrow::Cow::Borrowed("type_mismatch"),
                detail_for_log: "raw".into(),
            }),
            "rid",
        );
        assert_eq!(e.data.as_ref().unwrap()["kind"], "evm_decode_error");
    }

    #[test]
    fn map_state_error_invalid_input_becomes_invalid_params() {
        let e = map_state_error(StateError::InvalidInput(
            "source size 500000 exceeds 262144".into(),
        ));
        assert_eq!(e.code, INVALID_PARAMS);
        assert_eq!(e.code, ErrorCode(-32602));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "invalid_params");
        assert!(
            e.message.contains("source size 500000 exceeds 262144"),
            "message not preserved: {}",
            e.message
        );
    }

    // ─────────── Phase 5 Plan 05-02 / D-08 simulation_failure ───────────

    #[test]
    fn map_simulation_error_for_revert_emits_simulation_failure_kind() {
        let e = map_simulation_error(
            &SimulationFailReason::Revert {
                decoded: Some("ERC20: insufficient balance".into()),
            },
            0,
            "01ARZ123",
        );
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.code, ErrorCode(-32017));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_runtime_error");
        assert_eq!(data["kind"], "simulation_failure");
        assert_eq!(data["fail_reason"], "revert");
        assert_eq!(data["action_index"], 0);
        assert_eq!(data["decoded_revert"], "ERC20: insufficient balance");
        assert_eq!(data["run_id"], "01ARZ123");
        assert!(
            data["detail"]
                .as_str()
                .unwrap()
                .starts_with("simulation failed: evm revert: "),
            "detail missing canonical prefix: {}",
            data["detail"]
        );
    }

    #[test]
    fn map_simulation_error_for_revert_with_no_decoded_uses_unknown() {
        let e = map_simulation_error(
            &SimulationFailReason::Revert { decoded: None },
            7,
            "01ARZ123",
        );
        let data = e.data.as_ref().expect("data");
        assert_eq!(data["fail_reason"], "revert");
        assert_eq!(data["action_index"], 7);
        assert_eq!(data["decoded_revert"], serde_json::Value::Null);
        assert_eq!(data["detail"], "simulation failed: evm revert: unknown");
    }

    #[test]
    fn map_simulation_error_for_transport_emits_transport_fail_reason() {
        let e = map_simulation_error(&SimulationFailReason::Transport, 2, "01ARZ123");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        let data = e.data.as_ref().expect("data");
        assert_eq!(data["kind"], "simulation_failure");
        assert_eq!(data["fail_reason"], "transport");
        assert_eq!(data["action_index"], 2);
        assert_eq!(data["decoded_revert"], serde_json::Value::Null);
        assert_eq!(data["detail"], "simulation failed: evm rpc error: transport");
    }

    #[test]
    fn map_simulation_error_for_timeout_emits_timeout_fail_reason() {
        let e = map_simulation_error(&SimulationFailReason::Timeout, 5, "01ARZ123");
        let data = e.data.as_ref().expect("data");
        assert_eq!(data["kind"], "simulation_failure");
        assert_eq!(data["fail_reason"], "timeout");
        assert_eq!(data["action_index"], 5);
        assert_eq!(data["detail"], "simulation failed: evm rpc error: timeout");
    }

    #[test]
    fn map_simulation_error_does_not_leak_raw_alloy_text() {
        // MR-01: even with a benign decoded revert, no top-level field
        // carries raw alloy crate names. The factory never sees raw_for_log.
        let e = map_simulation_error(
            &SimulationFailReason::Revert {
                decoded: Some("benign reason".into()),
            },
            0,
            "01ARZ123",
        );
        let data = e.data.as_ref().expect("data");
        let s = serde_json::to_string(data).unwrap();
        assert!(!s.contains("TransportError"), "raw alloy text leaked: {s}");
        assert!(!s.contains("Reqwest"), "reqwest text leaked: {s}");
        assert!(!s.contains("ErrorResp"), "alloy ErrorResp leaked: {s}");
    }
}
