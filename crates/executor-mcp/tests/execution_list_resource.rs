//! v1.4 Track C — `execution://list` resource integration test.
//!
//! Drives the stdio MCP binary, registers two strategies, runs each manually
//! to produce real `runs` rows, then reads `execution://list?...` with each
//! filter combination and asserts the response shape:
//! ```json
//! { "runs": [...], "count": N, "filters_applied": { ... } }
//! ```
//!
//! Notes:
//! - We use the persistent-file fixture (tempfile-backed config) so the
//!   server actually has rows to list — `:memory:` works but we keep the
//!   pattern aligned with other resource tests that seed state.
//! - Real strategy execution requires `[evm]` / `[policy]` config in a
//!   non-trivial way. To stay hermetic, we register strategies and rely on
//!   the resource being readable even when zero runs match — the
//!   `empty_filter_set_returns_empty_runs_array` test covers the
//!   zero-row contract; a richer end-to-end is covered by the
//!   executor-state unit tests.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server, spawn_server_with_state};

/// Parse the `resources/read` response body as JSON.
fn read_resource_body(r: &Value) -> Value {
    let text = r["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read missing contents[0].text: {r}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("resource body is not JSON: {e} — text={text}"))
}

#[tokio::test]
async fn execution_list_template_registered() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/templates/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let templates = r["result"]["resourceTemplates"]
        .as_array()
        .expect("resourceTemplates array");
    let uris: Vec<&str> = templates
        .iter()
        .map(|t| t["uriTemplate"].as_str().unwrap_or_default())
        .collect();
    assert!(
        uris.contains(&"execution://list"),
        "execution://list template must be registered; got {uris:?}"
    );
    // Description must mention all four query parameters so agents can
    // discover the filter surface from the template alone.
    let exec_list_tpl = templates
        .iter()
        .find(|t| t["uriTemplate"].as_str() == Some("execution://list"))
        .expect("execution://list template");
    let desc = exec_list_tpl["description"]
        .as_str()
        .expect("description set");
    for required in ["strategy_id", "since", "status", "limit"] {
        assert!(
            desc.contains(required),
            "execution://list description must mention `{required}`; got {desc:?}"
        );
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn execution_list_empty_state_returns_empty_array() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "execution://list?limit=2" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "no error expected, got {r}");

    let body = read_resource_body(&r);
    let runs = body["runs"].as_array().expect("runs array");
    assert!(runs.is_empty(), "empty state must produce empty runs[], got {runs:?}");
    assert_eq!(body["count"], 0, "count must be 0 on empty state");

    let filters = body["filters_applied"]
        .as_object()
        .expect("filters_applied object");
    assert_eq!(filters.get("limit"), Some(&json!(2)));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn execution_list_with_runs_returns_summaries() -> Result<()> {
    // Spawn against a real temp-file DB so we can pre-seed runs through the
    // same SQLite the server reads from. (`:memory:` is per-connection.)
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    // Drop the auto-delete guard so the server can open the path.
    let _ = tmp.into_temp_path().keep()?;

    // Seed two strategies + four runs out-of-band, then start the server
    // pointed at this DB.
    {
        use executor_core::schema::execution::{JournalActionOutcome, RunStatus};
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let sid_a = match store
            .register_strategy("alpha", "// alpha", None, None)?
        {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };
        let sid_b = match store
            .register_strategy("beta", "// beta", None, None)?
        {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };

        let a1 = store.__test_insert_run_with_time(
            &sid_a,
            RunStatus::Queued,
            "2026-04-27T01:00:00Z",
        )?;
        #[allow(deprecated)]
        store.update_run_status(&a1, RunStatus::Succeeded)?;
        store.record_action_outcome(&a1, JournalActionOutcome::Noop, "{}")?;

        let a2 = store.__test_insert_run_with_time(
            &sid_a,
            RunStatus::Queued,
            "2026-04-27T02:00:00Z",
        )?;
        #[allow(deprecated)]
        store.update_run_status(&a2, RunStatus::Failed)?;

        let _b1 = store.__test_insert_run_with_time(
            &sid_b,
            RunStatus::Queued,
            "2026-04-27T03:00:00Z",
        )?;
        let _a3 = store.__test_insert_run_with_time(
            &sid_a,
            RunStatus::Queued,
            "2026-04-27T04:00:00Z",
        )?;

        // Drop store ⇒ release SQLite file lock before the server opens it.
        drop(store);
    }

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    // 1. strategy_id=<alpha> + limit=2 → newest two alpha runs.
    // First we need the alpha strategy id. v1.4 Track B: strategy_list tool
    // dropped; resolve via the `strategy://list` resource.
    let r = common::read_resource(&mut proc, 2, "strategy://list").await?;
    let body = common::extract_resource_json(&r);
    let strategies = body["strategies"].as_array().expect("strategies array");
    let sid_a = strategies
        .iter()
        .find(|s| s["name"] == "alpha")
        .expect("alpha strategy seeded")["id"]
        .as_str()
        .expect("alpha id")
        .to_string();

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "resources/read",
            "params": { "uri": format!("execution://list?strategy_id={sid_a}&limit=2") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);
    let runs = body["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 2, "limit=2 + 3 alpha runs ⇒ 2 returned");
    assert_eq!(body["count"], 2);

    // Newest-first: 04:00:00 then 02:00:00.
    let first_ts = runs[0]["started_at"].as_str().unwrap();
    let second_ts = runs[1]["started_at"].as_str().unwrap();
    assert!(
        first_ts > second_ts,
        "newest-first ordering: {first_ts} should be > {second_ts}"
    );
    // Every returned run must carry the requested strategy_id.
    for r in runs {
        assert_eq!(r["strategy_id"], json!(sid_a));
        // Required summary fields.
        for required in ["run_id", "strategy_id", "status", "started_at", "action_count"] {
            assert!(
                r.get(required).is_some(),
                "summary row must include `{required}`; got {r:?}"
            );
        }
    }

    let filters = body["filters_applied"].as_object().expect("filters_applied");
    assert_eq!(filters.get("strategy_id"), Some(&json!(sid_a)));
    assert_eq!(filters.get("limit"), Some(&json!(2)));

    // 2. status=failed → exactly one row (a2).
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "resources/read",
            "params": { "uri": format!("execution://list?strategy_id={sid_a}&status=failed") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);
    let runs = body["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 1, "exactly one failed alpha run");
    assert_eq!(runs[0]["status"], json!("failed"));

    // 3. status=noop → the noop-outcome run.
    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 5, "method": "resources/read",
            "params": { "uri": format!("execution://list?strategy_id={sid_a}&status=noop") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);
    let runs = body["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 1, "exactly one noop alpha run");
    assert_eq!(runs[0]["action_count"], json!(1));
    // The noop row was the 01:00:00 run; verify by timestamp.
    assert_eq!(runs[0]["started_at"], json!("2026-04-27T01:00:00Z"));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn execution_list_rejects_malformed_since() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "execution://list?since=not-a-date" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(
        r.get("error").is_some(),
        "malformed `since` must surface as an error, NOT a zero-row response"
    );
    assert_eq!(
        r["error"]["code"], -32602,
        "malformed `since` must use JSON-RPC -32602 invalid_params"
    );
    let detail = r["error"]["data"]["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("RFC3339") || detail.contains("ISO8601"),
        "error detail must mention the expected timestamp format: {detail:?}"
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn execution_list_rejects_unknown_status() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "execution://list?status=running" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(
        r.get("error").is_some(),
        "unsupported `status` label must error, not silently fall through"
    );
    assert_eq!(r["error"]["code"], -32602);

    proc.child.kill().await?;
    Ok(())
}
