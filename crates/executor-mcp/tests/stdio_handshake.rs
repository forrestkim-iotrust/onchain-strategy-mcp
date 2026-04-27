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

// VALIDATION.md 1-02-02 — narrowed in Plan 02-02:
// `strategy_register` and `strategy_delete` now hit real storage in Phase 2,
// so only the still-phase-gated tools remain.
#[tokio::test]
async fn unimplemented_tools_return_phase_hint() -> Result<()> {
    let cases: [(&str, u64); 2] = [("strategy_run_once", 6), ("policy_update", 5)];
    for (tool, expected_phase) in cases {
        let mut proc = common::spawn_server_with_state(":memory:").await?;
        let _ = initialize(&mut proc).await?;

        let args = match tool {
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

// VALIDATION.md 1-02-03 — narrowed in Plan 02-02:
// `strategy_list` / `strategy_get` / `execution_get` are now storage-backed
// (covered by the new Phase 2 tests below). `policy_get` keeps its
// placeholder shape (Phase 5 wires the real engine).
#[tokio::test]
async fn policy_get_returns_placeholder() -> Result<()> {
    let mut proc = common::spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
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

    // resources/read → resource_not_found (-32002) with data.uri echoed.
    // Plan 02-02 narrows the assertion: a non-hex id surfaces as
    // `data.code == "malformed_id"`; the legacy `data.phase == 1` envelope
    // belongs to Phase 1 and no longer applies now that `strategy://` is wired.
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
    assert_eq!(r["error"]["data"]["uri"], "strategy://nonexistent");
    assert_eq!(r["error"]["data"]["code"], "malformed_id");

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

// ─────────── Plan 02-02: Phase 2 strategy behaviours (D-08a) ───────────

use common::{call_tool, extract_json_result, spawn_server_with_state};

#[tokio::test]
async fn strategy_register_creates_row() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({
            "name": "arb",
            "source": "// noop v1",
            "description": "demo",
            "tags": ["a", "b"]
        }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["already_exists"], false);
    assert_eq!(body["name"], "arb");
    assert_eq!(body["strategy_id"].as_str().unwrap().len(), 64);
    assert!(
        !body["created_at"].as_str().unwrap_or_default().is_empty(),
        "created_at must be populated"
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_register_idempotent_same_source() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r1 = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "first", "source": "// SAME", "description": "v1" }),
    )
    .await?;
    let b1 = extract_json_result(&r1);
    assert_eq!(b1["already_exists"], false);
    let id1 = b1["strategy_id"].as_str().unwrap().to_string();

    // Second register with SAME source but a different (unique) name +
    // description: server must report idempotent, preserving the original
    // row's name/description.
    let r2 = call_tool(
        &mut proc,
        3,
        "strategy_register",
        json!({ "name": "second", "source": "// SAME", "description": "v2" }),
    )
    .await?;
    let b2 = extract_json_result(&r2);
    assert_eq!(b2["already_exists"], true);
    assert_eq!(b2["strategy_id"].as_str().unwrap(), id1);
    // The response surfaces the FIRST registration's name, not the new one.
    assert_eq!(b2["name"], "first");
    assert_eq!(b2["existing_name"], "first");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_register_conflict_same_name_different_source() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r1 = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "arb", "source": "// src-A" }),
    )
    .await?;
    let b1 = extract_json_result(&r1);
    let id1 = b1["strategy_id"].as_str().unwrap().to_string();

    // Different source but same active name → name_conflict (-32015).
    let r2 = call_tool(
        &mut proc,
        3,
        "strategy_register",
        json!({ "name": "arb", "source": "// src-B" }),
    )
    .await?;
    let err = &r2["error"];
    assert_eq!(err["code"], -32015);
    assert_eq!(err["data"]["code"], "name_conflict");
    assert_eq!(err["data"]["attempted_name"], "arb");
    assert_eq!(err["data"]["existing_strategy_id"], id1);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_register_rejects_oversized_source() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let big = "x".repeat(262_145);
    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "huge", "source": big }),
    )
    .await?;
    let err = &r["error"];
    assert_eq!(err["code"], -32602);
    assert_eq!(err["data"]["code"], "invalid_params");
    let msg = err["message"].as_str().unwrap_or_default();
    assert!(msg.contains("262145"), "msg missing actual size: {msg}");
    assert!(msg.contains("262144"), "msg missing limit: {msg}");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_register_rejects_empty_name() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "   ", "source": "// ok" }),
    )
    .await?;
    let err = &r["error"];
    assert_eq!(err["code"], -32602);
    let msg = err["message"].as_str().unwrap_or_default();
    assert!(msg.contains("whitespace-only"), "msg: {msg}");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_list_excludes_source_payload() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    for (i, src) in ["// one", "// two"].iter().enumerate() {
        let r = call_tool(
            &mut proc,
            (2 + i) as u64,
            "strategy_register",
            json!({ "name": format!("s{i}"), "source": src }),
        )
        .await?;
        assert!(r["error"].is_null(), "register {i} failed: {r}");
    }

    let r = call_tool(&mut proc, 10, "strategy_list", json!({})).await?;
    let body = extract_json_result(&r);
    let items = body["strategies"].as_array().expect("strategies array");
    assert_eq!(items.len(), 2);
    for it in items {
        assert!(
            it.get("source").is_none(),
            "list item must not contain source: {it}"
        );
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_list_filters_deleted_by_default() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r1 = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "keep", "source": "// keep" }),
    )
    .await?;
    let _id_keep = extract_json_result(&r1)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();
    let r2 = call_tool(
        &mut proc,
        3,
        "strategy_register",
        json!({ "name": "drop", "source": "// drop" }),
    )
    .await?;
    let id_drop = extract_json_result(&r2)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();

    let dr = call_tool(
        &mut proc,
        4,
        "strategy_delete",
        json!({ "strategy_id": id_drop }),
    )
    .await?;
    assert!(dr["error"].is_null(), "delete failed: {dr}");

    let active = extract_json_result(&call_tool(&mut proc, 5, "strategy_list", json!({})).await?);
    assert_eq!(active["strategies"].as_array().unwrap().len(), 1);

    let all = extract_json_result(
        &call_tool(
            &mut proc,
            6,
            "strategy_list",
            json!({ "include_deleted": true }),
        )
        .await?,
    );
    assert_eq!(all["strategies"].as_array().unwrap().len(), 2);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_get_by_id_returns_source() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let src = "// the-source";
    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "x", "source": src }),
    )
    .await?;
    let id = extract_json_result(&r)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();

    let g = call_tool(
        &mut proc,
        3,
        "strategy_get",
        json!({ "strategy_id": id }),
    )
    .await?;
    let body = extract_json_result(&g);
    assert_eq!(body["source"], src);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_get_by_name_only_returns_active() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "arb", "source": "// arb" }),
    )
    .await?;
    let id = extract_json_result(&r)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();

    let _ = call_tool(
        &mut proc,
        3,
        "strategy_delete",
        json!({ "strategy_id": id }),
    )
    .await?;

    let g = call_tool(&mut proc, 4, "strategy_get", json!({ "name": "arb" })).await?;
    let err = &g["error"];
    assert_eq!(err["code"], -32014);
    assert_eq!(err["data"]["code"], "not_found");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_delete_is_soft_and_idempotent() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "x", "source": "// x" }),
    )
    .await?;
    let id = extract_json_result(&r)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();

    let d1 = extract_json_result(
        &call_tool(
            &mut proc,
            3,
            "strategy_delete",
            json!({ "strategy_id": id.clone() }),
        )
        .await?,
    );
    let deleted_at_1 = d1["deleted_at"].as_str().unwrap().to_string();

    let d2 = extract_json_result(
        &call_tool(
            &mut proc,
            4,
            "strategy_delete",
            json!({ "strategy_id": id.clone() }),
        )
        .await?,
    );
    assert_eq!(d2["deleted_at"].as_str().unwrap(), deleted_at_1);

    // get_by_id still returns the row, with deleted_at populated.
    let g = call_tool(
        &mut proc,
        5,
        "strategy_get",
        json!({ "strategy_id": id }),
    )
    .await?;
    let body = extract_json_result(&g);
    assert_eq!(body["deleted_at"].as_str().unwrap(), deleted_at_1);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn soft_deleted_name_can_be_reused() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r1 = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "arb", "source": "// src-A" }),
    )
    .await?;
    let id1 = extract_json_result(&r1)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();

    let _ = call_tool(
        &mut proc,
        3,
        "strategy_delete",
        json!({ "strategy_id": id1.clone() }),
    )
    .await?;

    let r2 = call_tool(
        &mut proc,
        4,
        "strategy_register",
        json!({ "name": "arb", "source": "// src-B" }),
    )
    .await?;
    let body = extract_json_result(&r2);
    assert_eq!(body["already_exists"], false);
    let id2 = body["strategy_id"].as_str().unwrap().to_string();
    assert_ne!(id1, id2, "new content-addressed id must differ");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn resource_read_strategy_uri_returns_body() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let src = "// resource-test";
    let r = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({ "name": "rsrc", "source": src }),
    )
    .await?;
    let id = extract_json_result(&r)["strategy_id"]
        .as_str()
        .unwrap()
        .to_string();

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "resources/read",
            "params": { "uri": format!("strategy://{id}") }
        }),
    )
    .await?;
    let resp = recv(&mut proc).await?;
    let contents = resp["result"]["contents"]
        .as_array()
        .expect("contents array");
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["mimeType"], "application/json");
    let text = contents[0]["text"].as_str().expect("contents.text");
    let body: Value = serde_json::from_str(text)?;
    assert_eq!(body["source"], src);
    assert_eq!(body["strategy_id"], id);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn execution_get_returns_not_found_when_empty() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = call_tool(
        &mut proc,
        2,
        "execution_get",
        json!({ "execution_id": "01HGXNONEXISTENTRUNIDXXXXX" }),
    )
    .await?;
    let err = &r["error"];
    assert_eq!(err["code"], -32014);
    assert_eq!(err["data"]["code"], "not_found");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategies_persist_across_restart() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    {
        let mut proc1 = spawn_server_with_state(&db_path_str).await?;
        let _ = initialize(&mut proc1).await?;
        let r = call_tool(
            &mut proc1,
            2,
            "strategy_register",
            json!({ "name": "persist", "source": "// persist" }),
        )
        .await?;
        assert!(r["error"].is_null(), "first-spawn register failed: {r}");
        proc1.child.kill().await?;
    }

    {
        let mut proc2 = spawn_server_with_state(&db_path_str).await?;
        let _ = initialize(&mut proc2).await?;
        let body = extract_json_result(
            &call_tool(&mut proc2, 2, "strategy_list", json!({})).await?,
        );
        let items = body["strategies"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "persist");
        proc2.child.kill().await?;
    }

    Ok(())
}
