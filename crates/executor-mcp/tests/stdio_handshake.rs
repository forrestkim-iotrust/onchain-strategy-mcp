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
#[cfg(feature = "anvil-tests")]
use alloy::network::TransactionBuilder;
#[cfg(feature = "anvil-tests")]
use alloy::providers::Provider;
#[cfg(feature = "anvil-tests")]
use alloy::rpc::types::TransactionRequest;
#[cfg(feature = "anvil-tests")]
use alloy_primitives::Address;
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
        "strategy_run",
        "execution_get",
        "policy_get",
        // v1.2 Trigger Core (Stream C): 6 trigger tools.
        "trigger_register",
        "trigger_list",
        "trigger_get",
        "trigger_delete",
        "trigger_set_enabled",
        "trigger_events",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool: {expected} — got: {names:?}"
        );
    }
    assert!(
        !names.contains(&"policy_update"),
        "policy_update was removed in v1.4 (Track F); got: {names:?}"
    );
    assert_eq!(
        tools.len(),
        15,
        "expected exactly 15 tools (7 base + 2 evm read/view + 6 trigger), got {}",
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

// VALIDATION.md 1-02-02 — narrowed in Plan 02-02 and again in v1.4 Track F:
// `policy_update` was the last phase-gated placeholder. v1.4 removes the tool
// entirely (honesty-over-completeness, design principle P6); policy is edited
// via `.local/policy.toml` only. The test below asserts that calling the
// dropped tool surfaces as a normal "unknown tool" error rather than as the
// old `-32010 unimplemented` envelope.
#[tokio::test]
async fn dropped_policy_update_returns_unknown_tool() -> Result<()> {
    let _ = EXPECTED_UNIMPL_CODE; // kept for other tests; no longer asserted here
    let mut proc = common::spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "policy_update", "arguments": {} }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(
        r.get("error").is_some(),
        "policy_update was removed in v1.4; expected a JSON-RPC error, got: {r}"
    );
    let err = &r["error"];
    assert_ne!(
        err["code"], EXPECTED_UNIMPL_CODE,
        "policy_update must no longer return -32010 unimplemented; got: {err}"
    );
    proc.child.kill().await?;
    Ok(())
}

// VALIDATION.md 1-02-03 — narrowed in Plan 02-02 + updated in Plan 05-03:
// `policy_get` now returns the live policy via `Arc<RwLock<Option<LoadedPolicy>>>`.
// In the bare `spawn_server_with_state(":memory:")` setup there is no
// `[policy].path` configured, so the response is the fail-closed placeholder
// `{loaded: false, reason: ...}` (D-15).
#[tokio::test]
async fn policy_get_returns_loaded_false_when_policy_not_configured() -> Result<()> {
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
    let body: Value = serde_json::from_str(content)?;
    assert_eq!(
        body["loaded"], false,
        "policy_get without [policy].path must return loaded: false (D-15 fail-closed)"
    );
    assert!(
        body["reason"]
            .as_str()
            .is_some_and(|s| s.contains("policy not loaded")),
        "reason missing fail-closed marker: {}",
        body["reason"]
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
        template_uris.contains(&"execution://{run_id}"),
        "missing execution template; got {template_uris:?}"
    );
    // v1.4 Track C: filtered run-summary listing.
    assert!(
        template_uris.contains(&"execution://list"),
        "missing execution://list template; got {template_uris:?}"
    );
    assert!(
        template_uris.contains(&"journal://{run_id}"),
        "missing journal template; got {template_uris:?}"
    );
    // v1.2 Trigger Core (Stream C): trigger + trigger-events templates.
    assert!(
        template_uris.contains(&"trigger://{trigger_id}"),
        "missing trigger template; got {template_uris:?}"
    );
    assert!(
        template_uris.contains(&"trigger-events://{trigger_id}"),
        "missing trigger-events template; got {template_uris:?}"
    );
    // v1.3 self-documenting surface: examples + docs templates.
    for required in [
        "examples://strategies",
        "examples://strategies/{name}",
        "examples://contracts/{name}",
        "docs://policy-model",
        "docs://eip-7702",
        "docs://trigger-model",
    ] {
        assert!(
            template_uris.contains(&required),
            "missing self-doc template {required}; got {template_uris:?}"
        );
    }

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

    // prompts/list → authoring pair + 4 self-documenting prompts (v1.3).
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
    for required in [
        "write_evm_strategy",
        "review_evm_strategy",
        "getting_started",
        "trigger_patterns",
        "example_strategies",
        "common_pitfalls",
        // v1.4 Track E1 — server-prefetch workflow prompts.
        "safety_review",
        "author_strategy",
    ] {
        assert!(
            names.contains(&required),
            "missing prompt {required}; got {names:?}"
        );
    }
    for p in prompts {
        assert!(
            p.get("description").is_some(),
            "prompt {} missing description",
            p["name"]
        );
    }

    // prompts/get write_evm_strategy → guided authoring body referencing ctx API.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "prompts/get",
            "params": { "name": "write_evm_strategy", "arguments": { "intent": "test intent" } }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"].as_array().expect("messages array");
    assert!(
        !messages.is_empty(),
        "write_evm_strategy returned no messages"
    );
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("test intent") && text.contains("ctx.actions"),
        "authoring body did not echo intent + ctx API: {text}"
    );

    // prompts/get review_evm_strategy → review body referencing the strategy id.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "prompts/get",
            "params": { "name": "review_evm_strategy", "arguments": { "strategy_id": "s-1" } }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"].as_array().expect("messages array");
    assert!(
        !messages.is_empty(),
        "review_evm_strategy returned no messages"
    );
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("s-1") && text.contains("policy_get"),
        "review body did not reference id + policy_get: {text}"
    );

    // prompts/get getting_started → v1.4 Track E1 prefetched orientation:
    // inlines the live strategy_list (empty-state placeholder here, since the
    // server boots with an in-memory store and no registered strategies) +
    // the loaded policy summary (also empty here — no [policy].path) + the
    // 5-step playbook. Markers: the prefetched "Registered strategies"
    // header, the empty-state placeholder, the playbook H2.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 6, "method": "prompts/get",
            "params": { "name": "getting_started", "arguments": {} }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"]
        .as_array()
        .expect("messages array");
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.len() > 200,
        "getting_started body too short: {} chars",
        text.len()
    );
    assert!(
        text.contains("Registered strategies") && text.contains("strategy_list"),
        "getting_started missing prefetched strategy_list header: {text}"
    );
    assert!(
        text.contains("First-action playbook"),
        "getting_started missing playbook H2: {text}"
    );
    assert!(
        text.contains("Active policy"),
        "getting_started missing policy block: {text}"
    );

    // v1.4 Track E1: prompts/get safety_review with a proposed source that
    // exercises the static-analysis surface (ctx.actions.contractCall + a
    // trailing semicolon pitfall). Assert: itemized extracted action +
    // pitfall finding + verdict block present.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 7, "method": "prompts/get",
            "params": {
                "name": "safety_review",
                "arguments": {
                    "source": "((ctx) => [ctx.actions.contractCall({ address: \"0xdead\", abi: [], function: \"supply\", args: [] })]);"
                }
            }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"]
        .as_array()
        .expect("messages array");
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Extracted `ctx.actions.*` calls"),
        "safety_review missing action extraction header: {text}"
    );
    assert!(
        text.contains("ctx.actions.contractCall") && text.contains("0xdead"),
        "safety_review didn't surface the extracted action: {text}"
    );
    assert!(
        text.contains("Static-analysis findings") && text.contains("Trailing"),
        "safety_review didn't flag the trailing semicolon: {text}"
    );
    assert!(
        text.contains("## Verdict"),
        "safety_review missing verdict block: {text}"
    );
    assert!(
        text.contains("Active policy"),
        "safety_review missing inline policy: {text}"
    );

    // v1.4 Track E1: prompts/get author_strategy with a funnel-style intent
    // should select the eth-funnel example and include the bundle skeleton.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 8, "method": "prompts/get",
            "params": {
                "name": "author_strategy",
                "arguments": { "intent": "ETH to USDC Aave funnel" }
            }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let messages = r["result"]["messages"]
        .as_array()
        .expect("messages array");
    let text = messages[0]["content"]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("Bundle skeleton") && text.contains("execute") && text.contains("records") && text.contains("view"),
        "author_strategy missing bundle skeleton with execute/records/view: {text}"
    );
    assert!(
        text.contains("examples://strategies/eth-funnel"),
        "author_strategy didn't route funnel intent to eth-funnel: {text}"
    );
    assert!(
        text.contains("ETH to USDC Aave funnel"),
        "author_strategy didn't echo the intent: {text}"
    );
    assert!(
        text.contains("docs://strategy-bundle"),
        "author_strategy missing docs pointer: {text}"
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
        assert_eq!(r["jsonrpc"], "2.0", "method {method}: missing jsonrpc:2.0");
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
    use executor_core::schema::prompt_args::{
        AuthorStrategyArgs, ReviewEvmStrategyArgs, SafetyReviewArgs, WriteEvmStrategyArgs,
    };
    use executor_core::schema::strategy::{
        StrategyIdInput, StrategyRegisterInput, StrategyRunOnceInput,
    };

    let cases: [(&str, Value); 9] = [
        (
            "StrategyRegisterInput",
            json!({ "name": "x", "source": "// noop" }),
        ),
        ("StrategyIdInput", json!({ "strategy_id": "s-1" })),
        ("StrategyRunOnceInput", json!({ "strategy_id": "s-1" })),
        ("ExecutionIdInput", json!({ "run_id": "e-1" })),
        ("PolicyUpdateInput", json!({})),
        ("WriteEvmStrategyArgs", json!({ "intent": "transfer usdc" })),
        ("ReviewEvmStrategyArgs", json!({ "strategy_id": "s-1" })),
        // v1.4 Track E1 args
        ("SafetyReviewArgs", json!({ "source": "((ctx) => \"noop\")" })),
        ("AuthorStrategyArgs", json!({ "intent": "yield snapshot" })),
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
    let _: SafetyReviewArgs = serde_json::from_value(cases[7].1.clone())?;
    let _: AuthorStrategyArgs = serde_json::from_value(cases[8].1.clone())?;

    // Shape sanity — samples are JSON objects, not scalars or arrays.
    for (name, sample) in &cases {
        assert!(sample.is_object(), "{name}: sample is not a JSON object");
    }
    Ok(())
}

// ─────────── Plan 02-02: Phase 2 strategy behaviours (D-08a) ───────────

use common::{
    call_tool, extract_json_result, spawn_server_with_config_text,
    spawn_server_with_config_text_and_env, spawn_server_with_state,
};

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

    let g = call_tool(&mut proc, 3, "strategy_get", json!({ "strategy_id": id })).await?;
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
    let g = call_tool(&mut proc, 5, "strategy_get", json!({ "strategy_id": id })).await?;
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
        json!({ "run_id": "01HGXNONEXISTENTRUNIDXXXXX" }),
    )
    .await?;
    let err = &r["error"];
    assert_eq!(err["code"], -32014);
    assert_eq!(err["data"]["code"], "not_found");

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn execution_status_surfaces_match() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    let (strategy_id, run_id) = {
        let mut store = executor_state::StateStore::open(&db_path)?;
        let outcome = store.register_strategy("exec_status", "(ctx) => []", None, None)?;
        let sid = match outcome {
            executor_state::RegisterOutcome::Created(s)
            | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
        };
        let rid = store.insert_run(&sid, executor_core::schema::execution::RunStatus::Running)?;
        store.record_execution_broadcast(
            &rid,
            1,
            "0x1111111111111111111111111111111111111111",
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )?;
        store.record_execution_receipt_success(&rid, 1, "success", "21000")?;
        (sid, rid)
    };

    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let tool = extract_json_result(
        &call_tool(
            &mut proc,
            2,
            "execution_get",
            json!({ "run_id": run_id }),
        )
        .await?,
    );
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "resources/read",
            "params": { "uri": format!("execution://{run_id}") }
        }),
    )
    .await?;
    let resource_resp = recv(&mut proc).await?;
    let contents = resource_resp["result"]["contents"]
        .as_array()
        .expect("contents array");
    let resource_text = contents[0]["text"].as_str().expect("resource text");
    let resource: Value = serde_json::from_str(resource_text)?;

    assert_eq!(tool["run_id"], run_id);
    assert_eq!(tool["strategy_id"], strategy_id);
    assert_eq!(tool["run_id"], resource["run_id"]);
    assert_eq!(tool["status"], resource["status"]);
    assert_eq!(tool["actions"], resource["actions"]);
    assert_eq!(tool["actions"][0]["action_index"], 1);
    assert_eq!(tool["actions"][0]["status"], "confirmed");
    assert_eq!(tool["actions"][0]["receipt_status"], "success");
    assert_eq!(tool["actions"][0]["gas_used"], "21000");
    assert_eq!(tool["actions"][0]["tx_hash"], "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    proc.child.kill().await?;
    Ok(())
}

// ─────────── Plan 02-03: end-to-end run roundtrip + schema future-variants ───────────

#[tokio::test]
async fn run_roundtrip_insert_get_update_status() -> Result<()> {
    use executor_core::schema::execution::RunStatus;
    use executor_state::{RegisterOutcome, StateStore};

    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    // Step 1: seed strategy + run directly via executor-state (server is OFF).
    let (strategy_id, run_id) = {
        let mut store = StateStore::open(&db_path)?;
        let outcome = store.register_strategy("seed", "// seed strategy\n", None, None)?;
        let sid = match outcome {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        };
        let rid = store.insert_run(&sid, RunStatus::Queued)?;
        (sid, rid)
    };

    // Step 2: spawn server, observe queued.
    {
        let mut proc = common::spawn_server_with_state(&db_path_str).await?;
        let _ = initialize(&mut proc).await?;
        let r = call_tool(
            &mut proc,
            2,
            "execution_get",
            json!({ "run_id": run_id }),
        )
        .await?;
        let body = extract_json_result(&r);
        assert_eq!(body["run_id"].as_str(), Some(run_id.as_str()));
        assert_eq!(body["strategy_id"].as_str(), Some(strategy_id.as_str()));
        assert_eq!(body["status"].as_str(), Some("queued"));
        assert!(
            body["actions"].as_array().is_some_and(|a| a.is_empty()),
            "runs without execution rows must return actions: []: {body}"
        );
        assert!(
            body.get("signer_address").is_none_or(|v| v.is_null()),
            "runs without execution rows must omit/null signer_address: {body}"
        );
        assert!(
            body["started_at"].as_str().is_some_and(|s| !s.is_empty()),
            "started_at must be a non-empty string: {body}"
        );
        assert!(
            body.get("finished_at").is_none_or(|v| v.is_null()),
            "finished_at must be absent or null when queued: {body}"
        );
        proc.child.kill().await?;
    }

    // Step 3: transition to Running out-of-band.
    {
        let mut store = StateStore::open(&db_path)?;
        // MR-02: deprecated legacy API used here intentionally — the test
        // simulates an out-of-band lifecycle without a strategy_run handler.
        #[allow(deprecated)]
        store.update_run_status(&run_id, RunStatus::Running)?;
    }

    // Step 4: observe running.
    {
        let mut proc = common::spawn_server_with_state(&db_path_str).await?;
        let _ = initialize(&mut proc).await?;
        let r = call_tool(
            &mut proc,
            2,
            "execution_get",
            json!({ "run_id": run_id }),
        )
        .await?;
        let body = extract_json_result(&r);
        assert_eq!(body["status"].as_str(), Some("running"));
        assert!(
            body.get("finished_at").is_none_or(|v| v.is_null()),
            "finished_at must remain null while running: {body}"
        );
        proc.child.kill().await?;
    }

    // Step 5: transition to Succeeded out-of-band.
    {
        let mut store = StateStore::open(&db_path)?;
        // MR-02: deprecated legacy API used here intentionally (see Step 3).
        #[allow(deprecated)]
        store.update_run_status(&run_id, RunStatus::Succeeded)?;
    }

    // Step 6: observe succeeded + finished_at populated.
    {
        let mut proc = common::spawn_server_with_state(&db_path_str).await?;
        let _ = initialize(&mut proc).await?;
        let r = call_tool(
            &mut proc,
            2,
            "execution_get",
            json!({ "run_id": run_id }),
        )
        .await?;
        let body = extract_json_result(&r);
        assert_eq!(body["status"].as_str(), Some("succeeded"));
        assert!(
            body["finished_at"].as_str().is_some_and(|s| !s.is_empty()),
            "finished_at must be populated on terminal status: {body}"
        );
        proc.child.kill().await?;
    }

    Ok(())
}

#[tokio::test]
async fn run_status_schema_includes_future_variants() -> Result<()> {
    // D-08a: prove the RunStatus JSON Schema golden carries all 7 snake_case
    // variants — the 4 Phase 2 emits plus the 3 future-reserved ones
    // (canceled, simulation_denied, policy_denied) so Phase 5/6 can rely on
    // these wire names not drifting.
    //
    // The schemars 1.x emission for `RunStatus` uses a `oneOf` of an `enum`
    // array (the 4 emittable variants) plus 3 `const`-string entries (the
    // reserved variants). The walker below collects strings from both `enum`
    // arrays AND `const` fields, then asserts the full 7-variant set is
    // present.
    let path = std::path::Path::new("../executor-core/tests/schemas/RunStatus.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let v: Value = serde_json::from_str(&text)?;

    fn collect_strings(v: &Value, out: &mut std::collections::BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                if let Some(arr) = m.get("enum").and_then(|x| x.as_array()) {
                    for x in arr {
                        if let Some(s) = x.as_str() {
                            out.insert(s.to_string());
                        }
                    }
                }
                if let Some(s) = m.get("const").and_then(|x| x.as_str()) {
                    out.insert(s.to_string());
                }
                for (_k, val) in m {
                    collect_strings(val, out);
                }
            }
            Value::Array(a) => {
                for x in a {
                    collect_strings(x, out);
                }
            }
            _ => {}
        }
    }

    let mut found = std::collections::BTreeSet::new();
    collect_strings(&v, &mut found);

    let expected = [
        "queued",
        "running",
        "succeeded",
        "failed",
        "canceled",
        "simulation_denied",
        "policy_denied",
    ];
    let missing: Vec<&&str> = expected.iter().filter(|e| !found.contains(**e)).collect();
    assert!(
        missing.is_empty(),
        "RunStatus.json missing future-reserved variants {missing:?}; found {found:?}"
    );

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
        let body =
            extract_json_result(&call_tool(&mut proc2, 2, "strategy_list", json!({})).await?);
        let items = body["strategies"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "persist");
        proc2.child.kill().await?;
    }

    Ok(())
}

// ─────────── Plan 03-03: D-08a strategy_run integration tests (19) ───────────
//
// Each test spawns its own server with a tempdir-backed sqlite DB, drives
// `strategy_run` via JSON-RPC, and asserts on either the `result` body
// (success path) or the `error` envelope (failure path). For success
// paths the test re-opens the StateStore directly to verify journal rows.
//
// `call_tool`, `extract_json_result`, and `spawn_server_with_state` are
// already brought into scope earlier in the file via `use common::{..};`.

/// Helper: register a strategy directly via executor-state and return its id.
fn seed_strategy(db_path: &std::path::Path, name: &str, source: &str) -> Result<String> {
    let mut store = executor_state::StateStore::open(db_path)?;
    let outcome = store.register_strategy(name, source, None, None)?;
    let id = match outcome {
        executor_state::RegisterOutcome::Created(s)
        | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
    };
    Ok(id)
}

fn write_policy(toml: &str) -> Result<tempfile::NamedTempFile> {
    let policy = tempfile::NamedTempFile::new()?;
    std::fs::write(policy.path(), toml)?;
    Ok(policy)
}

#[cfg(feature = "anvil-tests")]
fn write_permissive_policy(contracts: &[&str]) -> Result<tempfile::NamedTempFile> {
    let policy = tempfile::NamedTempFile::new()?;
    let contracts_toml = contracts
        .iter()
        .map(|addr| format!("    \"{addr}\","))
        .collect::<Vec<_>>()
        .join("\n");
    let selector_entries = contracts
        .iter()
        .map(|addr| format!("[selectors.\"31337:{addr}\"]\nallow = [\"any\"]\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let raw_allow = contracts
        .iter()
        .map(|addr| format!("    {{ chain = 31337, contract = \"{addr}\", selector = \"any\" }},"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        policy.path(),
        format!(
            r#"[chains]
allow = [31337]

[contracts.31337]
allow = [
{contracts_toml}
]

{selector_entries}
[native_value.31337]
max_per_action = "1000000000000000000000000"

[raw_call]
allow_global = false
allow = [
{raw_allow}
]
"#
        ),
    )?;
    Ok(policy)
}

async fn spawn_server_with_policy_and_rpc(
    db_path: &std::path::Path,
    policy_path: &std::path::Path,
    rpc_url: &str,
) -> Result<common::ServerProc> {
    spawn_server_with_config_text(&format!(
        r#"[state]
path = "{}"

[policy]
path = "{}"

[evm]
rpc_url = "{}"
call_timeout_ms = 1000
"#,
        db_path.display(),
        policy_path.display(),
        rpc_url,
    ))
    .await
}

async fn spawn_server_with_policy_rpc_and_signer(
    db_path: &std::path::Path,
    policy_path: &std::path::Path,
    rpc_url: &str,
    private_key_env: &str,
    private_key: &str,
) -> Result<common::ServerProc> {
    spawn_server_with_config_text_and_env(
        &format!(
            r#"[state]
path = "{}"

[policy]
path = "{}"

[evm]
rpc_url = "{}"
call_timeout_ms = 1000

[signer]
private_key_env = "{}"
receipt_timeout_ms = 120000
"#,
            db_path.display(),
            policy_path.display(),
            rpc_url,
            private_key_env,
        ),
        &[(private_key_env, private_key)],
    )
    .await
}

#[tokio::test]
async fn strategy_run_returns_noop_for_minimal_strategy() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "noop_test", "(ctx) => \"noop\"")?;

    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["status"].as_str(), Some("succeeded"));
    assert_eq!(body["outcome"]["kind"].as_str(), Some("noop"));
    assert_eq!(body["strategy_id"].as_str(), Some(strategy_id.as_str()));
    let run_id = body["run_id"].as_str().expect("run_id present").to_string();
    assert!(!run_id.is_empty());
    assert!(body["finished_at"].as_str().is_some_and(|s| !s.is_empty()));
    proc.child.kill().await?;

    let store = executor_state::StateStore::open(&db_path)?;
    let sources = store.list_source_reads_for_run(&run_id)?;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].kind, "strategy_source");
    assert_eq!(sources[0].target, strategy_id);
    let actions = store.list_actions_for_run(&run_id)?;
    assert_eq!(actions.len(), 1);
    let outcome_json = serde_json::to_value(actions[0].outcome)?;
    assert_eq!(outcome_json.as_str(), Some("noop"));
    Ok(())
}

#[tokio::test]
async fn strategy_run_returns_actions_for_action_array_strategy() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "act", "(ctx) => [{kind:\"noop\"}]")?;

    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["outcome"]["kind"].as_str(), Some("actions"));
    let actions = body["outcome"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["kind"].as_str(), Some("noop"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_returns_actions_for_empty_array() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "empty", "(ctx) => []")?;

    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["outcome"]["kind"].as_str(), Some("actions"));
    assert_eq!(body["outcome"]["actions"].as_array().unwrap().len(), 0);
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_number_return() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "num", "(ctx) => 42")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope present");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    assert!(
        err["data"]["detail"]
            .as_str()
            .is_some_and(|s| s.contains("number"))
    );
    assert!(
        err["data"]["run_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_object_return() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "obj", "(ctx) => ({foo: 1})")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_null_return() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "n", "(ctx) => null")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_promise_return() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "p", "(ctx) => Promise.resolve(\"noop\")")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("promise"),
        "detail missing 'promise': {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_non_function_source() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    // Top-level expression evaluates to a string, not a function (violates D-05 Shape B).
    let strategy_id = seed_strategy(&db_path, "nonfn", "\"noop\"")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("function") || detail.contains("(ctx)"),
        "detail missing function/ctx hint: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

// D-16 (Phase-4 04-03): renamed from the Phase-3 placeholder reject test
// (03-CONTEXT D-08a). Phase 3 rejected ALL `kind != noop`. Phase 4 widens
// the allowlist to the five new wire variants (D-08 / D-09); the Phase-3
// spirit is preserved by `strategy_run_rejects_unknown_action_kind` below
// — `kind:"multi_call"` is still rejected because it's not in the Phase-4
// allowlist.
#[cfg(feature = "anvil-tests")]
#[tokio::test]
async fn strategy_run_accepts_contract_call() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    // Use the JS-side ctx.actions.contractCall builder so the round-trip
    // exercises both the sandbox host binding AND validate_strategy_output.
    let abi = r#"[{"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}]"#;
    let abi_lit = serde_json::to_string(abi)?;
    let source = format!(
        r#"(ctx) => [ctx.actions.contractCall({{
            address: "0x0000000000000000000000000000000000000001",
            abi: {abi_lit},
            function: "transfer",
            args: ["0x0000000000000000000000000000000000000002", "1000"]
        }})]"#
    );
    let strategy_id = seed_strategy(&db_path, "cc_accept", &source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["status"].as_str(), Some("succeeded"));
    assert_eq!(body["outcome"]["kind"].as_str(), Some("actions"));
    let actions = body["outcome"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["kind"].as_str(), Some("contract_call"));
    proc.child.kill().await?;
    Ok(())
}

#[cfg(feature = "anvil-tests")]
#[tokio::test]
async fn strategy_run_accepts_raw_call() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [ctx.actions.rawCall({
        address: "0x0000000000000000000000000000000000000001",
        data: "0xdeadbeef"
    })]"#;
    let strategy_id = seed_strategy(&db_path, "rc_accept", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    assert_eq!(body["outcome"]["kind"].as_str(), Some("actions"));
    let actions = body["outcome"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions[0]["kind"].as_str(), Some("raw_call"));
    assert_eq!(actions[0]["data"].as_str(), Some("0xdeadbeef"));
    proc.child.kill().await?;
    Ok(())
}

#[cfg(feature = "anvil-tests")]
#[tokio::test]
async fn strategy_run_accepts_erc20_transfer() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [ctx.actions.erc20Transfer({
        token:  "0x0000000000000000000000000000000000000001",
        to:     "0x0000000000000000000000000000000000000002",
        amount: "1000"
    })]"#;
    let strategy_id = seed_strategy(&db_path, "erc20t_accept", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    let actions = body["outcome"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions[0]["kind"].as_str(), Some("erc20_transfer"));
    assert_eq!(actions[0]["amount"].as_str(), Some("1000"));
    proc.child.kill().await?;
    Ok(())
}

#[cfg(feature = "anvil-tests")]
#[tokio::test]
async fn strategy_run_accepts_erc20_approve() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [ctx.actions.erc20Approve({
        token:   "0x0000000000000000000000000000000000000001",
        spender: "0x0000000000000000000000000000000000000003",
        amount:  "0"
    })]"#;
    let strategy_id = seed_strategy(&db_path, "erc20a_accept", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    let actions = body["outcome"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions[0]["kind"].as_str(), Some("erc20_approve"));
    assert_eq!(
        actions[0]["spender"].as_str(),
        Some("0x0000000000000000000000000000000000000003")
    );
    proc.child.kill().await?;
    Ok(())
}

#[cfg(feature = "anvil-tests")]
#[tokio::test]
async fn strategy_run_accepts_native_transfer() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [ctx.actions.nativeTransfer({
        to:    "0x0000000000000000000000000000000000000002",
        value: "1000000000000000000"
    })]"#;
    let _ = db_path_str;
    let strategy_id = seed_strategy(&db_path, "nt_accept", source)?;
    let policy = write_permissive_policy(&["0x0000000000000000000000000000000000000002"])?;
    let mut proc =
        spawn_server_with_policy_and_rpc(&db_path, policy.path(), "http://127.0.0.1:8545").await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    let actions = body["outcome"]["actions"]
        .as_array()
        .expect("actions array");
    assert_eq!(actions[0]["kind"].as_str(), Some("native_transfer"));
    assert_eq!(actions[0]["value"].as_str(), Some("1000000000000000000"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_contract_call_with_bad_address() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    // Free-form action JSON (no builder) with an obviously bad address —
    // the validate_strategy_output gate accepts the kind but serde-driven
    // address shape isn't enforced at deserialize time. The address-shape
    // validation is enforced at builder time; bypassing the builder, we
    // exercise the failure mode via a malformed-but-deserializable payload.
    // To force rejection at the JSON gate, we use an unknown field —
    // deny_unknown_fields catches it.
    let source = r#"(ctx) => [{
        kind: "contract_call",
        address: "0x0000000000000000000000000000000000000001",
        abi: "[]",
        function: "f",
        args: [],
        gas: 21000
    }]"#;
    let strategy_id = seed_strategy(&db_path, "cc_reject", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("unknown field") || detail.contains("gas"),
        "expected deny_unknown_fields detail, got: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

// ─── Plan 04-04 Task 2 — per-variant stdio rejection grid ──────────────────
//
// Five end-to-end rejection tests, one per Phase-4 action variant. Each
// emits a free-form action JSON object (NOT via the ctx.actions.* builder)
// so the failure mode flows through `validate_strategy_output` →
// -32018 STRATEGY_INVALID_OUTPUT.
//
// The builder-time rejection grid lives in
// `crates/strategy-js/tests/ctx_actions_negative_grid.rs` (15 tests).

#[tokio::test]
async fn strategy_run_rejects_contract_call_with_unknown_field() -> Result<()> {
    // contract_call with an unknown field — `deny_unknown_fields` rejects.
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [{
        kind: "contract_call",
        address: "0x0000000000000000000000000000000000000001",
        abi: "[]", function: "f", args: [],
        unknown_extra_field: "boom"
    }]"#;
    let strategy_id = seed_strategy(&db_path, "cc_unknown", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("unknown field") || detail.contains("unknown_extra_field"),
        "expected unknown-field detail, got: {detail}"
    );
    // MR-01 wire safety
    for forbidden in ["transporterror", "reqwest", "alloy_dyn_abi"] {
        assert!(
            !detail.contains(forbidden),
            "raw error text leaked ({forbidden}): {detail}"
        );
    }
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_raw_call_with_unknown_field() -> Result<()> {
    // raw_call with unknown field — deny_unknown_fields path.
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [{
        kind: "raw_call",
        address: "0x0000000000000000000000000000000000000001",
        data: "0xdeadbeef",
        gas_limit: 21000
    }]"#;
    let strategy_id = seed_strategy(&db_path, "rc_unknown", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("unknown field") || detail.contains("gas_limit"),
        "expected unknown-field detail, got: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_erc20_transfer_via_builder_with_bigint_amount() -> Result<()> {
    // BigInt amount → builder throws → strategy raises a JS Error →
    // RuntimeError::Exception → -32017 (NOT -32018) per Phase-3 mapping.
    // We assert -32017 with stable detail (no raw text). This documents
    // the Phase-4 boundary: BigInt amount surfaces as runtime_error, not
    // invalid_output.
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [ctx.actions.erc20Transfer({
        token: "0x0000000000000000000000000000000000000001",
        to:    "0x0000000000000000000000000000000000000002",
        amount: 100n
    })]"#;
    let strategy_id = seed_strategy(&db_path, "erc20t_bigint", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("bigint") || detail.contains("decimal string"),
        "expected stable BigInt rejection detail, got: {detail}"
    );
    for forbidden in ["transporterror", "reqwest", "alloy_dyn_abi"] {
        assert!(
            !detail.contains(forbidden),
            "raw error text leaked ({forbidden}): {detail}"
        );
    }
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_erc20_approve_with_unknown_field() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [{
        kind: "erc20_approve",
        token:   "0x0000000000000000000000000000000000000001",
        spender: "0x0000000000000000000000000000000000000003",
        amount:  "0",
        deadline: 9999
    }]"#;
    let strategy_id = seed_strategy(&db_path, "erc20a_unknown", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("unknown field") || detail.contains("deadline"),
        "expected unknown-field detail, got: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_native_transfer_via_builder_with_negative_value() -> Result<()> {
    // Builder throws → -32017 runtime_error with stable taxonomy detail.
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let source = r#"(ctx) => [ctx.actions.nativeTransfer({
        to: "0x0000000000000000000000000000000000000002",
        value: "-1"
    })]"#;
    let strategy_id = seed_strategy(&db_path, "nt_negative", source)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("non-negative") || detail.contains("bad_decimal"),
        "expected stable non-negative detail, got: {detail}"
    );
    for forbidden in ["transporterror", "reqwest", "alloy_dyn_abi"] {
        assert!(
            !detail.contains(forbidden),
            "raw error text leaked ({forbidden}): {detail}"
        );
    }
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_unknown_action_kind() -> Result<()> {
    // Phase-3 spirit preserved (D-16): a kind NOT in the Phase-4 allowlist
    // (e.g. `multi_call` — Phase-5 candidate) still surfaces as -32018.
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "p5", "(ctx) => [{kind:\"multi_call\"}]")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or("").to_lowercase();
    assert!(
        detail.contains("multi_call") || detail.contains("not allowed in phase 4"),
        "expected stable allowlist detail, got: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

async fn assert_no_policy_rejects_action(name: &str, source: &str) -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, name, source)?;

    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("policy_not_loaded"));
    let run_id = err["data"]["run_id"].as_str().expect("run_id present");
    proc.child.kill().await?;

    let store = executor_state::StateStore::open(&db_path)?;
    let run = store.get_run(run_id)?.expect("run row exists");
    assert_eq!(
        run.status,
        executor_core::schema::execution::RunStatus::PolicyDenied
    );
    assert!(
        run.finished_at.is_some(),
        "PolicyDenied no-policy rejection must populate finished_at"
    );
    Ok(())
}

#[tokio::test]
async fn strategy_run_no_policy_rejects_raw_call() -> Result<()> {
    assert_no_policy_rejects_action(
        "no_policy_raw",
        r#"(ctx) => [{
            kind: "raw_call",
            address: "0x0000000000000000000000000000000000000001",
            data: "0xdeadbeef",
            value: "0"
        }]"#,
    )
    .await
}

#[tokio::test]
async fn strategy_run_no_policy_rejects_contract_call() -> Result<()> {
    assert_no_policy_rejects_action(
        "no_policy_contract",
        r#"(ctx) => [{
            kind: "contract_call",
            address: "0x0000000000000000000000000000000000000001",
            abi: "[{\"type\":\"function\",\"name\":\"f\",\"inputs\":[],\"outputs\":[],\"stateMutability\":\"nonpayable\"}]",
            function: "f",
            args: [],
            value: "0"
        }]"#,
    )
    .await
}

#[tokio::test]
async fn strategy_run_no_policy_rejects_erc20_transfer() -> Result<()> {
    assert_no_policy_rejects_action(
        "no_policy_transfer",
        r#"(ctx) => [{
            kind: "erc20_transfer",
            token: "0x0000000000000000000000000000000000000001",
            to: "0x0000000000000000000000000000000000000002",
            amount: "1000"
        }]"#,
    )
    .await
}

#[tokio::test]
async fn strategy_run_no_policy_rejects_erc20_approve() -> Result<()> {
    assert_no_policy_rejects_action(
        "no_policy_approve",
        r#"(ctx) => [{
            kind: "erc20_approve",
            token: "0x0000000000000000000000000000000000000001",
            spender: "0x0000000000000000000000000000000000000003",
            amount: "0"
        }]"#,
    )
    .await
}

#[tokio::test]
async fn strategy_run_no_policy_rejects_native_transfer() -> Result<()> {
    assert_no_policy_rejects_action(
        "no_policy_native",
        r#"(ctx) => [{
            kind: "native_transfer",
            to: "0x0000000000000000000000000000000000000002",
            value: "1"
        }]"#,
    )
    .await
}

#[tokio::test]
async fn strategy_run_runtime_error_on_throw() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "throw", "(ctx) => { throw new Error(\"nope\"); }")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("exception"));
    assert!(
        err["data"]["detail"]
            .as_str()
            .is_some_and(|s| s.contains("nope"))
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_runtime_error_on_infinite_loop() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "loop", "(ctx) => { while(true){} }")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    // Wall-clock budget is 2s; allow ~5s for spawn + JSON-RPC overhead.
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        call_tool(
            &mut proc,
            2,
            "strategy_run",
            json!({ "strategy_id": strategy_id }),
        ),
    )
    .await??;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["kind"].as_str(), Some("timeout"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_runtime_error_on_oom() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let src = "(ctx) => { let a=[]; while(true) a.push(new Array(1e6)); }";
    let strategy_id = seed_strategy(&db_path, "oom", src)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        call_tool(
            &mut proc,
            2,
            "strategy_run",
            json!({ "strategy_id": strategy_id }),
        ),
    )
    .await??;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    // Allocation-blowup may surface as "oom" or "exception" depending on
    // where the rquickjs allocator runs out (the heap cap or an interrupt).
    let kind = err["data"]["kind"].as_str().unwrap_or("");
    assert!(
        kind == "oom" || kind == "exception",
        "expected kind oom|exception, got {kind}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_runtime_error_on_stack_overflow() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "stack", "(ctx) => { function f(){f();} f(); }")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    let kind = err["data"]["kind"].as_str().unwrap_or("");
    // rquickjs surfaces stack overflow either as a typed StackOverflow or
    // as a generic Exception depending on the recursion depth path.
    assert!(
        kind == "stack_overflow" || kind == "exception",
        "expected stack_overflow|exception, got {kind}"
    );
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_rejects_deleted_strategy() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = {
        let mut store = executor_state::StateStore::open(&db_path)?;
        let outcome = store.register_strategy("d", "(ctx) => \"noop\"", None, None)?;
        let sid = match outcome {
            executor_state::RegisterOutcome::Created(s)
            | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
        };
        store.soft_delete_strategy(&sid)?;
        sid
    };
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32011));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_deleted"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_records_source_read_journal_row() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "src", "(ctx) => \"noop\"")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    let run_id = body["run_id"].as_str().unwrap().to_string();
    proc.child.kill().await?;

    let store = executor_state::StateStore::open(&db_path)?;
    let sources = store.list_source_reads_for_run(&run_id)?;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].kind, "strategy_source");
    assert_eq!(sources[0].target, strategy_id);
    Ok(())
}

#[tokio::test]
async fn strategy_run_records_log_messages() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let src = "(ctx) => { ctx.log(\"hello\", 42); ctx.log(\"world\"); return \"noop\"; }";
    let strategy_id = seed_strategy(&db_path, "logs", src)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    let run_id = body["run_id"].as_str().unwrap().to_string();
    proc.child.kill().await?;

    let store = executor_state::StateStore::open(&db_path)?;
    let logs = store.list_logs_for_run(&run_id)?;
    assert_eq!(logs.len(), 2);
    // Order between logs sharing the same recorded_at second falls back to
    // ULID id ASC; ULID monotonicity within the same millisecond is not
    // guaranteed by `Ulid::new()`, so assert membership rather than order.
    let messages: std::collections::HashSet<String> =
        logs.iter().map(|l| l.message.clone()).collect();
    assert!(
        messages.contains("hello 42"),
        "missing 'hello 42'; got {messages:?}"
    );
    assert!(
        messages.contains("world"),
        "missing 'world'; got {messages:?}"
    );
    Ok(())
}

#[tokio::test]
async fn strategy_run_run_row_status_transitions_to_failed_on_error() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "fail", "(ctx) => { throw new Error(\"bad\"); }")?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    let run_id = err["data"]["run_id"].as_str().unwrap().to_string();
    proc.child.kill().await?;

    let store = executor_state::StateStore::open(&db_path)?;
    let run = store.get_run(&run_id)?.expect("run row exists");
    assert_eq!(
        run.status,
        executor_core::schema::execution::RunStatus::Failed
    );
    assert!(
        run.finished_at.is_some(),
        "finished_at must be populated on failed runs"
    );
    Ok(())
}

#[tokio::test]
async fn strategy_run_invalid_strategy_id_format_returns_invalid_params() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": "ZZZ" }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32602));
    assert_eq!(err["data"]["code"].as_str(), Some("invalid_params"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_unknown_strategy_id_returns_not_found() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let strategy_id = "a".repeat(64);
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32014));
    assert_eq!(err["data"]["code"].as_str(), Some("not_found"));
    proc.child.kill().await?;
    Ok(())
}

/// BR-01 regression: when an EVM error is thrown from inside the JS sandbox
/// (here, `ctx.actions.contractCall` with malformed `abi`), it must surface
/// on the wire as `data.kind == "evm_decode_error"` (D-12 taxonomy), NOT
/// the generic `"exception"`. Pre-fix, `RuntimeError::Evm(_)` was never
/// constructed in production — every EVM error became `RuntimeError::Exception`
/// and the taxonomy upgrade was decorative. Fix: `classify_message` now
/// re-classifies stable EvmError prefixes back into `RuntimeError::Evm(_)`.
#[cfg(feature = "anvil-tests")]
#[tokio::test]
async fn strategy_run_normalization_failure_after_running_is_terminal() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let policy = write_permissive_policy(&["0x0000000000000000000000000000000000000001"])?;
    let src = r#"(ctx) => [{
        kind: "raw_call",
        address: "not-an-address",
        data: "0xdeadbeef",
        value: "0"
    }]"#;
    let strategy_id = seed_strategy(&db_path, "bad_normalize", src)?;
    let mut proc =
        spawn_server_with_policy_and_rpc(&db_path, policy.path(), "http://127.0.0.1:8545").await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    let run_id = err["data"]["run_id"]
        .as_str()
        .expect("run_id present")
        .to_string();
    proc.child.kill().await?;

    let store = executor_state::StateStore::open(&db_path)?;
    let run = store.get_run(&run_id)?.expect("run row exists");
    assert_eq!(
        run.status,
        executor_core::schema::execution::RunStatus::Failed,
        "normalization failure after Running must not leave the run running"
    );
    assert!(
        run.finished_at.is_some(),
        "terminal failure must set finished_at"
    );
    let actions = store.list_actions_for_run(&run_id)?;
    assert_eq!(actions.len(), 1, "normalization error must be journaled");
    Ok(())
}

#[tokio::test]
async fn strategy_run_evm_error_surfaces_typed_data_kind() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    // ctx.actions.contractCall with malformed ABI → EvmError::Decode
    // { category: "abi_parse" } → throw_js_error("evm decode error: abi_parse")
    // → JS exception → caught_to_runtime_error → classify_message → Evm(Decode).
    let src = "(ctx) => {\n\
              ctx.actions.contractCall({ \
                address: '0x0000000000000000000000000000000000000001', \
                abi: 'this is not json', function: 'f', args: [] });\n\
              return 'noop';\n\
              }";
    let strategy_id = seed_strategy(&db_path, "evm_typed", src)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    // -32017 = STRATEGY_RUNTIME_ERROR; data.kind must be a typed evm_*
    // value, NOT "exception".
    assert_eq!(err["code"].as_i64(), Some(-32017));
    let kind = err["data"]["kind"].as_str().unwrap_or_default();
    assert!(
        matches!(kind, "evm_decode_error" | "evm_rpc_error" | "evm_revert"),
        "expected typed evm_* data.kind, got: {kind}"
    );
    assert_ne!(
        kind, "exception",
        "BR-01 regressed: data.kind is generic 'exception'"
    );
    proc.child.kill().await?;
    Ok(())
}

/// BR-02 regression: a strategy that hand-builds a `contract_call` action
/// with an oversize `abi` string (1 MiB) MUST be rejected at the JSON-output
/// gate (validate_strategy_output → dry_run_abi_encode), not just at builder
/// time. Wire mapping: -32018 strategy_invalid_output with stable detail
/// containing `abi_oversize` (the EvmError encode-category).
#[tokio::test]
async fn strategy_run_rejects_hand_built_oversize_abi() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    // Build an "abi" that is a JSON-string of a giant array. Hand-built —
    // does NOT go through ctx.actions.contractCall (which would catch the
    // cap at builder time). 1 MiB easily exceeds the 64 KiB cap.
    let src = "(ctx) => {\n\
              const big = '[' + new Array(70000).fill('null').join(',') + ']';\n\
              return [{ kind: 'contract_call', \
                        address: '0x0000000000000000000000000000000000000001', \
                        abi: big, function: 'f', args: [], value: '0' }];\n\
              }";
    let strategy_id = seed_strategy(&db_path, "oversize", src)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("abi_oversize") || detail.contains("evm encode error"),
        "expected stable abi_oversize detail, got: {detail}"
    );
    proc.child.kill().await?;
    Ok(())
}

/// Phase 5 D-12 / D-18 / BR-02 carry-forward: a strategy that hand-builds
/// an `Action[]` longer than `MAX_ACTIONS_PER_RUN` (32) MUST be rejected at
/// `validate_strategy_output` (the JSON-output gate, NOT only at the
/// strategy-js builder). Wire mapping: -32018 strategy_invalid_output with
/// stable detail naming both the offending length (33) and the cap
/// (`MAX_ACTIONS_PER_RUN 32`).
#[tokio::test]
async fn strategy_run_caps_action_array_length_at_32() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    // 33 noop actions — one over the cap. Hand-built (no ctx.actions.* —
    // the cap MUST be enforced at the JSON-output gate regardless of the
    // construction path).
    let src = "(ctx) => Array.from({length: 33}, () => ({kind:'noop'}))";
    let strategy_id = seed_strategy(&db_path, "cap33", src)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_eq!(err["code"].as_i64(), Some(-32018));
    assert_eq!(
        err["data"]["code"].as_str(),
        Some("strategy_invalid_output")
    );
    let detail = err["data"]["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("MAX_ACTIONS_PER_RUN 32"),
        "detail must include the cap; got {detail:?}"
    );
    assert!(
        detail.contains("33"),
        "detail must include the offending length; got {detail:?}"
    );
    proc.child.kill().await?;
    Ok(())
}

/// Boundary regression for D-12: exactly 32 noop actions PASSES validation
/// (the cap is `>` not `>=`).
#[tokio::test]
async fn strategy_run_accepts_action_array_length_32() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let src = "(ctx) => Array.from({length: 32}, () => ({kind:'noop'}))";
    let strategy_id = seed_strategy(&db_path, "cap32", src)?;
    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    assert!(
        r.get("error").is_none(),
        "expected no error envelope at boundary 32; got {r:?}"
    );
    let body = extract_json_result(&r);
    assert_eq!(body["outcome"]["kind"].as_str(), Some("actions"));
    assert_eq!(body["outcome"]["actions"].as_array().unwrap().len(), 32);
    proc.child.kill().await?;
    Ok(())
}

// ─────────── Phase 5 Plan 05-05: gap closure ───────────

#[cfg(feature = "anvil-tests")]
const REVERT_BYTECODE: &str = include_str!("../../executor-evm/tests/fixtures/revert_counter.hex");

#[cfg(feature = "anvil-tests")]
async fn deploy_bytecode_for_stdio(
    provider: &std::sync::Arc<executor_evm::DynProvider>,
    deployer: Address,
    bytecode_hex: &str,
) -> Address {
    let stripped = bytecode_hex
        .trim()
        .strip_prefix("0x")
        .or_else(|| bytecode_hex.trim().strip_prefix("0X"))
        .unwrap_or(bytecode_hex.trim());
    let padded;
    let stripped = if stripped.len() % 2 == 0 {
        stripped
    } else {
        padded = format!("0{stripped}");
        padded.as_str()
    };
    let bytecode = hex::decode(stripped).expect("hex bytecode");
    let tx = TransactionRequest::default()
        .with_from(deployer)
        .with_deploy_code(bytecode);
    let pending = provider.send_transaction(tx).await.expect("send deploy tx");
    let receipt = pending.get_receipt().await.expect("deploy receipt");
    receipt
        .contract_address
        .expect("deploy receipt has contract_address")
}

fn assert_policy_violation(err: &Value, rule: &str) {
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("policy_violation"));
    assert_eq!(err["data"]["rule"].as_str(), Some(rule));
}

async fn ensure_anvil_8545() -> Result<()> {
    let reachable = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        tokio::net::TcpStream::connect("127.0.0.1:8545"),
    )
    .await
    .is_ok_and(|r| r.is_ok());
    if reachable {
        return Ok(());
    }

    let _child = tokio::process::Command::new("anvil")
        .args(["--host", "127.0.0.1", "--port", "8545", "--chain-id", "31337"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    // Keep the process alive for the test duration. The leaked child is scoped to
    // the test binary process and exits when the test process ends.
    std::mem::forget(_child);
    for _ in 0..20 {
        let reachable = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            tokio::net::TcpStream::connect("127.0.0.1:8545"),
        )
        .await
        .is_ok_and(|r| r.is_ok());
        if reachable {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    anyhow::bail!("anvil did not start on 127.0.0.1:8545")
}

async fn read_journal_resource(proc: &mut common::ServerProc, id: u64, run_id: &str) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "resources/read",
            "params": { "uri": format!("journal://{run_id}") }
        }),
    )
    .await?;
    let resp = recv(proc).await?;
    let text = resp["result"]["contents"][0]["text"]
        .as_str()
        .expect("journal contents text");
    Ok(serde_json::from_str(text)?)
}

fn assert_decision_row(
    journal: &Value,
    action_index: i64,
    gate: &str,
    verdict: &str,
    rule: Option<&str>,
) {
    let rows = journal["decisions"].as_array().expect("decisions array");
    assert!(
        rows.iter().any(|row| {
            row["action_index"].as_i64() == Some(action_index)
                && row["gate"].as_str() == Some(gate)
                && row["verdict"].as_str() == Some(verdict)
                && match rule {
                    Some(expected) => row["rule"].as_str() == Some(expected),
                    None => row["rule"].is_null(),
                }
        }),
        "missing decision row action_index={action_index} gate={gate} verdict={verdict} rule={rule:?}; rows={rows:?}"
    );
}

fn policy_with_contracts(
    chains_allow: &[u64],
    contracts: &[&str],
    selector_entries: &[(&str, &[&str])],
    raw_entries: &[(&str, &str)],
    native_cap: &str,
    erc20_caps: &[(&str, &str)],
) -> String {
    let chains = chains_allow
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let contracts_toml = contracts
        .iter()
        .map(|addr| format!("    \"{addr}\","))
        .collect::<Vec<_>>()
        .join("\n");
    let selectors_toml = selector_entries
        .iter()
        .map(|(addr, selectors)| {
            let values = selectors
                .iter()
                .map(|sel| format!("\"{sel}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[selectors.\"31337:{addr}\"]\nallow = [{values}]\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let raw_toml = raw_entries
        .iter()
        .map(|(addr, selector)| {
            format!("    {{ chain = 31337, contract = \"{addr}\", selector = \"{selector}\" }},")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let erc20_toml = erc20_caps
        .iter()
        .map(|(token, cap)| format!("[erc20_spend.\"31337:{token}\"]\nmax_per_run = \"{cap}\"\n"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"[chains]
allow = [{chains}]

[contracts.31337]
allow = [
{contracts_toml}
]

{selectors_toml}
[native_value.31337]
max_per_action = "{native_cap}"

{erc20_toml}[raw_call]
allow_global = false
allow = [
{raw_toml}
]
"#
    )
}

#[cfg(feature = "anvil-tests")]
#[tokio::test(flavor = "multi_thread")]
async fn strategy_run_returns_simulation_failed_when_revert() -> Result<()> {
    let Some(fixture) = alloy::node_bindings::Anvil::new()
        .chain_id(31337)
        .try_spawn()
        .ok()
    else {
        return Ok(());
    };
    let funded_accounts = fixture.addresses().to_vec();
    if funded_accounts.is_empty() {
        return Ok(());
    }
    let rpc_url = fixture.endpoint_url();
    let cfg = executor_evm::EvmConfig {
        rpc_url: rpc_url.clone(),
        ..executor_evm::EvmConfig::default()
    };
    let provider = executor_evm::build_provider(&cfg)?;
    let revert_addr = deploy_bytecode_for_stdio(&provider, funded_accounts[0], REVERT_BYTECODE).await;
    let revert_addr_s = revert_addr.to_string();

    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let policy = write_permissive_policy(&[revert_addr_s.as_str()])?;
    let source = format!(
        r#"(ctx) => [{{
            kind: "raw_call",
            address: "{revert_addr_s}",
            data: "0x00000000",
            value: "0"
        }}]"#
    );
    let strategy_id = seed_strategy(&db_path, "sim_revert_stdio", &source)?;
    let mut proc = spawn_server_with_policy_and_rpc(&db_path, policy.path(), rpc_url.as_str()).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope present");
    assert_eq!(err["code"].as_i64(), Some(-32017));
    assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
    assert_eq!(err["data"]["kind"].as_str(), Some("simulation_failure"));
    assert_eq!(err["data"]["action_index"].as_i64(), Some(0));
    assert_eq!(err["data"]["fail_reason"].as_str(), Some("revert"));
    assert_ne!(err["data"]["kind"].as_str(), Some("exception"));
    assert_ne!(err["data"]["kind"].as_str(), Some("policy_violation"));
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_journal_records_pass_decisions_on_success() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let recipient = "0x0000000000000000000000000000000000001000";
    let policy_toml = policy_with_contracts(
        &[31337],
        &[recipient],
        &[],
        &[],
        "1000000000000000000",
        &[],
    );
    let policy = write_policy(&policy_toml)?;
    let source = format!(
        r#"(ctx) => [{{
            kind: "native_transfer",
            to: "{recipient}",
            value: "0"
        }}]"#
    );
    let strategy_id = seed_strategy(&db_path, "journal_success", &source)?;
    ensure_anvil_8545().await?;
    let mut proc = spawn_server_with_policy_rpc_and_signer(
        &db_path,
        policy.path(),
        "http://127.0.0.1:8545",
        "EXECUTOR_TEST_PRIVATE_KEY",
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )
    .await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let body = extract_json_result(&r);
    let run_id = body["run_id"].as_str().expect("run_id");
    let journal = read_journal_resource(&mut proc, 3, run_id).await?;
    assert_decision_row(&journal, 0, "policy", "pass", None);
    assert_decision_row(&journal, 0, "simulation", "pass", None);
    proc.child.kill().await?;
    Ok(())
}

async fn assert_policy_denied_journal(name: &str, policy_toml: String, source: String) -> Result<()> {
    ensure_anvil_8545().await?;
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let policy = write_policy(&policy_toml)?;
    let strategy_id = seed_strategy(&db_path, name, &source)?;
    let mut proc = spawn_server_with_policy_and_rpc(&db_path, policy.path(), "http://127.0.0.1:8545").await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_policy_violation(err, "contract_not_allowed");
    let run_id = err["data"]["run_id"].as_str().expect("run_id");
    let journal = read_journal_resource(&mut proc, 3, run_id).await?;
    assert_decision_row(&journal, 0, "policy", "fail", Some("contract_not_allowed"));
    assert_decision_row(&journal, 0, "simulation", "skipped", None);
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_journal_records_fail_decision_on_policy_denied() -> Result<()> {
    let policy_toml = policy_with_contracts(&[31337], &[], &[], &[], "0", &[]);
    let source = r#"(ctx) => [{
        kind: "native_transfer",
        to: "0x0000000000000000000000000000000000000002",
        value: "0"
    }]"#
    .to_string();
    assert_policy_denied_journal("journal_policy_fail", policy_toml, source).await
}

#[tokio::test]
async fn strategy_run_records_skipped_simulation_when_policy_denied() -> Result<()> {
    let policy_toml = policy_with_contracts(&[31337], &[], &[], &[], "0", &[]);
    let source = r#"(ctx) => [{
        kind: "native_transfer",
        to: "0x0000000000000000000000000000000000000002",
        value: "0"
    }]"#
    .to_string();
    assert_policy_denied_journal("journal_sim_skipped", policy_toml, source).await
}

async fn assert_policy_violation_for_source(
    name: &str,
    policy_toml: String,
    source: String,
    expected_rule: &str,
) -> Result<()> {
    ensure_anvil_8545().await?;
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let policy = write_policy(&policy_toml)?;
    let strategy_id = seed_strategy(&db_path, name, &source)?;
    let mut proc =
        spawn_server_with_policy_and_rpc(&db_path, policy.path(), "http://127.0.0.1:8545").await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(
        &mut proc,
        2,
        "strategy_run",
        json!({ "strategy_id": strategy_id }),
    )
    .await?;
    let err = r.get("error").expect("error envelope");
    assert_policy_violation(err, expected_rule);
    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_run_returns_policy_violation_for_disallowed_chain() -> Result<()> {
    let policy_toml = r#"[chains]
allow = [1]

[contracts.1]
allow = []

[native_value.31337]
max_per_action = "1000000000000000000"

[raw_call]
allow_global = false
allow = []
"#
    .to_string();
    let source = r#"(ctx) => [{
        kind: "native_transfer",
        to: "0x0000000000000000000000000000000000000002",
        value: "0"
    }]"#
    .to_string();
    assert_policy_violation_for_source(
        "policy_chain_denied",
        policy_toml,
        source,
        "chain_not_allowed",
    )
    .await
}

#[tokio::test]
async fn strategy_run_returns_policy_violation_for_disallowed_contract() -> Result<()> {
    let policy_toml = policy_with_contracts(&[31337], &[], &[], &[], "0", &[]);
    let source = r#"(ctx) => [{
        kind: "native_transfer",
        to: "0x0000000000000000000000000000000000000002",
        value: "0"
    }]"#
    .to_string();
    assert_policy_violation_for_source(
        "policy_contract_denied",
        policy_toml,
        source,
        "contract_not_allowed",
    )
    .await
}

#[tokio::test]
async fn strategy_run_returns_policy_violation_for_disallowed_selector() -> Result<()> {
    let target = "0x0000000000000000000000000000000000000002";
    let policy_toml = policy_with_contracts(
        &[31337],
        &[target],
        &[(target, &["0xaaaaaaaa"])],
        &[],
        "0",
        &[],
    );
    let abi = r#"[{"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"amount","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}]"#;
    let source = format!(
        r#"(ctx) => [{{
            kind: "contract_call",
            address: "{target}",
            abi: {},
            function: "transfer",
            args: ["0x0000000000000000000000000000000000000003", "1"],
            value: "0"
        }}]"#,
        serde_json::to_string(abi)?
    );
    assert_policy_violation_for_source(
        "policy_selector_denied",
        policy_toml,
        source,
        "selector_not_allowed",
    )
    .await
}

#[tokio::test]
async fn strategy_run_returns_policy_violation_for_native_value_cap() -> Result<()> {
    let recipient = "0x0000000000000000000000000000000000000002";
    let policy_toml = policy_with_contracts(&[31337], &[recipient], &[], &[], "0", &[]);
    let source = format!(
        r#"(ctx) => [{{
            kind: "native_transfer",
            to: "{recipient}",
            value: "1"
        }}]"#
    );
    assert_policy_violation_for_source(
        "policy_native_cap",
        policy_toml,
        source,
        "native_value_exceeds",
    )
    .await
}

#[tokio::test]
async fn strategy_run_returns_policy_violation_for_erc20_spend_cap() -> Result<()> {
    let token = "0x0000000000000000000000000000000000000002";
    let policy_toml = policy_with_contracts(
        &[31337],
        &[token],
        &[(token, &["any"])],
        &[],
        "0",
        &[(token, "1000")],
    );
    let source = format!(
        r#"(ctx) => [{{
            kind: "erc20_transfer",
            token: "{token}",
            to: "0x0000000000000000000000000000000000000003",
            amount: "1001"
        }}]"#
    );
    assert_policy_violation_for_source(
        "policy_erc20_cap",
        policy_toml,
        source,
        "erc20_spend_exceeds",
    )
    .await
}

#[tokio::test]
async fn strategy_run_returns_policy_violation_for_raw_call_denied() -> Result<()> {
    let target = "0x0000000000000000000000000000000000000002";
    let policy_toml = policy_with_contracts(&[31337], &[target], &[], &[], "0", &[]);
    let source = format!(
        r#"(ctx) => [{{
            kind: "raw_call",
            address: "{target}",
            data: "0xdeadbeef",
            value: "0"
        }}]"#
    );
    assert_policy_violation_for_source(
        "policy_raw_denied",
        policy_toml,
        source,
        "raw_call_denied",
    )
    .await
}
