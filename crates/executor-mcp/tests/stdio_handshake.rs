//! Phase 1 integration tests.
//!
//! Plan 01-01 seeded the harness (`common` module) plus the `harness_compiles`
//! smoke test. Plan 01-02 adds:
//!   - `tools_list_emits_full_surface` (VALIDATION.md 1-02-01)
//!   - `unimplemented_tools_return_phase_hint` (1-02-02)
//!   - `readonly_tools_return_placeholder` (1-02-03)
//!
//! Plan 03 will add resources / prompts / stdout-purity tests to this same
//! file. Every test drives a freshly-spawned `executor-mcp` bin over stdio.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server};

/// JSON-RPC wire code returned by `unimplemented_err`.
/// Primary path recorded in 01-02-SUMMARY: `rmcp::model::ErrorCode(pub i32)`
/// tuple constructor is public on rmcp 1.5, so `-32010` is used directly.
const EXPECTED_UNIMPL_CODE: i64 = -32010;

#[tokio::test]
async fn harness_compiles() -> Result<()> {
    // Plan 01-01 smoke test — kept as a fast sanity check even though
    // `tools_list_emits_full_surface` subsumes it.
    let _ = spawn_server().await?;
    Ok(())
}

// VALIDATION.md 1-02-01
#[tokio::test]
async fn tools_list_emits_full_surface() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let tools = r["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default())
        .collect();
    for expected in [
        "strategy_register",
        "strategy_list",
        "strategy_get",
        "strategy_delete",
        "strategy_run_once",
        "execution_get",
        "policy_get",
        "policy_update",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool: {expected} — got: {names:?}"
        );
    }
    assert_eq!(
        tools.len(),
        8,
        "expected exactly 8 tools, got {}",
        tools.len()
    );
    for t in tools {
        assert!(
            t.get("inputSchema").is_some(),
            "tool {} missing inputSchema",
            t["name"]
        );
        assert!(
            t.get("description").is_some(),
            "tool {} missing description",
            t["name"]
        );
    }
    proc.child.kill().await?;
    Ok(())
}

// VALIDATION.md 1-02-02
#[tokio::test]
async fn unimplemented_tools_return_phase_hint() -> Result<()> {
    let cases: [(&str, u64); 4] = [
        ("strategy_register", 2),
        ("strategy_delete", 2),
        ("strategy_run_once", 6),
        ("policy_update", 5),
    ];
    for (tool, expected_phase) in cases {
        let mut proc = spawn_server().await?;
        let _ = initialize(&mut proc).await?;

        let args = match tool {
            "strategy_register" => json!({ "name": "x", "source": "// noop" }),
            "strategy_delete" => json!({ "strategy_id": "s-1" }),
            "strategy_run_once" => json!({ "strategy_id": "s-1" }),
            "policy_update" => json!({}),
            _ => unreachable!(),
        };
        send(
            &mut proc,
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": tool, "arguments": args }
            }),
        )
        .await?;
        let r = recv(&mut proc).await?;
        let err = &r["error"];
        assert_eq!(
            err["code"], EXPECTED_UNIMPL_CODE,
            "tool {tool}: expected code {EXPECTED_UNIMPL_CODE}, got {}",
            err["code"]
        );
        assert_eq!(err["data"]["code"], "unimplemented", "tool {tool}: data.code");
        assert_eq!(
            err["data"]["phase"], expected_phase,
            "tool {tool}: expected phase {expected_phase}, got {}",
            err["data"]["phase"]
        );
        assert_eq!(err["data"]["tool"], tool, "tool {tool}: data.tool mismatch");
        proc.child.kill().await?;
    }
    Ok(())
}

// VALIDATION.md 1-02-03
#[tokio::test]
async fn readonly_tools_return_placeholder() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    // strategy_list → []
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "strategy_list", "arguments": {} }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let content = r["result"]["content"][0]["text"]
        .as_str()
        .expect("content text");
    let list: Value = serde_json::from_str(content)?;
    assert!(
        list.is_array() && list.as_array().unwrap().is_empty(),
        "strategy_list must return []"
    );

    // policy_get → placeholder object with chains/targets/selectors arrays
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "policy_get", "arguments": {} }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let content = r["result"]["content"][0]["text"]
        .as_str()
        .expect("content text");
    let policy: Value = serde_json::from_str(content)?;
    assert!(policy["chains"].is_array(), "policy.chains must be array");
    assert!(policy["targets"].is_array(), "policy.targets must be array");
    assert!(
        policy["selectors"].is_array(),
        "policy.selectors must be array"
    );

    // strategy_get → resource_not_found with data.phase == 2
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "strategy_get", "arguments": { "strategy_id": "none" } }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert_eq!(r["error"]["data"]["phase"], 2);

    // execution_get → resource_not_found with data.phase == 6
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "execution_get", "arguments": { "execution_id": "none" } }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert_eq!(r["error"]["data"]["phase"], 6);

    proc.child.kill().await?;
    Ok(())
}
