//! v1.13 Track P3 — `policy://history?include_body=true` opt-in body inclusion.
//!
//! Covers:
//!   - Default `policy://history` (no flag) — entries have NO `body` field,
//!     payload shape is byte-identical to pre-v1.13.
//!   - `policy://history?include_body=true` — every entry has a parsed
//!     object `body`, never a stringified body.
//!   - `policy://history?include_body=true&limit=2` — query params compose
//!     correctly; only 2 entries returned and each still has body.
//!   - `policy://history?include_body=false` — explicit false matches the
//!     default no-body shape.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{
    call_tool, extract_resource_json, initialize, read_resource, spawn_server_with_state,
};

fn fresh_body(chain: u64, contract: &str) -> Value {
    json!({
        "chains": { "allow": [chain] },
        "contracts": { chain.to_string(): { "allow": [contract] } },
        "selectors": {},
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    })
}

/// Seed N distinct revisions, spacing writes by ~6ms so the
/// newest-first `ORDER BY set_at DESC, revision_id DESC` is robust against
/// same-millisecond ULID collisions.
async fn seed_revisions(proc: &mut common::ServerProc, n: u32) -> Result<()> {
    for i in 0..n {
        let body = fresh_body(31337, &format!("0x{:040x}", i + 1));
        let _ = call_tool(
            proc,
            100 + i as u64,
            "policy_set",
            json!({ "policy": body, "rationale": format!("rev {i}") }),
        )
        .await?;
        tokio::time::sleep(std::time::Duration::from_millis(6)).await;
    }
    Ok(())
}

#[tokio::test]
async fn default_history_has_no_body_field() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;
    seed_revisions(&mut proc, 3).await?;

    let r = read_resource(&mut proc, 200, "policy://history").await?;
    let body = extract_resource_json(&r);
    let revs = body["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 3, "all three seeded revisions returned");
    for (i, rev) in revs.iter().enumerate() {
        assert!(
            rev.get("body").is_none(),
            "default response must not include `body` field on entry {i}: {rev}",
        );
        assert!(
            rev.get("body_parse_error").is_none(),
            "default response must not include `body_parse_error` on entry {i}: {rev}",
        );
        // Pre-existing shape must still be present.
        assert!(rev["revision_id"].is_string());
        assert!(rev["set_at"].is_string());
        assert!(rev["is_active"].is_boolean());
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn explicit_include_body_false_matches_default() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;
    seed_revisions(&mut proc, 2).await?;

    let r_default = read_resource(&mut proc, 210, "policy://history").await?;
    let r_explicit =
        read_resource(&mut proc, 211, "policy://history?include_body=false").await?;
    let body_default = extract_resource_json(&r_default);
    let body_explicit = extract_resource_json(&r_explicit);

    assert_eq!(
        body_default, body_explicit,
        "explicit include_body=false must be byte-equivalent to the default",
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn include_body_true_emits_parsed_body_object_per_entry() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;
    seed_revisions(&mut proc, 3).await?;

    let r = read_resource(&mut proc, 220, "policy://history?include_body=true").await?;
    let body = extract_resource_json(&r);
    let revs = body["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 3, "all three seeded revisions returned");

    for (i, rev) in revs.iter().enumerate() {
        let body_field = rev
            .get("body")
            .unwrap_or_else(|| panic!("entry {i} missing `body` field: {rev}"));
        assert!(
            body_field.is_object(),
            "body must be a parsed object (not a stringified body) on entry {i}: {body_field}",
        );
        // It's the policy shape we wrote — `chains.allow` is present.
        assert_eq!(
            body_field["chains"]["allow"],
            json!([31337]),
            "body must round-trip the policy we wrote on entry {i}: {body_field}",
        );
        assert!(
            rev.get("body_parse_error").is_none(),
            "well-formed rows must not emit body_parse_error: {rev}",
        );
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn include_body_composes_with_limit() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;
    seed_revisions(&mut proc, 4).await?;

    let r =
        read_resource(&mut proc, 230, "policy://history?include_body=true&limit=2").await?;
    let body = extract_resource_json(&r);
    let revs = body["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 2, "limit=2 honoured alongside include_body=true: {body}");
    // Newest-first: first entry is the active row.
    assert_eq!(revs[0]["is_active"], json!(true));
    assert_eq!(revs[1]["is_active"], json!(false));
    for (i, rev) in revs.iter().enumerate() {
        assert!(
            rev["body"].is_object(),
            "entry {i} body must be parsed object even when limit applied: {rev}",
        );
    }
    // count must reflect emitted revisions, not the underlying total.
    assert_eq!(body["count"], json!(2));

    proc.child.kill().await?;
    Ok(())
}
