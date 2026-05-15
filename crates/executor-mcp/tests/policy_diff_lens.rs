//! v1.13 Track P4 — backend assertions backing the Policy-tab diff lens.
//!
//! The diff itself is computed client-side (see
//! `crates/executor-mcp/src/web_assets/app.js` — `diffJson` /
//! `renderObjectDiff`). This test pins down the wire shape the frontend
//! depends on:
//!
//!   - Seeding two distinct policy revisions via `policy_set`
//!     (initial + an update that adds / removes / changes entries).
//!   - `policy://history?include_body=true&limit=2` returns BOTH bodies
//!     as parsed objects in newest-first order.
//!   - Active row is index 0; previous (the diff baseline) is index 1.
//!   - The previous revision's body still reflects the v1 state (no
//!     "lossy snapshot" where the older body was overwritten).
//!   - `_field_kinds` is present on the response so the renderer can
//!     reuse P1's value-formatter dispatch in diff mode.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{
    call_tool, extract_resource_json, initialize, read_resource, spawn_server_with_state,
};

fn initial_body() -> Value {
    json!({
        "chains": { "allow": [31337] },
        "contracts": { "31337": { "allow": ["0x0000000000000000000000000000000000000001"] } },
        "selectors": {},
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    })
}

fn updated_body() -> Value {
    // vs initial:
    //   - chains.allow:    [31337]       → [31337, 8453]                      (added)
    //   - contracts.31337: {0x...01}      → {0x...02}                          (changed)
    //   - contracts.8453:  (absent)       → {0x...0a}                          (added subtree)
    //   - native_value:    {}             → { "31337": { "max_per_action":1 }} (added subtree)
    //   - raw_call:        allow_global:false → allow_global:true              (changed scalar)
    json!({
        "chains": { "allow": [31337, 8453] },
        "contracts": {
            "31337": { "allow": ["0x0000000000000000000000000000000000000002"] },
            "8453":  { "allow": ["0x000000000000000000000000000000000000000a"] },
        },
        "selectors": {},
        "native_value": { "31337": { "max_per_action": "1" } },
        "erc20_spend": {},
        "raw_call": { "allow_global": true, "allow": [] },
    })
}

#[tokio::test]
async fn diff_lens_history_returns_two_bodies_newest_first() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Revision 1: initial.
    let r1 = call_tool(
        &mut proc,
        300,
        "policy_set",
        json!({ "policy": initial_body(), "rationale": "v1 — initial" }),
    )
    .await?;
    assert!(r1.get("error").is_none(), "policy_set rev1 ok: {r1}");
    // Force distinct set_at so the descending ORDER BY is deterministic
    // even on machines where ULID collisions could occur in the same ms.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Revision 2: updated (the active row).
    let r2 = call_tool(
        &mut proc,
        301,
        "policy_set",
        json!({ "policy": updated_body(), "rationale": "v2 — diff baseline" }),
    )
    .await?;
    assert!(r2.get("error").is_none(), "policy_set rev2 ok: {r2}");

    // Fetch what the frontend fetches in diff mode.
    let resp = read_resource(
        &mut proc,
        310,
        "policy://history?include_body=true&limit=2",
    )
    .await?;
    let body = extract_resource_json(&resp);

    // Field-kinds envelope is present (P2 — diff renderer reuses dispatch).
    assert!(
        body.get("_field_kinds").is_some(),
        "policy://history must emit _field_kinds for the frontend renderer: {body}",
    );

    let revs = body["revisions"].as_array().expect("revisions array");
    assert_eq!(
        revs.len(),
        2,
        "diff lens needs exactly two revisions for limit=2: {body}",
    );

    // Newest-first: index 0 is the active row, index 1 is the diff
    // baseline. The frontend uses this exact ordering to wire prev → curr.
    assert_eq!(revs[0]["is_active"], json!(true), "newest is active");
    assert_eq!(revs[1]["is_active"], json!(false), "previous is inactive");

    let curr_body = &revs[0]["body"];
    let prev_body = &revs[1]["body"];
    assert!(curr_body.is_object(), "current body parsed object: {curr_body}");
    assert!(prev_body.is_object(), "previous body parsed object: {prev_body}");

    // Pin down the diff inputs — the prev row must still carry the v1
    // state (it would be a regression if `policy_set` clobbered older
    // bodies).
    assert_eq!(
        prev_body["chains"]["allow"],
        json!([31337]),
        "prev revision retains its original chains.allow: {prev_body}",
    );
    assert_eq!(
        curr_body["chains"]["allow"],
        json!([31337, 8453]),
        "curr revision carries the updated chains.allow: {curr_body}",
    );
    assert_eq!(
        prev_body["raw_call"]["allow_global"],
        json!(false),
        "prev raw_call.allow_global = false: {prev_body}",
    );
    assert_eq!(
        curr_body["raw_call"]["allow_global"],
        json!(true),
        "curr raw_call.allow_global = true (changed leaf): {curr_body}",
    );

    // No parse errors on the well-formed bodies.
    assert!(revs[0].get("body_parse_error").is_none());
    assert!(revs[1].get("body_parse_error").is_none());

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn diff_lens_with_only_one_revision_returns_single_entry() -> Result<()> {
    // Edge case the frontend handles: when there's only one revision in
    // history, the diff lens shows a "this is the initial revision"
    // banner. The backend's contract is simply that the response
    // contains < 2 entries; the JS doesn't try to compute a diff.
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let _ = call_tool(
        &mut proc,
        320,
        "policy_set",
        json!({ "policy": initial_body(), "rationale": "only revision" }),
    )
    .await?;

    let resp = read_resource(
        &mut proc,
        330,
        "policy://history?include_body=true&limit=2",
    )
    .await?;
    let body = extract_resource_json(&resp);
    let revs = body["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 1, "only one revision available: {body}");
    assert_eq!(revs[0]["is_active"], json!(true));

    proc.child.kill().await?;
    Ok(())
}
