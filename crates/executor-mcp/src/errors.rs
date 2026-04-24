//! Structured MCP errors.
//!
//! Phase 1 ships `unimplemented_err(tool, phase)` so the four write-capable
//! tools (`strategy_register`, `strategy_delete`, `strategy_run_once`,
//! `policy_update`) return a uniform, machine-readable shape instead of a
//! free-form error string. Agents key off `data.code == "unimplemented"` +
//! `data.phase` + `data.tool` to plan follow-up work.
//!
//! Wire code: **-32010** (primary path verified against rmcp 1.5
//! `model::ErrorCode(pub i32)` tuple struct — cf. RESEARCH.md RESOLVED #4).

use rmcp::{ErrorData as McpError, model::ErrorCode};
use serde_json::json;

/// JSON-RPC 2.0 server-defined range: `-32000..-32099`.
/// We carve out `-32010` for "unimplemented feature".
const UNIMPLEMENTED_CODE: ErrorCode = ErrorCode(-32010);

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
}
