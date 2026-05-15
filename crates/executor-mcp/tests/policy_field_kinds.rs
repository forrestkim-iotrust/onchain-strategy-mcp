//! v1.13 Track P2 — backend `_field_kinds` emit on policy responses.
//!
//! The structure-aware dashboard renderer (Track P1) formats JSON
//! responses based on field-name hints. To prevent drift when the backend
//! grows new field names, `policy://current` and `policy://history`
//! declare an authoritative `_field_kinds` map at the response top level.
//! The frontend merges these with its built-in defaults; backend takes
//! precedence.
//!
//! Acceptance:
//!   1. `policy://current` (any state) carries `_field_kinds` with the
//!      six v1.13 kinds.
//!   2. `policy://history` carries the same `_field_kinds` shape.
//!   3. `_field_kinds["address"]` includes at least "address" and "to".
//!   4. The existing body fields are untouched (additive only).

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{
    call_tool, extract_resource_json, initialize, read_resource, spawn_server_with_state,
};

const REQUIRED_KINDS: &[&str] = &[
    "chain_id",
    "address",
    "selector",
    "wei_amount",
    "timestamp",
    "hash",
];

fn minimal_policy() -> Value {
    json!({
        "chains": { "allow": [31337] },
        "contracts": {
            "31337": { "allow": ["0xaaaa000000000000000000000000000000000001"] }
        },
        "selectors": {},
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    })
}

fn assert_field_kinds_shape(body: &Value) {
    let fk = &body["_field_kinds"];
    assert!(
        fk.is_object(),
        "_field_kinds must be an object: got {fk}",
    );
    for kind in REQUIRED_KINDS {
        let entry = &fk[kind];
        assert!(
            entry.is_array(),
            "_field_kinds.{kind} must be an array of patterns: got {entry}",
        );
        assert!(
            !entry.as_array().unwrap().is_empty(),
            "_field_kinds.{kind} must declare at least one pattern",
        );
    }
    let address_arr = fk["address"]
        .as_array()
        .expect("address kind is array (checked above)");
    let address_strs: Vec<&str> = address_arr
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        address_strs.contains(&"address"),
        "_field_kinds.address must include 'address': got {address_strs:?}",
    );
    assert!(
        address_strs.contains(&"to"),
        "_field_kinds.address must include 'to': got {address_strs:?}",
    );
}

#[tokio::test]
async fn policy_current_emits_field_kinds_when_unconfigured() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = read_resource(&mut proc, 2, "policy://current").await?;
    let body = extract_resource_json(&r);

    // Existing fail-closed shape preserved.
    assert_eq!(body["loaded"], false);
    assert_eq!(body["confidence"], "missing");

    // v1.13 Track P2: hints declared.
    assert_field_kinds_shape(&body);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_current_emits_field_kinds_when_loaded() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let _ = call_tool(
        &mut proc,
        10,
        "policy_set",
        json!({ "policy": minimal_policy(), "rationale": "v1.13 field-kinds test" }),
    )
    .await?;

    let r = read_resource(&mut proc, 11, "policy://current").await?;
    let body = extract_resource_json(&r);

    // Existing loaded shape preserved.
    assert_eq!(body["loaded"], true);
    assert!(
        body["revision_id"].is_string(),
        "revision_id should still be a string: {body}",
    );
    assert!(
        body["policy"].is_object(),
        "policy body should still be present: {body}",
    );

    // v1.13 Track P2: hints declared on the loaded shape too.
    assert_field_kinds_shape(&body);

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_history_emits_field_kinds() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Install one revision so history is non-empty (also exercises the
    // happy path; the field_kinds emit must work regardless of count).
    let _ = call_tool(
        &mut proc,
        20,
        "policy_set",
        json!({ "policy": minimal_policy(), "rationale": "history seed" }),
    )
    .await?;

    let r = read_resource(&mut proc, 21, "policy://history").await?;
    let body = extract_resource_json(&r);

    // Existing shape preserved.
    assert!(
        body["revisions"].is_array(),
        "history must keep `revisions` array: {body}",
    );
    assert!(
        body["count"].as_u64().is_some(),
        "history must keep `count` number: {body}",
    );

    // v1.13 Track P2: hints declared.
    assert_field_kinds_shape(&body);

    proc.child.kill().await?;
    Ok(())
}
