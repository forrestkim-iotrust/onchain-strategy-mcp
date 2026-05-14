//! v1.6 Track 6C — `/api/portfolio` integration coverage.
//!
//! Each test pre-seeds a strategy (with or without `view_source`) by opening
//! the SQLite state store directly, then spawns the MCP binary against the
//! same file, drives `initialize`, and probes the loopback web UI.
//!
//! No live RPC: every test runs with `[evm].rpc_url` pointed at an
//! unreachable address so the balance walk short-circuits via
//! `_balance_walk_status: "no_provider"` (when `provider` is forcibly
//! disabled by an invalid scheme) or `"rpc_error"` (when `chain_id` can't
//! resolve). We never assert on a live `chain_id` value — the contract is
//! "no panic, no hang, structured status".
//!
//! v1.6 Track 6C plan §7 deliverable list:
//! - Idle balance with no provider → `idle_balances: []` + `no_provider`.
//! - `$assets` aggregation with attribution.
//! - Dedup conflict flagging.
//! - Per-strategy truncation at 50 entries.

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod common;

use std::{
    net::{Ipv4Addr, SocketAddr},
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::Result;
use common::initialize;
use executor_core::schema::execution::RunStatus;
use executor_state::{RegisterOutcome, StateStore};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

// ───── boot helpers (mirror web_api.rs but with a custom DB + EVM config) ─────

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

/// RAII wrapper that owns the spawned child process. Both fields are kept
/// alive so `kill_on_drop` fires on test exit and the child's stdin pipe
/// stays open for the lifetime of the test.
struct UiProc {
    #[allow(dead_code)]
    child: Child,
    #[allow(dead_code)]
    stdin: tokio::process::ChildStdin,
}

/// Spawn the binary with a custom DB path + an unreachable RPC. The UI
/// boots normally; the balance walk falls back to `no_provider` /
/// `rpc_error` instead of hanging.
async fn spawn_with_db_and_unreachable_rpc(db_path: &std::path::Path) -> Result<(UiProc, SocketAddr)> {
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let tmp = tempfile::NamedTempFile::new()?;
    let cfg_path = tmp.path().to_path_buf();
    // 127.0.0.1:1 (port 1 is privileged + nothing listens) → connection
    // refused fast. Combined with the 2s per-call timeout this keeps the
    // test latency bounded.
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

    let addr = wait_for_ui_url(&mut child, Duration::from_secs(5))
        .await
        .expect("UI url logged within 5s");

    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    // Drive a full initialize handshake so the MCP transport is alive.
    let mut server = common::ServerProc {
        child,
        stdin,
        stdout,
    };
    initialize(&mut server).await?;
    let common::ServerProc { child, stdin, .. } = server;
    Ok((UiProc { child, stdin }, addr))
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

// Tiny helper to seed a bundle strategy with a view that returns
// `{ $assets: [...] }` literally. The records spec is empty (no captures
// needed for these tests — `$assets` is hand-authored).
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
    // Insert at least one queued run so `strategy_records` / view caching
    // exercises the full path. Not strictly required — view runs even with
    // zero records — but keeps the fingerprint deterministic.
    let _ = store.insert_run(&sid, RunStatus::Queued)?;
    Ok(sid)
}

// ───── tests ─────

#[tokio::test]
async fn idle_balance_walk_no_provider_returns_empty_with_sentinel() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;
    // Fresh DB; no strategies.
    {
        let _ = StateStore::open(&db_path)?;
    }
    let (proc, addr) = spawn_with_db_and_unreachable_rpc(&db_path).await?;
    let body = http_get_json(addr, "/api/portfolio").await?;

    // The walk MUST short-circuit with no provider available OR fall through
    // to rpc_error when the unreachable RPC means we couldn't resolve
    // chain_id. Both shapes are honest and the v1.6 contract pins them as
    // valid sentinels — assert that the field is one of the four allowed
    // strings and that `idle_balances` is an empty array.
    let status = body["_balance_walk_status"]
        .as_str()
        .expect("_balance_walk_status sentinel present");
    assert!(
        matches!(status, "ok" | "no_provider" | "truncated" | "rpc_error"),
        "unexpected status: {status}"
    );
    assert!(
        body["idle_balances"].as_array().expect("array").is_empty(),
        "idle_balances must be empty when provider unreachable"
    );
    // chain_id must be present as a key (may be null when RPC is dead).
    assert!(body.get("chain_id").is_some(), "chain_id key present");
    // `assets` array always present (may be empty).
    assert!(
        body.get("assets").and_then(Value::as_array).is_some(),
        "assets array present"
    );
    drop(proc);
    Ok(())
}

#[tokio::test]
async fn assets_aggregate_carries_attribution_from_strategy_view() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

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
        seed_bundle_with_assets_view(&mut store, "aave-stub", assets)?
    };

    let (proc, addr) = spawn_with_db_and_unreachable_rpc(&db_path).await?;
    let body = http_get_json(addr, "/api/portfolio").await?;
    let assets = body["assets"].as_array().expect("assets array");
    assert_eq!(
        assets.len(),
        1,
        "exactly one $assets entry should aggregate; body={body}"
    );
    let e = &assets[0];
    assert_eq!(e["asset"], json!("USDC"));
    assert_eq!(e["venue"], json!("aave"));
    assert_eq!(e["amount"], json!("1.0"));
    assert_eq!(e["raw"], json!("1000000"));
    assert_eq!(e["decimals"], json!(6));
    assert_eq!(e["_attribution"], json!(sid));
    assert_eq!(e["_amount_conflict"], json!(false));
    assert_eq!(e["_truncated"], json!(false));
    // Strategy entry is shaped right too.
    let strategies = body["strategies"].as_array().expect("strategies array");
    assert_eq!(strategies.len(), 1);
    assert_eq!(strategies[0]["id"], json!(sid));
    assert!(strategies[0].get("view_output").is_some());
    drop(proc);
    Ok(())
}

#[tokio::test]
async fn conflicting_amounts_flag_both_entries() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let (sid_a, sid_b) = {
        let mut store = StateStore::open(&db_path)?;
        let a_assets = r#"[{
            "chain_id": 8453,
            "venue": "aave",
            "asset": "USDC",
            "amount": "1.0",
            "raw": "1000000",
            "decimals": 6,
            "address": null
        }]"#;
        let b_assets = r#"[{
            "chain_id": 8453,
            "venue": "aave",
            "asset": "USDC",
            "amount": "2.5",
            "raw": "2500000",
            "decimals": 6,
            "address": null
        }]"#;
        let a = seed_bundle_with_assets_view(&mut store, "claim-a", a_assets)?;
        let b = seed_bundle_with_assets_view(&mut store, "claim-b", b_assets)?;
        (a, b)
    };

    let (proc, addr) = spawn_with_db_and_unreachable_rpc(&db_path).await?;
    let body = http_get_json(addr, "/api/portfolio").await?;
    let assets = body["assets"].as_array().expect("assets array");
    assert_eq!(assets.len(), 2, "both entries should aggregate; body={body}");
    for entry in assets {
        assert_eq!(
            entry["_amount_conflict"],
            json!(true),
            "every conflicting entry must be flagged; entry={entry}"
        );
        let summary = entry["_conflict_summary"]
            .as_array()
            .expect("_conflict_summary array");
        assert_eq!(summary.len(), 2, "summary must include all attributed amounts");
        // Both sids must appear in the summary list.
        let attributions: Vec<&str> = summary
            .iter()
            .filter_map(|s| s.get("attribution").and_then(Value::as_str))
            .collect();
        assert!(attributions.contains(&sid_a.as_str()));
        assert!(attributions.contains(&sid_b.as_str()));
        let amounts: Vec<&str> = summary
            .iter()
            .filter_map(|s| s.get("amount").and_then(Value::as_str))
            .collect();
        assert!(amounts.contains(&"1.0"));
        assert!(amounts.contains(&"2.5"));
    }
    drop(proc);
    Ok(())
}

#[tokio::test]
async fn strategy_contributing_60_assets_truncates_to_50() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let sid = {
        let mut store = StateStore::open(&db_path)?;
        // Build a 60-entry literal via JS-side generation to avoid a giant
        // hard-coded string. The view returns Array(60).fill(...).map(...).
        let view = r#"(ctx, records) => {
            const out = [];
            for (let i = 0; i < 60; i++) {
                out.push({
                    chain_id: 8453,
                    venue: "wallet",
                    asset: "TOK" + i,
                    amount: "1",
                    raw: "1",
                    decimals: 0,
                    address: null,
                });
            }
            return { "$assets": out };
        }"#;
        let outcome = store.register_strategy_bundle(
            "many-assets",
            "(ctx) => 'noop'",
            None,
            None,
            None,
            Some(view),
            None,
            None,
        )?;
        let s = match outcome {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s,
            RegisterOutcome::ReplacedVersion { created, .. } => created,
        };
        s.id
    };

    let (proc, addr) = spawn_with_db_and_unreachable_rpc(&db_path).await?;
    let body = http_get_json(addr, "/api/portfolio").await?;
    let assets = body["assets"].as_array().expect("assets array");
    assert_eq!(
        assets.len(),
        50,
        "must truncate at MAX_ASSETS_PER_STRATEGY=50; got {} body={body}",
        assets.len()
    );
    // Every retained entry must carry the _truncated flag.
    for entry in assets {
        assert_eq!(entry["_truncated"], json!(true), "{entry}");
        assert_eq!(entry["_attribution"], json!(sid));
    }
    // The strategy envelope must also carry `_truncated: true`.
    let strategies = body["strategies"].as_array().expect("array");
    let s = strategies
        .iter()
        .find(|s| s.get("id").and_then(Value::as_str) == Some(sid.as_str()))
        .expect("strategy entry present");
    assert_eq!(s["_truncated"], json!(true));
    drop(proc);
    Ok(())
}
