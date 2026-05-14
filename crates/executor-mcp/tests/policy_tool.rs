//! v1.5 Track 1A — `policy_set` MCP tool + `policy://current` /
//! `policy://history` resource integration tests.
//!
//! Covers:
//!   - Calling `policy_set` over stdio with a fresh body returns the expected
//!     response shape (revision_id, diff, impact fields).
//!   - A follow-up `policy_set` with a modification produces a non-empty
//!     diff and the `previous_revision_id` is the prior revision.
//!   - `policy://current` reflects the latest set.
//!   - `policy://history?limit=2` returns 2 entries newest-first.
//!   - First boot with a `[policy].path` TOML file at an empty DB seeds
//!     the first revision automatically.
//!   - `policy_set` description carries the `[DESTRUCTIVE]` marker.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{
    call_tool, extract_json_result, extract_resource_json, initialize, read_resource,
    spawn_server_with_state,
};

fn fresh_policy_body(chain: u64, contract: &str) -> Value {
    json!({
        "chains": { "allow": [chain] },
        "contracts": { chain.to_string(): { "allow": [contract] } },
        "selectors": {},
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    })
}

#[tokio::test]
async fn policy_set_response_shape_on_fresh_db() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let body = fresh_policy_body(31337, "0x5fbdb2315678afecb367f032d93f642f64180aa3");
    let r = call_tool(
        &mut proc,
        10,
        "policy_set",
        json!({ "policy": body, "rationale": "initial set" }),
    )
    .await?;
    assert!(
        r.get("error").is_none(),
        "policy_set should succeed on fresh DB; got: {r}"
    );
    let resp = extract_json_result(&r);
    assert!(resp["previous_revision_id"].is_null(), "no prior revision");
    assert!(
        resp["new_revision_id"].as_str().is_some_and(|s| !s.is_empty()),
        "new_revision_id present and non-empty: {resp}",
    );
    assert!(
        resp["applied_at"].as_str().is_some(),
        "applied_at present: {resp}",
    );
    assert!(resp["diff"].is_array(), "diff is array");
    assert!(resp["impact"]["newly_satisfied_strategies"].is_array());
    assert!(resp["impact"]["newly_unsatisfied_strategies"].is_array());
    assert!(resp["impact"]["new_capabilities_granted"].is_array());

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn second_policy_set_produces_non_empty_diff() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let body_a = fresh_policy_body(31337, "0x5fbdb2315678afecb367f032d93f642f64180aa3");
    let r1 = call_tool(&mut proc, 11, "policy_set", json!({ "policy": body_a })).await?;
    let r1_body = extract_json_result(&r1);
    let first_rev_id = r1_body["new_revision_id"].as_str().unwrap().to_string();

    let body_b = json!({
        "chains": { "allow": [31337] },
        "contracts": { "31337": { "allow": [
            "0x5fbdb2315678afecb367f032d93f642f64180aa3",
            "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",
        ]}},
        "selectors": {},
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    });
    let r2 = call_tool(
        &mut proc,
        12,
        "policy_set",
        json!({ "policy": body_b, "rationale": "add second contract" }),
    )
    .await?;
    let r2_body = extract_json_result(&r2);

    assert_eq!(
        r2_body["previous_revision_id"].as_str(),
        Some(first_rev_id.as_str()),
        "previous_revision_id must match first revision id",
    );
    let diff = r2_body["diff"].as_array().expect("diff array");
    assert!(!diff.is_empty(), "diff should be non-empty for a real change");

    let caps = r2_body["impact"]["new_capabilities_granted"]
        .as_array()
        .expect("capabilities array");
    assert!(
        caps.iter().any(|c| {
            let s = c.as_str().unwrap_or("");
            s.contains("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512")
        }),
        "new_capabilities_granted should mention the newly-allowed contract; got: {caps:?}",
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_current_reflects_latest_set() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let body = fresh_policy_body(31337, "0x5fbdb2315678afecb367f032d93f642f64180aa3");
    let _ = call_tool(&mut proc, 20, "policy_set", json!({ "policy": body })).await?;

    let r = read_resource(&mut proc, 21, "policy://current").await?;
    assert!(r.get("error").is_none(), "policy://current should succeed: {r}");
    let cur = extract_resource_json(&r);
    assert_eq!(cur["loaded"], json!(true));
    assert!(cur["revision_id"].as_str().is_some_and(|s| !s.is_empty()));
    assert_eq!(cur["policy"]["chains"]["allow"], json!([31337]));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_history_returns_newest_first_limited() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    for i in 0..3u32 {
        let body = json!({
            "chains": { "allow": [31337] },
            "contracts": { "31337": { "allow": [
                format!("0x{:040x}", i + 1),
            ]}},
            "selectors": {},
            "native_value": {},
            "erc20_spend": {},
            "raw_call": { "allow_global": false, "allow": [] },
        });
        let _ = call_tool(
            &mut proc,
            30 + i as u64,
            "policy_set",
            json!({ "policy": body, "rationale": format!("rev {i}") }),
        )
        .await?;
        // Force distinct millisecond timestamps so the newest-first
        // assertion below is robust against same-ms ULID collisions.
        tokio::time::sleep(std::time::Duration::from_millis(6)).await;
    }

    let r = read_resource(&mut proc, 40, "policy://history?limit=2").await?;
    let body = extract_resource_json(&r);
    let revs = body["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 2, "limit=2 honoured: {body}");
    // The first one should be the active row (the most recent).
    assert_eq!(revs[0]["is_active"], json!(true));
    assert_eq!(revs[1]["is_active"], json!(false));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn first_boot_toml_imports_as_first_revision() -> Result<()> {
    // Write a temp policy.toml AND a temp config that points at it. The
    // server should auto-import on first boot when `policies` is empty.
    let tmp = tempfile::tempdir()?;
    let db_path = tmp.path().join("state.db");
    let policy_path = tmp.path().join("policy.toml");
    let policy_toml = r#"
[chains]
allow = [31337]

[contracts.31337]
allow = ["0x5fbdb2315678afecb367f032d93f642f64180aa3"]
"#;
    std::fs::write(&policy_path, policy_toml)?;

    let cfg = format!(
        "[state]\npath = \"{}\"\n[policy]\npath = \"{}\"\n",
        db_path.to_str().unwrap(),
        policy_path.to_str().unwrap(),
    );
    let mut proc = common::spawn_server_with_config_text(&cfg).await?;
    let _ = initialize(&mut proc).await?;

    let r = read_resource(&mut proc, 50, "policy://current").await?;
    let cur = extract_resource_json(&r);
    assert_eq!(cur["loaded"], json!(true), "TOML import should populate policy");
    assert_eq!(cur["policy"]["chains"]["allow"], json!([31337]));
    assert_eq!(
        cur["rationale"].as_str(),
        Some("initial import from .local/policy.toml"),
    );

    // Verify history has exactly one revision.
    let r = read_resource(&mut proc, 51, "policy://history").await?;
    let hist = extract_resource_json(&r);
    assert_eq!(hist["count"], json!(1));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_set_description_carries_destructive_marker() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    common::send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 60, "method": "tools/list" }),
    )
    .await?;
    let r = common::recv(&mut proc).await?;
    let tools = r["result"]["tools"].as_array().expect("tools");
    let policy_set = tools
        .iter()
        .find(|t| t["name"].as_str() == Some("policy_set"))
        .expect("policy_set must be in tools/list");
    let desc = policy_set["description"].as_str().unwrap_or("");
    assert!(
        desc.starts_with("[DESTRUCTIVE]"),
        "policy_set description must start with [DESTRUCTIVE] marker; got: {desc:?}",
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_set_rejects_malformed_body() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Missing the [contracts.31337] subtable that Pitfall P-10 requires
    // when [chains.allow] lists 31337 — must surface as invalid_params,
    // NOT as a successful write.
    let body = json!({
        "chains": { "allow": [31337] },
        "contracts": {},
        "selectors": {},
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    });
    let r = call_tool(&mut proc, 70, "policy_set", json!({ "policy": body })).await?;
    assert!(r.get("error").is_some(), "malformed body must error: {r}");

    // Active row should still be absent — failed validation must not write.
    let cur_r = read_resource(&mut proc, 71, "policy://current").await?;
    let cur = extract_resource_json(&cur_r);
    assert_eq!(cur["loaded"], json!(false));

    proc.child.kill().await?;
    Ok(())
}
