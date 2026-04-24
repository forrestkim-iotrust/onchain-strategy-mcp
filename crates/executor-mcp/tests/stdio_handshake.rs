//! Phase 1 integration tests.
//!
//! Plan 01-01 seeded the harness (`common` module) plus the `harness_compiles`
//! smoke test. Plan 01-02 added:
//!   - `tools_list_emits_full_surface` (VALIDATION.md 1-02-01)
//!   - `unimplemented_tools_return_phase_hint` (1-02-02)
//!   - `readonly_tools_return_placeholder` (1-02-03)
//!
//! Plan 01-03 adds:
//!   - `resources_surface_matches_contract` (1-03-01)
//!   - `prompts_surface_matches_contract` (1-03-02)
//!   - `stdout_is_strict_jsonrpc` (1-03-03) — MCP-01 core assertion
//!   - `schema_contract_round_trip` (1-03-04) — MCP-02 serde round-trip
//!
//! Every test drives a freshly-spawned `executor-mcp` bin over stdio.

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

// VALIDATION.md 1-03-01
#[tokio::test]
async fn resources_surface_matches_contract() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    // resources/list → empty array
    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let list = r["result"]["resources"]
        .as_array()
        .expect("resources array");
    assert!(
        list.is_empty(),
        "resources/list must be empty in Phase 1, got {list:?}"
    );

    // resources/templates/list → 3 URI templates
    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/templates/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let templates = r["result"]["resourceTemplates"]
        .as_array()
        .expect("resourceTemplates array");
    let template_uris: Vec<&str> = templates
        .iter()
        .map(|t| t["uriTemplate"].as_str().unwrap_or_default())
        .collect();
    assert!(
        template_uris.contains(&"strategy://{strategy_id}"),
        "missing strategy template; got {template_uris:?}"
    );
    assert!(
        template_uris.contains(&"execution://{execution_id}"),
        "missing execution template; got {template_uris:?}"
    );
    assert!(
        template_uris.contains(&"journal://{execution_id}"),
        "missing journal template; got {template_uris:?}"
    );
    assert_eq!(
        templates.len(),
        3,
        "expected exactly 3 resource templates, got {}",
        templates.len()
    );

    // resources/read → resource_not_found (-32002) with data.phase=1, data.uri echoed
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "resources/read",
            "params": { "uri": "strategy://nonexistent" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert_eq!(r["error"]["code"], -32002, "expected resource_not_found");
    assert_eq!(r["error"]["data"]["phase"], 1);
    assert_eq!(r["error"]["data"]["uri"], "strategy://nonexistent");

    proc.child.kill().await?;
    Ok(())
}

// VALIDATION.md 1-03-02
#[tokio::test]
async fn prompts_surface_matches_contract() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    // prompts/list → 2 prompts with arguments schemas
    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let prompts = r["result"]["prompts"].as_array().expect("prompts array");
    let names: Vec<&str> = prompts
        .iter()
        .map(|p| p["name"].as_str().unwrap_or_default())
        .collect();
    assert!(
        names.contains(&"write_evm_strategy"),
        "missing write_evm_strategy; got {names:?}"
    );
    assert!(
        names.contains(&"review_evm_strategy"),
        "missing review_evm_strategy; got {names:?}"
    );
    assert_eq!(
        prompts.len(),
        2,
        "expected exactly 2 prompts, got {}",
        prompts.len()
    );
    for p in prompts {
        assert!(
            p.get("description").is_some(),
            "prompt {} missing description",
            p["name"]
        );
    }

    // prompts/get write_evm_strategy → placeholder PromptMessage referencing Phase 7
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "prompts/get",
            "params": { "name": "write_evm_strategy", "arguments": { "intent": "test intent" } }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"]
        .as_array()
        .expect("messages array");
    assert!(
        !messages.is_empty(),
        "write_evm_strategy returned no messages"
    );
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Phase 7") || text.contains("body will be finalized"),
        "placeholder marker missing from write_evm_strategy body: {text}"
    );

    // prompts/get review_evm_strategy → placeholder PromptMessage referencing Phase 7
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "prompts/get",
            "params": { "name": "review_evm_strategy", "arguments": { "strategy_id": "s-1" } }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"]
        .as_array()
        .expect("messages array");
    assert!(
        !messages.is_empty(),
        "review_evm_strategy returned no messages"
    );
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Phase 7") || text.contains("body will be finalized"),
        "placeholder marker missing from review_evm_strategy body: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// VALIDATION.md 1-03-03 — MCP-01 core assertion: stdout is JSON-RPC only.
// `common::recv` already asserts every received line parses as JSON-RPC 2.0
// with `jsonrpc: "2.0"`; this test exercises every MCP method the Phase 1
// server supports in rapid succession plus a deliberately-unknown tool call,
// so any rogue println!/eprintln! or non-JSON stderr leakage trips the
// `recv` assertion instead of silently corrupting the stream.
#[tokio::test]
async fn stdout_is_strict_jsonrpc() -> Result<()> {
    let mut proc = spawn_server().await?;
    let init_resp = initialize(&mut proc).await?;
    assert_eq!(init_resp["jsonrpc"], "2.0");
    assert_eq!(init_resp["id"], 1);

    for (id, method) in [
        (2i64, "tools/list"),
        (3, "resources/list"),
        (4, "resources/templates/list"),
        (5, "prompts/list"),
    ] {
        send(
            &mut proc,
            json!({ "jsonrpc": "2.0", "id": id, "method": method }),
        )
        .await?;
        let r = recv(&mut proc).await?;
        assert_eq!(
            r["jsonrpc"], "2.0",
            "method {method}: missing jsonrpc:2.0"
        );
        assert_eq!(r["id"], id, "method {method}: id mismatch");
    }

    // Unknown tool → must return a JSON-RPC error object, not a log line or
    // panic output. If `recv` parses the response as valid JSON-RPC the
    // stdout-purity contract holds.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 99, "method": "tools/call",
            "params": { "name": "nonexistent_tool", "arguments": {} }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(
        r.get("error").is_some(),
        "unknown tool must return a JSON-RPC error, not a log line: {r}"
    );

    proc.child.kill().await?;
    Ok(())
}

// VALIDATION.md 1-03-04 — schema contract round-trip for every input struct
// used as tool input OR prompt args. Detects silent serde/JsonSchema drift
// that would break agent integrations without necessarily failing the
// golden snapshot test (e.g., if the sample payload no longer deserializes).
#[tokio::test]
async fn schema_contract_round_trip() -> Result<()> {
    use executor_core::schema::execution::ExecutionIdInput;
    use executor_core::schema::policy::PolicyUpdateInput;
    use executor_core::schema::prompt_args::{ReviewEvmStrategyArgs, WriteEvmStrategyArgs};
    use executor_core::schema::strategy::{
        StrategyIdInput, StrategyRegisterInput, StrategyRunOnceInput,
    };

    let cases: [(&str, Value); 7] = [
        (
            "StrategyRegisterInput",
            json!({ "name": "x", "source": "// noop" }),
        ),
        ("StrategyIdInput", json!({ "strategy_id": "s-1" })),
        ("StrategyRunOnceInput", json!({ "strategy_id": "s-1" })),
        ("ExecutionIdInput", json!({ "execution_id": "e-1" })),
        ("PolicyUpdateInput", json!({})),
        ("WriteEvmStrategyArgs", json!({ "intent": "transfer usdc" })),
        ("ReviewEvmStrategyArgs", json!({ "strategy_id": "s-1" })),
    ];

    // Each `from_value` call is the round-trip: agent-shaped JSON → our
    // JsonSchema-derived struct. If the field names / types drift, this
    // fails before the tool or prompt handler ever gets called.
    let _: StrategyRegisterInput = serde_json::from_value(cases[0].1.clone())?;
    let _: StrategyIdInput = serde_json::from_value(cases[1].1.clone())?;
    let _: StrategyRunOnceInput = serde_json::from_value(cases[2].1.clone())?;
    let _: ExecutionIdInput = serde_json::from_value(cases[3].1.clone())?;
    let _: PolicyUpdateInput = serde_json::from_value(cases[4].1.clone())?;
    let _: WriteEvmStrategyArgs = serde_json::from_value(cases[5].1.clone())?;
    let _: ReviewEvmStrategyArgs = serde_json::from_value(cases[6].1.clone())?;

    // Shape sanity — samples are JSON objects, not scalars or arrays.
    for (name, sample) in &cases {
        assert!(sample.is_object(), "{name}: sample is not a JSON object");
    }
    Ok(())
}
