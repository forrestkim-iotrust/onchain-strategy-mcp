//! v1.11 Track B — `runtime://` resource plane integration tests.
//!
//! Drives the stdio MCP binary and exercises the three runtime URIs:
//!
//! - `runtime://status`  — operational state snapshot
//! - `runtime://signals` — per-watcher signal surface
//! - `runtime://recent`  — newest 50 runs across all strategies
//!
//! The default fixture spawns the binary with no `[evm]` section, so the
//! provider is built against the default `EvmConfig` (anvil at
//! `127.0.0.1:8545`). The RPC probe either:
//!
//! - succeeds (CI runs anvil; happy path → `confidence: "full"`,
//!   `rpc.ok: true`); or
//! - fails / times out (no devnet; degraded path → `confidence: "partial"`,
//!   `rpc.ok: false`, non-empty `reason`).
//!
//! Tests assert that the shape and per-field invariants hold in both cases
//! so the suite stays hermetic. The dedicated `rpc_timeout` test pins the
//! degraded path explicitly by routing through a deliberately-unreachable
//! RPC URL.

mod common;

use anyhow::Result;
use executor_core::schema::execution::RunStatus;
use executor_state::{RegisterOutcome, StateStore};
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server_with_config_text, spawn_server_with_state};

fn read_resource_body(r: &Value) -> Value {
    let text = r["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read missing contents[0].text: {r}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("resource body is not JSON: {e} — text={text}"))
}

// ─────────────────────────── runtime://status ───────────────────────────

#[tokio::test]
async fn runtime_status_template_registered() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // resources/list — singleton entry for the runtime plane.
    send(
        &mut proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let list = r["result"]["resources"].as_array().expect("resources array");
    let uris: Vec<&str> = list
        .iter()
        .map(|t| t["uri"].as_str().unwrap_or_default())
        .collect();
    assert!(
        uris.contains(&"runtime://status"),
        "runtime://status must appear in resources/list; got {uris:?}"
    );

    // resources/templates/list — all three runtime templates.
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
    for required in ["runtime://status", "runtime://signals", "runtime://recent"] {
        assert!(
            template_uris.contains(&required),
            "v1.11 Track B template {required} missing; got {template_uris:?}"
        );
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn runtime_status_empty_state_legitimately_full_with_no_rpc() -> Result<()> {
    // Empty DB + no live RPC (default config points at anvil:8545 which isn't
    // running). The probe will time out within 500ms → confidence "partial".
    // active_triggers must be 0, last_24h.runs must be 0, schema_version 1.11.
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "runtime://status" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");

    let body = read_resource_body(&r);
    let data = &body["data"];
    assert_eq!(data["schema_version"], json!("1.11"));
    assert_eq!(data["active_triggers"], json!(0));
    assert_eq!(data["last_24h"]["runs"], json!(0));
    assert_eq!(data["last_24h"]["succeeded"], json!(0));
    assert_eq!(data["last_24h"]["failed"], json!(0));
    assert_eq!(data["last_24h"]["noop"], json!(0));

    // Watchers default to "not running" on an empty trigger set.
    assert_eq!(data["watchers"]["mempool"]["running"], json!(false));
    assert_eq!(data["watchers"]["log"]["running"], json!(false));
    assert_eq!(data["watchers"]["mempool"]["last_signal_ts"], Value::Null);
    assert_eq!(data["watchers"]["log"]["last_signal_ts"], Value::Null);

    // Burner is the EvmConfig default (anvil account 0) — non-empty string
    // starting with `0x`.
    let burner = data["burner"].as_str().expect("burner is a string");
    assert!(burner.starts_with("0x"), "burner must be 0x-prefixed: {burner:?}");
    assert_eq!(burner.len(), 42, "burner must be a 20-byte hex address");

    // RPC info is always present. The default rpc_url is loopback anvil →
    // host_str = "127.0.0.1" with port "8545".
    let rpc = &data["rpc"];
    let url_masked = rpc["url_masked"].as_str().expect("url_masked is a string");
    assert!(
        !url_masked.contains("http") && !url_masked.contains("/"),
        "url_masked must NOT include scheme or path; got {url_masked:?}"
    );

    // Signer probe is structural — burner parseable ⇒ ok=true.
    assert_eq!(data["signer"]["ok"], json!(true));

    // Confidence accounting: `full` requires RPC ok AND no other degraded
    // probe; `partial` / `missing` carry a non-empty `reason`. Both
    // outcomes are legitimate in an empty-state scenario.
    let confidence = body["confidence"].as_str().expect("confidence is a string");
    assert!(
        matches!(confidence, "full" | "partial" | "missing"),
        "confidence must be a known label; got {confidence:?}"
    );
    if confidence != "full" {
        assert!(
            body["reason"].is_string(),
            "degraded confidence must carry a non-empty `reason`; got {:?}",
            body["reason"]
        );
    } else {
        // Happy path: RPC ok ⇒ rpc.ok = true and a last_ok_ts timestamp.
        assert_eq!(data["rpc"]["ok"], json!(true));
        assert!(data["rpc"]["last_ok_ts"].is_string());
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn runtime_status_with_triggers_and_runs() -> Result<()> {
    // Persistent DB so we can seed state out-of-band, then point the server
    // at it. Mirrors execution_list_resource.rs::execution_list_with_runs.
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let strategy_id: String;
    {
        let mut store = StateStore::open(&db_path)?;
        strategy_id = match store.register_strategy("alpha", "// alpha", None, None)? {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };

        // Two runs within the 24h window: one succeeded, one failed.
        let now = chrono::Utc::now();
        let recent_ts = (now - chrono::Duration::minutes(30))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let r1 = store.__test_insert_run_with_time(
            &strategy_id,
            RunStatus::Queued,
            &recent_ts,
        )?;
        #[allow(deprecated)]
        store.update_run_status(&r1, RunStatus::Succeeded)?;
        let r2 = store.__test_insert_run_with_time(
            &strategy_id,
            RunStatus::Queued,
            &recent_ts,
        )?;
        #[allow(deprecated)]
        store.update_run_status(&r2, RunStatus::Failed)?;
        drop(store);
    }

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "runtime://status" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");

    let body = read_resource_body(&r);
    let data = &body["data"];
    assert_eq!(data["last_24h"]["runs"], json!(2));
    assert_eq!(data["last_24h"]["succeeded"], json!(1));
    assert_eq!(data["last_24h"]["failed"], json!(1));

    // No triggers were registered — `active_triggers` must remain 0 even
    // when runs exist (this is the explicit guard the v1.11 spec calls out:
    // the two counters move independently).
    assert_eq!(data["active_triggers"], json!(0));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn runtime_status_rpc_timeout_downgrades_confidence() -> Result<()> {
    // Pin the degraded path by routing through a deliberately unreachable
    // RPC URL. Port 1 on loopback is reserved and the kernel returns
    // ECONNREFUSED immediately — fetch_chain_id surfaces an Err inside
    // our 500ms wrap, so this drives the partial-confidence branch
    // deterministically regardless of whether anvil is also running on 8545.
    let config = "\
        [state]\n\
        path = \":memory:\"\n\
        [evm]\n\
        rpc_url = \"http://127.0.0.1:1\"\n\
    ";
    let mut proc = spawn_server_with_config_text(config).await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "runtime://status" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);

    assert_eq!(body["data"]["rpc"]["ok"], json!(false));
    assert_eq!(body["data"]["rpc"]["last_ok_ts"], Value::Null);
    let confidence = body["confidence"].as_str().expect("confidence string");
    assert_ne!(
        confidence, "full",
        "RPC unreachable must NOT report full confidence"
    );
    let reason = body["reason"].as_str().expect("reason string on degraded");
    assert!(
        !reason.is_empty(),
        "degraded path must surface a non-empty reason"
    );

    proc.child.kill().await?;
    Ok(())
}

// ─────────────────────────── runtime://signals ───────────────────────────

#[tokio::test]
async fn runtime_signals_empty_state() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "runtime://signals" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);
    assert!(body["data"]["mempool"].is_object());
    assert!(body["data"]["log"].is_object());
    // queue_depth is unobservable from the resource layer in v1.11 → null.
    assert_eq!(body["data"]["mempool"]["queue_depth"], Value::Null);
    assert_eq!(body["data"]["log"]["queue_depth"], Value::Null);
    // No mempool triggers ⇒ watched_addresses is an empty array, not null.
    let watched = body["data"]["mempool"]["watched_addresses"]
        .as_array()
        .expect("watched_addresses array");
    assert!(watched.is_empty());
    // The contract pins confidence to "partial" until queue_depth is wired.
    assert_eq!(body["confidence"], json!("partial"));
    assert!(body["reason"].is_string());

    proc.child.kill().await?;
    Ok(())
}

// ─────────────────────────── runtime://recent ────────────────────────────

#[tokio::test]
async fn runtime_recent_empty_state() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "runtime://recent" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);
    assert!(body["runs"].is_array());
    assert_eq!(body["count"], json!(0));
    assert!(body["runs"].as_array().unwrap().is_empty());

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn runtime_recent_returns_newest_first_with_summary_shape() -> Result<()> {
    // Seed three runs across two strategies; expect newest-first ordering
    // and the execution://list summary shape on every row.
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    {
        let mut store = StateStore::open(&db_path)?;
        let sid_a = match store.register_strategy("alpha", "// alpha", None, None)? {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };
        let sid_b = match store.register_strategy("beta", "// beta", None, None)? {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };
        let _ = store.__test_insert_run_with_time(
            &sid_a,
            RunStatus::Queued,
            "2026-04-27T01:00:00Z",
        )?;
        let _ = store.__test_insert_run_with_time(
            &sid_b,
            RunStatus::Queued,
            "2026-04-27T02:00:00Z",
        )?;
        let _ = store.__test_insert_run_with_time(
            &sid_a,
            RunStatus::Queued,
            "2026-04-27T03:00:00Z",
        )?;
        drop(store);
    }

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": "runtime://recent" }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    let body = read_resource_body(&r);
    let runs = body["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 3);
    assert_eq!(body["count"], json!(3));

    // Newest-first.
    let first = runs[0]["started_at"].as_str().unwrap();
    let last = runs[2]["started_at"].as_str().unwrap();
    assert!(first > last, "expected newest first: {first} > {last}");

    // Summary shape matches execution://list summary rows.
    for r in runs {
        for required in [
            "run_id",
            "strategy_id",
            "status",
            "started_at",
            "action_count",
        ] {
            assert!(
                r.get(required).is_some(),
                "summary row must include `{required}`; got {r:?}"
            );
        }
    }

    proc.child.kill().await?;
    Ok(())
}
