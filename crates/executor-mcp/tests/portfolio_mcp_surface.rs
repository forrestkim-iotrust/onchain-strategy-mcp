//! v1.11 Track C — MCP `portfolio://` resource byte-identity test.
//!
//! Verifies that the MCP `resources/read` body for `portfolio://` and the
//! HTTP `/api/portfolio` response carry the same underlying aggregation.
//! The HTTP route is a façade: it dispatches through the same MCP handler
//! and peels off the v1.4 honesty envelope so the embedded web UI keeps
//! its flat wire shape. The MCP path keeps the envelope.
//!
//! The fixture mirrors `portfolio_aggregation::assets_aggregate_carries_attribution_from_strategy_view`:
//! one strategy with a deterministic `$assets` view, an unreachable RPC so
//! the balance walk short-circuits, and a non-volatile shape.
//!
//! Two assertions:
//! 1. The MCP body has the honesty envelope shape `{ data, confidence, ... }`.
//! 2. `mcp_body["data"]` equals the HTTP body (modulo the `refreshed_at`
//!    timestamp which is recomputed per call — we normalize both to the
//!    same placeholder before comparing).

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod common;

use std::{
    net::{Ipv4Addr, SocketAddr},
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::Result;
use common::{initialize, read_resource};
use executor_core::schema::execution::RunStatus;
use executor_state::{RegisterOutcome, StateStore};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

// ───── boot helpers (mirror the portfolio_aggregation harness) ─────

fn parse_ui_log_line(line: &str) -> Option<SocketAddr> {
    if !line.contains("🌐 UI: http://127.0.0.1:") {
        return None;
    }
    let needle = "🌐 UI: http://127.0.0.1:";
    let idx = line.find(needle)?;
    let tail = &line[idx + needle.len()..];
    let end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    let port: u16 = tail[..end].parse().ok()?;
    Some(SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), port))
}

async fn wait_for_ui_url(child: &mut Child, wait: Duration) -> Option<SocketAddr> {
    let stderr = child.stderr.take()?;
    let mut reader = BufReader::new(stderr).lines();
    let deadline = Instant::now() + wait;
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return None;
        }
        match tokio::time::timeout(timeout, reader.next_line()).await {
            Ok(Ok(Some(line))) => {
                if let Some(addr) = parse_ui_log_line(&line) {
                    let mut tail = reader;
                    tokio::spawn(async move {
                        while let Ok(Some(_)) = tail.next_line().await {}
                    });
                    return Some(addr);
                }
            }
            Ok(Ok(None)) => return None,
            Ok(Err(_)) | Err(_) => return None,
        }
    }
}

/// Owns the child process + an open stdio pair so the MCP transport stays
/// alive for the lifetime of the test. The struct kills the child on drop.
struct PortfolioProc {
    server: common::ServerProc,
    ui_addr: SocketAddr,
}

async fn spawn_with_db_and_unreachable_rpc(
    db_path: &std::path::Path,
) -> Result<PortfolioProc> {
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let tmp = tempfile::NamedTempFile::new()?;
    let cfg_path = tmp.path().to_path_buf();
    let cfg_text = format!(
        "[state]\npath = \"{}\"\n[evm]\nrpc_url = \"http://127.0.0.1:1\"\ncall_timeout_ms = 200\n",
        db_path.to_string_lossy().replace('\\', "\\\\")
    );
    std::fs::write(&cfg_path, cfg_text)?;
    let _ = tmp.into_temp_path().keep()?;

    let mut child = Command::new(bin)
        .env("RUST_LOG", "info")
        .env("EXECUTOR_CONFIG", cfg_path.as_os_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let ui_addr = wait_for_ui_url(&mut child, Duration::from_secs(5))
        .await
        .expect("UI url logged within 5s");

    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut server = common::ServerProc {
        child,
        stdin,
        stdout,
    };
    initialize(&mut server).await?;
    Ok(PortfolioProc { server, ui_addr })
}

async fn http_get_json(addr: SocketAddr, path: &str) -> Result<Value> {
    let url = format!("http://{addr}{path}");
    let body: Value = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(body)
}

fn seed_bundle_with_assets_view(
    store: &mut StateStore,
    name: &str,
    assets_literal: &str,
) -> Result<String> {
    let view_source = format!("(ctx, records) => ({{ \"$assets\": {assets_literal} }})");
    let outcome = store.register_strategy_bundle(
        name,
        "(ctx) => 'noop'",
        None,
        None,
        None,
        Some(&view_source),
        None,
        None,
    )?;
    let sid = match outcome {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
        RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    };
    // One queued run keeps the fingerprint deterministic (mirror the
    // portfolio_aggregation fixture).
    let _ = store.insert_run(&sid, RunStatus::Queued)?;
    Ok(sid)
}

/// Replace the volatile `refreshed_at` field with a sentinel so MCP and
/// HTTP responses, which are inevitably built fractions of a second apart,
/// can still be compared byte-for-byte on the load-bearing structure.
fn normalize_refreshed_at(v: &mut Value) {
    if let Some(obj) = v.as_object_mut() {
        if obj.contains_key("refreshed_at") {
            obj.insert(
                "refreshed_at".to_string(),
                Value::String("<normalized>".into()),
            );
        }
    }
}

// ───── tests ─────

#[tokio::test]
async fn portfolio_resource_envelope_matches_http_data_byte_identical() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // Same fixture shape as portfolio_aggregation::assets_aggregate_*.
    let sid = {
        let mut store = StateStore::open(&db_path)?;
        let assets = r#"[{
            "chain_id": 8453,
            "venue": "aave",
            "asset": "USDC",
            "amount": "1.0",
            "raw": "1000000",
            "decimals": 6,
            "address": null
        }]"#;
        seed_bundle_with_assets_view(&mut store, "mcp-surface-fixture", assets)?
    };

    let mut proc = spawn_with_db_and_unreachable_rpc(&db_path).await?;
    let ui_addr = proc.ui_addr;

    // Pull the HTTP response first so the MCP read happens against the same
    // strategy state (the fixture is static so the order doesn't matter
    // semantically — this just keeps the timeline obvious).
    let mut http_body = http_get_json(ui_addr, "/api/portfolio").await?;

    // Read `portfolio://` over the MCP `resources/read` RPC.
    let mcp_rpc = read_resource(&mut proc.server, 2, "portfolio://").await?;
    let mcp_body = common::extract_resource_json(&mcp_rpc);

    // ── 1. MCP response carries the v1.4 honesty envelope ──
    assert!(
        mcp_body.get("data").is_some(),
        "MCP body must wrap aggregation under `data`; got {mcp_body}"
    );
    let confidence = mcp_body
        .get("confidence")
        .and_then(Value::as_str)
        .expect("`confidence` field present");
    assert!(
        matches!(confidence, "full" | "partial" | "missing"),
        "confidence must be one of full|partial|missing; got {confidence}"
    );

    // ── 2. Byte-identity: HTTP body matches `mcp_body["data"]` ──
    let mut mcp_data = mcp_body
        .get("data")
        .cloned()
        .expect("MCP `data` field exists per assertion above");

    // Strategy `sid` must appear in both — sanity that the fixture was
    // observed by both transports.
    assert!(
        mcp_data["strategies"]
            .as_array()
            .map(|a| a.iter().any(|s| s["id"] == json!(sid)))
            .unwrap_or(false),
        "MCP body must list seeded strategy; got {mcp_data}"
    );
    assert!(
        http_body["strategies"]
            .as_array()
            .map(|a| a.iter().any(|s| s["id"] == json!(sid)))
            .unwrap_or(false),
        "HTTP body must list seeded strategy; got {http_body}"
    );

    // Normalize the only field that legitimately differs across the two
    // round-trips (server-side `chrono::Utc::now()` runs once per call).
    normalize_refreshed_at(&mut mcp_data);
    normalize_refreshed_at(&mut http_body);

    assert_eq!(
        mcp_data, http_body,
        "MCP `data` and HTTP body must be byte-identical (modulo `refreshed_at`)"
    );

    // Drop the child explicitly so the harness teardown is observable in
    // test output when something goes wrong.
    drop(proc);
    Ok(())
}
