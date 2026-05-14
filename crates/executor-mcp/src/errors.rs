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
//!
//! ## v1.4 Track D — `hint` field
//!
//! Every constructor accepts a `hint: impl Into<String>` parameter as its
//! LAST argument. The hint is embedded into the `ErrorData.data` JSON as
//! `data.hint` and names a concrete next tool call or URI the agent can use
//! to recover. Empty hints panic in debug builds (`require_hint`).

use executor_evm::{EvmError, SimulationFailReason};
use executor_policy::DecisionVerdict;
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

// ─────────── v1.4 Track D ───────────

/// v1.4 Track D — every error MUST carry a non-empty `hint`. Empty hint
/// panics in debug builds and logs at `warn` in release so the lint failure
/// is loud during development while still failing-open in production.
pub(crate) fn require_hint(s: String) -> String {
    debug_assert!(
        !s.is_empty(),
        "v1.4: every error needs a hint — empty hints violate the agent-UX honesty contract"
    );
    if s.is_empty() {
        tracing::warn!(
            "v1.4 lint: empty hint reached the wire; agent UX contract violated"
        );
    }
    s
}

/// Common "recoverable next action" hints. Centralized so call sites stay
/// consistent and a code audit can grep them.
pub mod hints {
    pub const STRATEGY_LIST: &str =
        "list active strategies via strategy://list?status=active";
    pub const STRATEGY_LIST_DELETED: &str =
        "list including deleted via strategy://list?status=all";
    pub const TRIGGER_LIST: &str = "list triggers via trigger://list";
    pub const POLICY_DOCS: &str =
        "create .local/policy.toml — see docs://policy-model";
    pub const STRATEGY_BUNDLE_DOCS: &str = "see docs://strategy-bundle";
}

/// Build a `STRATEGY_DELETED` (-32011) error. Carries `data.code = "strategy_deleted"`
/// and `data.strategy_id` so agents can re-register before retrying.
pub fn strategy_deleted(strategy_id: &str, hint: impl Into<String>) -> McpError {
    let hint = require_hint(hint.into());
    McpError::new(
        STRATEGY_DELETED,
        format!("strategy {strategy_id} is soft-deleted; cannot run"),
        Some(json!({
            "code": "strategy_deleted",
            "kind": "strategy_deleted",
            "strategy_id": strategy_id,
            "hint": hint,
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
    hint: impl Into<String>,
) -> McpError {
    let detail = detail.into();
    let hint = require_hint(hint.into());
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        format!("strategy runtime error ({kind}): {detail}"),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": kind,
            "detail": detail,
            "run_id": run_id,
            "hint": hint,
        })),
    )
}

/// Build a `STRATEGY_INVALID_OUTPUT` (-32018) error. `data.detail` carries
/// the rejection reason; `data.run_id` references the run row whose journal
/// captured the validation failure.
pub fn strategy_invalid_output(
    detail: impl Into<String>,
    run_id: &str,
    hint: impl Into<String>,
) -> McpError {
    let detail = detail.into();
    let hint = require_hint(hint.into());
    McpError::new(
        STRATEGY_INVALID_OUTPUT,
        format!("strategy invalid output: {detail}"),
        Some(json!({
            "code": "strategy_invalid_output",
            "kind": "strategy_invalid_output",
            "detail": detail,
            "run_id": run_id,
            "hint": hint,
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
        RuntimeError::Timeout => strategy_runtime_error(
            "timeout",
            "wall-clock budget exceeded",
            run_id,
            "review the strategy source for unbounded loops or large allocations; read it via strategy://{strategy_id}",
        ),
        RuntimeError::Oom => strategy_runtime_error(
            "oom",
            "heap budget exceeded",
            run_id,
            "review the strategy for large allocations; read journal://{run_id} for the recorded payload",
        ),
        RuntimeError::StackOverflow => strategy_runtime_error(
            "stack_overflow",
            "max stack size exceeded",
            run_id,
            "remove recursive calls; read the source via strategy://{strategy_id}",
        ),
        RuntimeError::Exception(msg) => strategy_runtime_error(
            "exception",
            msg,
            run_id,
            "fix the JS exception; inspect journal://{run_id} for the recorded detail",
        ),
        RuntimeError::EngineInit(msg) => strategy_runtime_error(
            "exception",
            format!("engine init: {msg}"),
            run_id,
            "rare host-level error; retry strategy_run and check daemon logs",
        ),
        RuntimeError::InvalidOutput { detail } => strategy_invalid_output(
            detail,
            run_id,
            "return either \"noop\" or an Action[]; see docs://strategy-bundle for shapes",
        ),
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
    let hint = require_hint(match kind {
        "evm_rpc_error" => "check [evm].rpc_url in config; see docs://eip-7702",
        "evm_decode_error" => "verify the action ABI and arg shapes match the target contract",
        "evm_revert" => "read journal://{run_id} for the recorded revert reason and adjust the strategy",
        _ => "inspect journal://{run_id} for the recorded payload",
    }.to_string());
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        stable.clone(),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": kind,
            "detail": stable,
            "run_id": run_id,
            "hint": hint,
        })),
    )
}

/// Phase 5 D-08 — emit `-32017 STRATEGY_RUNTIME_ERROR` with `data.kind =
/// "simulation_failure"` for a failed `simulate_one` outcome (per Phase 4
/// D-12 reuse precedent — no new wire codes).
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
        _ => "simulation failed".to_string(),
    };
    tracing::warn!(
        action_index,
        run_id,
        fail_reason,
        "simulation denial",
    );
    let hint = require_hint(match fail_reason {
        "revert" => "fix the on-chain state assumption in the strategy or update args; inspect journal://{run_id}",
        "transport" => "check [evm].rpc_url health and retry; see docs://eip-7702",
        "timeout" => "retry strategy_run when the RPC is healthier",
        _ => "inspect journal://{run_id} for the recorded simulation outcome",
    }.to_string());
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
            "hint": hint,
        })),
    )
}

/// Phase 5 Plan 05-03 / D-08 — emit `-32017 STRATEGY_RUNTIME_ERROR` with
/// `data.kind = "policy_violation"` for a policy denial verdict.
pub fn map_policy_error(
    verdict: &DecisionVerdict,
    action_index: u32,
    run_id: &str,
) -> McpError {
    let (rule, detail_inner) = match verdict {
        DecisionVerdict::Deny { rule, detail } => (rule.clone(), detail.clone()),
        DecisionVerdict::Allow => {
            debug_assert!(
                false,
                "map_policy_error called with Allow — caller must filter"
            );
            (
                std::borrow::Cow::Borrowed("policy_violation"),
                "unexpected Allow verdict reached map_policy_error".to_string(),
            )
        }
    };
    let stable_detail = format!("policy violation: {detail_inner}");
    tracing::warn!(
        action_index,
        run_id,
        rule = %rule.as_ref(),
        "policy denial",
    );
    // Hint references the policy rule + a concrete next action.
    let hint = require_hint(format!(
        "adjust .local/policy.toml to allow this {rule}, or change the strategy; see docs://policy-model and policy://current",
        rule = rule.as_ref(),
    ));
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        stable_detail.clone(),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": "policy_violation",
            "rule": rule.as_ref(),
            "action_index": action_index,
            "detail": stable_detail,
            "run_id": run_id,
            "hint": hint,
        })),
    )
}

/// Phase 5 Plan 05-03 / D-15 — emit `-32017 STRATEGY_RUNTIME_ERROR` with
/// `data.kind = "policy_not_loaded"` when `ExecutorServer.policy` is `None`.
pub fn policy_not_loaded(run_id: &str) -> McpError {
    let detail =
        "policy violation: policy file not loaded — set [policy].path in config".to_string();
    tracing::warn!(run_id, "strategy_run blocked: policy not loaded");
    let hint = require_hint(hints::POLICY_DOCS.to_string());
    McpError::new(
        STRATEGY_RUNTIME_ERROR,
        detail.clone(),
        Some(json!({
            "code": "strategy_runtime_error",
            "kind": "policy_not_loaded",
            "detail": detail,
            "run_id": run_id,
            "hint": hint,
        })),
    )
}

/// Build an `unimplemented` error for `tool_name`, pointing agents at the
/// phase where it will land.
pub fn unimplemented_err(
    tool_name: &'static str,
    phase: u8,
    hint: impl Into<String>,
) -> McpError {
    let hint = require_hint(hint.into());
    McpError::new(
        UNIMPLEMENTED_CODE,
        format!("{tool_name} is not implemented yet (lands in Phase {phase})"),
        Some(json!({
            "code": "unimplemented",
            "kind": "unimplemented",
            "tool": tool_name,
            "phase": phase,
            "hint": hint,
        })),
    )
}

/// Map a storage-layer [`StateError`] to its MCP wire code + structured `data`
/// payload. Agents key off `data.code` (string) for stable matching; the
/// numeric `error.code` is provided for JSON-RPC clients.
///
/// The hint surfaced here is the **generic** state-error hint. Callers that
/// want a more specific hint (e.g. "list triggers via trigger://list" for a
/// trigger NotFound) should classify the error themselves before calling this.
pub fn map_state_error(e: StateError) -> McpError {
    match e {
        StateError::NotFound(what) => {
            // Best-effort: route the hint based on the resource prefix the
            // state layer used. Falls back to strategy://list (the most
            // common case in the agent UX) if the resource string is opaque.
            let lower = what.to_lowercase();
            let hint = if lower.starts_with("strategy") {
                hints::STRATEGY_LIST.to_string()
            } else if lower.starts_with("run") {
                "list recent runs via execution://list".to_string()
            } else if lower.starts_with("trigger") {
                hints::TRIGGER_LIST.to_string()
            } else {
                format!("verify the id and retry; if unknown, list via the matching *://list resource (was: {what})")
            };
            let hint = require_hint(hint);
            McpError::new(
                STORAGE_NOT_FOUND,
                format!("not found: {what}"),
                Some(json!({
                    "code": "not_found",
                    "kind": "not_found",
                    "resource": what,
                    "hint": hint,
                })),
            )
        }
        StateError::NameConflict {
            attempted_name,
            existing_strategy_id,
            existing_source_hash: _,
            existing_created_at,
        } => {
            let hint = require_hint(format!(
                "choose a different name or use the existing strategy_id `{existing_strategy_id}`; soft-delete the existing row with strategy_delete to free the name"
            ));
            McpError::new(
                STORAGE_NAME_CONFLICT,
                format!(
                    "strategy name '{attempted_name}' already used by strategy_id={existing_strategy_id} \
                     (created {existing_created_at}); soft-delete that strategy to reuse the name, \
                     or choose a different name"
                ),
                Some(json!({
                    "code": "name_conflict",
                    "kind": "name_conflict",
                    "attempted_name": attempted_name,
                    "existing_strategy_id": existing_strategy_id,
                    "existing_created_at": existing_created_at,
                    "hint": hint,
                })),
            )
        }
        StateError::InvalidInput(msg) => invalid_params(msg),
        StateError::SerializationError(msg) => {
            tracing::warn!(detail = %msg, "journal payload serialization failed");
            let hint = require_hint(
                "this is a server-side encoding bug; report it with the run_id and check daemon logs"
                    .to_string(),
            );
            McpError::new(
                STORAGE_ERROR,
                "journal payload serialization failed".to_string(),
                Some(json!({
                    "code": "storage_error",
                    "kind": "storage_error",
                    "detail": "journal payload serialization failed",
                    "hint": hint,
                })),
            )
        }
        StateError::Storage(msg) => {
            tracing::warn!(detail = %msg, "storage error");
            let hint = require_hint(
                "retry the call; if it persists, check daemon logs for the underlying SQLite error"
                    .to_string(),
            );
            McpError::new(
                STORAGE_ERROR,
                "storage backend error".to_string(),
                Some(json!({
                    "code": "storage_error",
                    "kind": "storage_error",
                    "detail": "storage backend error",
                    "hint": hint,
                })),
            )
        }
    }
}

/// Build an `invalid_params` error (JSON-RPC -32602) from a free-form message.
///
/// The hint is chosen heuristically from the message content — `address` /
/// `run_id` / `strategy_id` / `tx_hash` get specialized hints; everything
/// else falls back to a generic schema pointer.
pub fn invalid_params(msg: impl Into<String>) -> McpError {
    let msg = msg.into();
    let lower = msg.to_lowercase();
    let hint = if lower.contains("address") {
        "address must be 0x + 40 hex chars (EIP-55 mixed case or lowercase accepted)"
    } else if lower.contains("run_id") {
        "run_id must be a 26-char ULID; list runs via execution://list"
    } else if lower.contains("strategy_id") {
        "strategy_id must be 64 lowercase hex chars; list via strategy://list"
    } else if lower.contains("tx_hash") || lower.contains("tx hash") {
        "tx_hash must be 0x + 64 hex chars"
    } else if lower.contains("source") {
        "see docs://strategy-bundle for the strategy source contract"
    } else if lower.contains("tag") {
        "each tag must be non-empty and ≤ 64 chars; max 16 tags"
    } else if lower.contains("name") {
        "name must be non-empty, ≤ 128 chars, and active-unique"
    } else if lower.contains("limit") {
        "limit must be an integer between 1 and 500"
    } else if lower.contains("since") {
        "since must be an RFC3339 timestamp (e.g. 2026-05-14T00:00:00Z)"
    } else if lower.contains("status") {
        "status must be one of: succeeded | failed | noop"
    } else {
        "re-read the tool description's `inputSchema`; correct the offending field and retry"
    };
    let hint = require_hint(hint.to_string());
    McpError::new(
        INVALID_PARAMS,
        msg.clone(),
        Some(json!({
            "code": "invalid_params",
            "kind": "invalid_params",
            "detail": msg,
            "hint": hint,
        })),
    )
}

/// Build a generic `storage_error` (-32016) from a free-form message. Used for
/// non-`StateError` failures (`spawn_blocking` join, JSON serialisation, etc.)
/// so all storage-path errors carry a uniform `data.code == "storage_error"`.
pub fn storage_error(msg: impl Into<String>) -> McpError {
    let msg = msg.into();
    let hint = require_hint(
        "transient or internal — retry the call; if it persists check daemon logs".to_string(),
    );
    McpError::new(
        STORAGE_ERROR,
        format!("storage error: {msg}"),
        Some(json!({
            "code": "storage_error",
            "kind": "storage_error",
            "detail": msg,
            "hint": hint,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_structured_data() {
        let e = unimplemented_err("strategy_register", 2, "see docs://strategy-bundle");
        assert_eq!(e.code, UNIMPLEMENTED_CODE);
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "unimplemented");
        assert_eq!(data["tool"], "strategy_register");
        assert_eq!(data["phase"], 2);
        assert!(
            data["hint"].as_str().is_some_and(|h| !h.is_empty()),
            "hint must be present and non-empty: {data}"
        );
    }

    #[test]
    fn map_state_error_not_found_uses_32014() {
        let e = map_state_error(StateError::NotFound("foo".into()));
        assert_eq!(e.code, STORAGE_NOT_FOUND);
        assert_eq!(e.code, ErrorCode(-32014));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "not_found");
        assert_eq!(data["resource"], "foo");
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn map_state_error_not_found_strategy_hint_points_at_strategy_list() {
        let e = map_state_error(StateError::NotFound("strategy abc".into()));
        let data = e.data.as_ref().expect("data");
        let hint = data["hint"].as_str().unwrap();
        assert!(
            hint.contains("strategy://list"),
            "strategy not_found hint should point at strategy://list: {hint}"
        );
    }

    #[test]
    fn map_state_error_not_found_trigger_hint_points_at_trigger_list() {
        let e = map_state_error(StateError::NotFound("trigger xyz".into()));
        let data = e.data.as_ref().expect("data");
        let hint = data["hint"].as_str().unwrap();
        assert!(
            hint.contains("trigger://list"),
            "trigger not_found hint should point at trigger://list: {hint}"
        );
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
        let hint = data["hint"].as_str().unwrap();
        assert!(
            hint.contains("abc") || hint.contains("existing_strategy_id"),
            "name_conflict hint should reference the existing id: {hint}"
        );
    }

    #[test]
    fn map_state_error_storage_uses_32016() {
        let e = map_state_error(StateError::Storage("boom".into()));
        assert_eq!(e.code, STORAGE_ERROR);
        assert_eq!(e.code, ErrorCode(-32016));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "storage_error");
        // MR-01: raw rusqlite text MUST NOT appear on the wire.
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
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn strategy_deleted_uses_32011() {
        let e = strategy_deleted("abc", "register a new strategy");
        assert_eq!(e.code, STRATEGY_DELETED);
        assert_eq!(e.code, ErrorCode(-32011));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_deleted");
        assert_eq!(data["strategy_id"], "abc");
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn strategy_invalid_output_uses_32018_carries_run_id_and_detail() {
        let e = strategy_invalid_output("got number", "01ARZ123", "return \"noop\" or Action[]");
        assert_eq!(e.code, STRATEGY_INVALID_OUTPUT);
        assert_eq!(e.code, ErrorCode(-32018));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_invalid_output");
        assert_eq!(data["detail"], "got number");
        assert_eq!(data["run_id"], "01ARZ123");
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn strategy_runtime_error_timeout_uses_32017_with_kind_timeout() {
        let e = strategy_runtime_error("timeout", "wall clock", "01ARZ123", "review source");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.code, ErrorCode(-32017));
        let data = e.data.as_ref().expect("data present");
        assert_eq!(data["code"], "strategy_runtime_error");
        assert_eq!(data["kind"], "timeout");
        assert_eq!(data["detail"], "wall clock");
        assert_eq!(data["run_id"], "01ARZ123");
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn map_runtime_error_classifies_each_variant() {
        // Timeout
        let e = map_runtime_error(RuntimeError::Timeout, "rid");
        assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
        assert_eq!(e.data.as_ref().unwrap()["kind"], "timeout");
        assert!(e.data.as_ref().unwrap()["hint"].as_str().is_some_and(|h| !h.is_empty()));
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
        assert!(e.data.as_ref().unwrap()["hint"].as_str().is_some_and(|h| !h.is_empty()));
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
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));

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
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
    }

    #[test]
    fn invalid_params_specializes_hint_for_address() {
        let e = invalid_params("expected address but got abcd");
        let hint = e.data.as_ref().unwrap()["hint"].as_str().unwrap();
        assert!(
            hint.contains("0x") && hint.contains("40"),
            "address hint should mention 0x+40 hex: {hint}"
        );
    }

    #[test]
    fn invalid_params_specializes_hint_for_run_id() {
        let e = invalid_params("run_id parse error");
        let hint = e.data.as_ref().unwrap()["hint"].as_str().unwrap();
        assert!(hint.contains("26") && hint.contains("ULID"), "run_id hint: {hint}");
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
        assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
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

    // ─────────── Phase 5 Plan 05-03 / D-08 policy_violation + D-15 policy_not_loaded ───────────

    mod policy_factory_tests {
        use super::*;
        use std::borrow::Cow;

        #[test]
        fn map_policy_error_emits_policy_violation_kind() {
            let verdict = DecisionVerdict::Deny {
                rule: Cow::Borrowed("contract_not_allowed"),
                detail: "contract 0xdead not allowed on chain 31337".into(),
            };
            let e = map_policy_error(&verdict, 1, "01ARZ");
            assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
            assert_eq!(e.code, ErrorCode(-32017));
            let data = e.data.expect("data present");
            assert_eq!(data["code"], "strategy_runtime_error");
            assert_eq!(data["kind"], "policy_violation");
            assert_eq!(data["rule"], "contract_not_allowed");
            assert_eq!(data["action_index"], 1);
            assert_eq!(data["run_id"], "01ARZ");
            let detail = data["detail"].as_str().unwrap();
            assert!(
                detail.starts_with("policy violation: "),
                "detail missing prefix: {detail}"
            );
            assert!(detail.contains("contract 0xdead not allowed"));
            assert!(data["hint"].as_str().is_some_and(|h| !h.is_empty()));
        }

        #[test]
        fn map_policy_error_carries_each_rule_taxonomy() {
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
                let e = map_policy_error(&verdict, 0, "rid");
                let data = e.data.expect("data present");
                assert_eq!(data["rule"], rule);
                assert_eq!(data["kind"], "policy_violation");
            }
        }

        #[test]
        fn policy_not_loaded_factory_emits_kind_policy_not_loaded() {
            let e = policy_not_loaded("01ARZ");
            assert_eq!(e.code, STRATEGY_RUNTIME_ERROR);
            assert_eq!(e.code, ErrorCode(-32017));
            let data = e.data.expect("data present");
            assert_eq!(data["code"], "strategy_runtime_error");
            assert_eq!(data["kind"], "policy_not_loaded");
            assert_eq!(data["run_id"], "01ARZ");
            assert_eq!(
                data["detail"],
                "policy violation: policy file not loaded — set [policy].path in config"
            );
            assert!(data["hint"].as_str().is_some_and(|h| h.contains("policy")));
            assert!(data.get("rule").is_none() || data["rule"].is_null());
            assert!(data.get("action_index").is_none() || data["action_index"].is_null());
        }

        #[test]
        fn map_policy_error_does_not_leak_raw_alloy_text() {
            let verdict = DecisionVerdict::Deny {
                rule: Cow::Borrowed("chain_not_allowed"),
                detail: "chain 999 not in policy allowlist".into(),
            };
            let e = map_policy_error(&verdict, 0, "rid");
            let data = e.data.expect("data");
            let s = serde_json::to_string(&data).unwrap();
            assert!(!s.contains("TransportError"), "alloy text leaked: {s}");
            assert!(!s.contains("toml::de"), "toml text leaked: {s}");
            assert!(!s.contains("Reqwest"), "reqwest text leaked: {s}");
        }
    }
}
