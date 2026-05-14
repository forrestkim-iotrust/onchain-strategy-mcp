//! v1.11 Track E3 — integration tests for the `tune_thresholds` prompt.
//!
//! Each test registers a strategy with a particular source shape, then calls
//! `prompts/get` with `name: tune_thresholds` and asserts the prompt body
//! reflects the expected static-parse output. No real run history is needed
//! for the heuristic-edge tests — the prompt gracefully handles an empty
//! `execution://list` window.

mod common;

use anyhow::Result;
use common::{call_tool, extract_json_result, initialize, recv, send, spawn_server_with_state};
use serde_json::{Value, json};

async fn register(proc: &mut common::ServerProc, name: &str, source: &str) -> Result<String> {
    let r = call_tool(
        proc,
        100,
        "strategy_register",
        json!({ "name": name, "source": source }),
    )
    .await?;
    let body = extract_json_result(&r);
    Ok(body["strategy_id"].as_str().unwrap().to_string())
}

async fn get_tune_body(proc: &mut common::ServerProc, args: Value) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0", "id": 200, "method": "prompts/get",
            "params": { "name": "tune_thresholds", "arguments": args }
        }),
    )
    .await?;
    recv(proc).await
}

fn body_text(r: &Value) -> String {
    r["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

// 1. Strategy with multiple numeric thresholds → table has rows for each candidate.
#[tokio::test]
async fn tune_thresholds_extracts_multiple_candidates() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Three distinct thresholds with threshold-y vocab on the line.
    let source = r#"(ctx) => {
        const apy = 5;
        if (apy > 0.05) return "noop";
        if (balance >= 100000) return "noop";
        if (slippage < 0.03) return "noop";
        return "noop";
    }"#;
    let id = register(&mut proc, "multi-threshold", source).await?;

    let r = get_tune_body(&mut proc, json!({ "strategy_id": id })).await?;
    let text = body_text(&r);

    assert!(
        text.contains("Threshold tuning report"),
        "missing report header: {text}"
    );
    // Each numeric literal should surface in the table.
    assert!(text.contains("0.05"), "missing 0.05 candidate: {text}");
    assert!(text.contains("100000"), "missing 100000 candidate: {text}");
    assert!(text.contains("0.03"), "missing 0.03 candidate: {text}");
    assert!(
        text.contains("| Current value |"),
        "missing markdown table header: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// 2. Strategy with zero numeric literals → graceful missing-data response.
#[tokio::test]
async fn tune_thresholds_handles_no_numeric_literals() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Pure-read snapshot, no thresholds.
    let source = r#"(ctx) => "noop""#;
    let id = register(&mut proc, "no-thresholds", source).await?;

    let r = get_tune_body(&mut proc, json!({ "strategy_id": id })).await?;
    let text = body_text(&r);

    assert!(
        text.contains("confidence: missing"),
        "missing graceful confidence marker: {text}"
    );
    assert!(
        text.contains("No numeric-literal thresholds"),
        "missing graceful-empty wording: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// 3. Strategy with only comments / empty body → graceful missing-data response.
#[tokio::test]
async fn tune_thresholds_handles_comments_only_strategy() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Source is just comments (and a noop expression).
    let source = "// nothing here\n/* and here */\n\"noop\"";
    let id = register(&mut proc, "comments-only", source).await?;

    let r = get_tune_body(&mut proc, json!({ "strategy_id": id })).await?;
    let text = body_text(&r);

    assert!(
        text.contains("confidence: missing"),
        "missing graceful confidence marker: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// 4. Strategy with literals only in comments (should be skipped) → no false positives.
#[tokio::test]
async fn tune_thresholds_skips_numbers_in_comments() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // The numbers `0.99`, `42`, `7777` only appear inside comments; the
    // executable body is pure-noop.
    let source = r#"// threshold > 0.99 someday
/* magic 42 number here, and 7777 too */
(ctx) => "noop""#;
    let id = register(&mut proc, "commented-numbers", source).await?;

    let r = get_tune_body(&mut proc, json!({ "strategy_id": id })).await?;
    let text = body_text(&r);

    // Must take the graceful path — none of the commented numbers count.
    assert!(
        text.contains("confidence: missing"),
        "must report missing when only commented-out numbers: {text}"
    );
    // Spot-check: none of the commented numbers appear in a table row.
    assert!(
        !text.contains("| `0.99` "),
        "commented-out 0.99 leaked into table: {text}"
    );
    assert!(
        !text.contains("| `42` "),
        "commented-out 42 leaked into table: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// 5. Strategy with hex addresses + selectors (should be skipped) → no false positives.
#[tokio::test]
async fn tune_thresholds_skips_addresses_and_selectors() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Address (0x + 40 hex) and selector (0x + 8 hex) appear on a comparison
    // line; neither should surface as a threshold. The `0.05` IS a real
    // candidate and should show up.
    let source = r#"(ctx) => {
        const target = "0xa238dd80c259a72e81d7e4664a9801593f98d1c5";
        const sel = 0xa9059cbb;
        if (ctx.x > 0.05 && target != "0x0000000000000000000000000000000000000000") return "noop";
        if (sel == 0xa9059cbb) return "noop";
        return "noop";
    }"#;
    let id = register(&mut proc, "hex-skipping", source).await?;

    let r = get_tune_body(&mut proc, json!({ "strategy_id": id })).await?;
    let text = body_text(&r);

    // Real threshold present.
    assert!(text.contains("0.05"), "missing real candidate 0.05: {text}");
    // Address fragments should NOT appear as candidates. We check the
    // address-prefix isn't in a table row.
    assert!(
        !text.contains("| `0xa238"),
        "address fragment leaked into candidates: {text}"
    );
    assert!(
        !text.contains("| `0xa9059cbb` "),
        "selector leaked into candidates: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// 6. Malformed strategy_id → invalid_params with hint.
#[tokio::test]
async fn tune_thresholds_rejects_malformed_strategy_id() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let r = get_tune_body(&mut proc, json!({ "strategy_id": "not-hex" })).await?;

    assert_eq!(
        r["error"]["code"], -32602,
        "expected invalid_params: {r}"
    );
    let hint = r["error"]["data"]["hint"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        hint.contains("strategy://list") || r["error"]["data"]["detail"].as_str().unwrap_or("").contains("strategy://list"),
        "hint should point at strategy://list, got hint={hint} data={}",
        r["error"]["data"]
    );

    proc.child.kill().await?;
    Ok(())
}

// 7. lookback_runs > 200 → clamped to 200 (no error, prompt still returns).
#[tokio::test]
async fn tune_thresholds_clamps_lookback_runs_to_max_200() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Minimal real candidate so we hit the table-rendering path (not the
    // graceful-empty path).
    let source = r#"(ctx) => { if (ctx.x > 0.5) return "noop"; return "noop"; }"#;
    let id = register(&mut proc, "lookback-clamp", source).await?;

    let r = get_tune_body(
        &mut proc,
        json!({ "strategy_id": id, "lookback_runs": 9999 }),
    )
    .await?;
    let text = body_text(&r);

    assert!(
        r["error"].is_null(),
        "must not error on oversized lookback: {r}"
    );
    // No runs registered → "0 found"; the report still renders.
    assert!(
        text.contains("Threshold tuning report"),
        "missing report header: {text}"
    );
    // The candidate must be present.
    assert!(text.contains("0.5"), "missing candidate row: {text}");
    // Sanity-check: 9999 shouldn't appear in the report as a literal lookback
    // window count — we clamp to 200, so the requested-count phrasing should
    // show 200 (or actual=0 when no runs found).
    assert!(
        !text.contains("9999"),
        "lookback_runs was not clamped: {text}"
    );

    proc.child.kill().await?;
    Ok(())
}

// 8. Unknown but well-formed strategy_id → invalid_params with strategy://list hint.
#[tokio::test]
async fn tune_thresholds_unknown_id_points_at_strategy_list() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // 64 hex chars but no such strategy registered.
    let phantom = "0".repeat(64);
    let r = get_tune_body(&mut proc, json!({ "strategy_id": phantom })).await?;

    assert_eq!(
        r["error"]["code"], -32602,
        "unknown id should be invalid_params: {r}"
    );
    let detail = r["error"]["data"]["detail"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        detail.contains("strategy://list"),
        "detail should point at strategy://list, got: {detail}"
    );

    proc.child.kill().await?;
    Ok(())
}
