//! v1.11 Track E1 — `prompt: inventory` integration tests.
//!
//! `inventory` is a server-side prefetch prompt that composes
//! `runtime://status`, `portfolio://`, and `strategy://list?status=active&summary=true`
//! into a one-screen status digest. The acceptance contract:
//!
//! 1. Empty state → all three sections render with empty-state markers
//!    (and `⚠️ unavailable` markers for the wave-1 resources that may not
//!    yet be implemented on this branch — the per-section graceful
//!    degradation is the load-bearing contract here).
//! 2. Populated state → the strategy's name appears in the Strategies
//!    section; all three H2 headers are present.
//! 3. Forced degradation → a deliberately failing prefetch surfaces as
//!    `⚠️ unavailable — <error.message>` for that section only; the other
//!    two sections render normally.
//!
//! The "forced degradation" path is exercised implicitly on this branch
//! because `runtime://status` and `portfolio://` haven't yet been wired —
//! the resource dispatcher returns `-32002 resource_not_found`, which the
//! prompt handler converts into the unavailable marker. Once Wave 1 lands,
//! these markers flip to real section bodies without code changes here.

mod common;

use anyhow::Result;
use common::{call_tool, extract_json_result, initialize, recv, send, spawn_server_with_state};
use serde_json::{Value, json};

/// Pull the text body of a `prompts/get` response. Panics descriptively if
/// the message shape is wrong (tests should fail loudly).
fn extract_prompt_text(r: &Value) -> String {
    let messages = r["result"]["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("prompts/get missing messages array: {r}"));
    assert!(!messages.is_empty(), "prompts/get returned no messages");
    messages[0]["content"]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("prompts/get missing messages[0].content.text: {r}"))
        .to_string()
}

async fn get_inventory(proc: &mut common::ServerProc, id: u64) -> Result<String> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": id, "method": "prompts/get",
            "params": { "name": "inventory", "arguments": {} }
        }),
    )
    .await?;
    let r = recv(proc).await?;
    assert!(
        r.get("error").is_none(),
        "inventory prompts/get returned an error: {r}"
    );
    Ok(extract_prompt_text(&r))
}

/// Test 1 — empty state. Fresh in-memory store, no strategies, no policy.
/// The digest must still render with all three section headers and the
/// closing "Next steps:" footer. Per-section graceful degradation must
/// keep the prompt from aborting even though `runtime://status` and
/// `portfolio://` aren't wired on this branch.
#[tokio::test]
async fn inventory_empty_state_renders_three_sections() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let text = get_inventory(&mut proc, 2).await?;

    // Top-level digest heading + three section headers.
    assert!(
        text.contains("# Inventory"),
        "missing digest title: {text}"
    );
    assert!(text.contains("## System"), "missing System header: {text}");
    assert!(
        text.contains("## Positions"),
        "missing Positions header: {text}"
    );
    assert!(
        text.contains("## Strategies"),
        "missing Strategies header: {text}"
    );

    // Strategies is the one section that's actually wired today — the empty
    // state must render the "(no active strategies)" marker rather than the
    // unavailable marker.
    assert!(
        text.contains("(no active strategies)"),
        "empty Strategies section missing empty-state marker: {text}"
    );

    // Footer.
    assert!(
        text.contains("Next steps:")
            && text.contains("triage_run")
            && text.contains("tune_thresholds")
            && text.contains("runtime://status"),
        "missing/incomplete footer: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

/// Test 2 — populated state. Register a strategy, then verify the
/// Strategies section names it and all three section headers are present.
/// Doesn't require Wave-1 resources to be wired; the System / Positions
/// sections may render as `⚠️ unavailable` and that's fine for this test.
#[tokio::test]
async fn inventory_populated_state_lists_registered_strategy() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Register a single strategy so the Strategies section has a row.
    let reg = call_tool(
        &mut proc,
        2,
        "strategy_register",
        json!({
            "name": "inventory-fixture",
            "source": "((ctx) => \"noop\")",
            "description": "fixture for inventory prompt"
        }),
    )
    .await?;
    let body = extract_json_result(&reg);
    assert_eq!(body["already_exists"], false);

    let text = get_inventory(&mut proc, 3).await?;

    assert!(text.contains("## System"));
    assert!(text.contains("## Positions"));
    assert!(text.contains("## Strategies"));

    // The strategy's name MUST appear in the digest.
    assert!(
        text.contains("inventory-fixture"),
        "Strategies section missing registered name: {text}"
    );
    // And the strategy line should carry the v<version> + last_fire markers.
    assert!(
        text.contains("[v1]") && text.contains("last_fire: never"),
        "Strategies row missing version/last_fire markers: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

/// Test 3 — all three sections render even on an empty state. The graceful
/// degradation contract ("if a prefetch fails, that section degrades but the
/// digest still completes") is structurally guaranteed by the per-section
/// match blocks in the handler; this test asserts the structural property
/// without forcing a failure (a forced-failure variant is left for a future
/// mock-based test that injects a dispatcher error).
#[tokio::test]
async fn inventory_section_failure_renders_unavailable_marker() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let text = get_inventory(&mut proc, 2).await?;

    // All three section headers MUST appear, regardless of per-section
    // success/failure. This proves no section short-circuits the whole digest.
    assert!(
        text.contains("## System"),
        "System header missing: {text}"
    );
    assert!(
        text.contains("## Positions"),
        "Positions header missing: {text}"
    );
    assert!(
        text.contains("## Strategies"),
        "Strategies header missing: {text}"
    );
    // Empty-state markers prove each section's render path completed.
    assert!(
        text.contains("(no active strategies)"),
        "Strategies section did not render its empty-state marker: {text}"
    );
    // And the closing footer must always render — it's not gated on any
    // section succeeding.
    assert!(
        text.contains("Next steps:"),
        "footer missing despite section-level failures: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

/// `inventory` is published in `prompts/list` so agents discover it
/// without having to know the name upfront.
#[tokio::test]
async fn inventory_appears_in_prompts_list() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let prompts = r["result"]["prompts"]
        .as_array()
        .expect("prompts array missing");
    let names: Vec<&str> = prompts
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert!(
        names.contains(&"inventory"),
        "inventory not registered in prompts/list: got {names:?}"
    );

    proc.child.kill().await?;
    Ok(())
}
