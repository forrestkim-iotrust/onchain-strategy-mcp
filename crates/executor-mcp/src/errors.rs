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

use executor_state::StateError;
use rmcp::{ErrorData as McpError, model::ErrorCode};
use serde_json::json;

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
        StateError::Storage(msg) => McpError::new(
            STORAGE_ERROR,
            format!("storage error: {msg}"),
            Some(json!({ "code": "storage_error", "detail": msg })),
        ),
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
        assert!(e.message.contains("boom"), "message missing detail: {}", e.message);
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
}
