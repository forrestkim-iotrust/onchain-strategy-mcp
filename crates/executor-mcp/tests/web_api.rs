//! v1.6 Track 6A — integration tests for the local web UI HTTP API.
//!
//! Spawns the full `executor-mcp` binary with a throwaway config, then
//! probes the `/api/*` routes over loopback HTTP. Stdout discipline is
//! enforced by the shared `common::recv` helper (stdin/stdout JSON-RPC
//! parsing), so these tests double as a regression for "the UI server
//! must NOT leak text onto stdout".

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod common;

use std::{
    net::{Ipv4Addr, SocketAddr, TcpListener as StdTcpListener},
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::Result;
use common::{initialize, send, spawn_server_with_config_text_and_env};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time::sleep,
};

/// Walk stderr lines from a child process for up to `wait` and return the
/// URL we logged at boot (`🌐 UI: http://127.0.0.1:PORT`). Returns `None`
/// when nothing matched within the deadline — the caller is expected to
/// treat that as "the UI never bound".
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
                    // Drain the rest of stderr asynchronously so the child's
                    // stderr pipe never blocks.
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

fn parse_ui_log_line(line: &str) -> Option<SocketAddr> {
    // The tracing line has the shape `... 🌐 UI: http://127.0.0.1:PORT ...`.
    // Match the leading sentinel to avoid colliding with the EVM RPC URL
    // line (`evm_rpc=http://127.0.0.1:8545`) that tracing logs at boot.
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

/// Boot the server with a minimal config (in-memory SQLite, no RPC), drive
/// `initialize`, and return the UI socket address. Each test gets a fresh
/// binary process.
async fn spawn_with_ui_default() -> Result<(common::ServerProc, SocketAddr)> {
    let cfg = "[state]\npath = \":memory:\"\n";
    let proc = spawn_server_with_config_text_and_env(cfg, &[]).await?;
    finish_boot(proc).await
}

async fn spawn_with_ui_disabled_env() -> Result<common::ServerProc> {
    let cfg = "[state]\npath = \":memory:\"\n";
    let mut proc =
        spawn_server_with_config_text_and_env(cfg, &[("OSMCP_NO_UI", "1")]).await?;
    initialize(&mut proc).await?;
    Ok(proc)
}

async fn finish_boot(proc: common::ServerProc) -> Result<(common::ServerProc, SocketAddr)> {
    // The default `spawn_server_with_config_text_and_env` consumes the
    // child's stderr; we need it to find the UI port. Re-spawn directly.
    drop(proc);
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let tmp = tempfile::NamedTempFile::new()?;
    let cfg_path = tmp.path().to_path_buf();
    std::fs::write(&cfg_path, "[state]\npath = \":memory:\"\n")?;
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
    let mut server = common::ServerProc {
        child,
        stdin,
        stdout,
    };
    // Drive MCP initialize so the rmcp transport state is sane before we
    // start poking the UI side-car.
    initialize(&mut server).await?;
    Ok((server, addr))
}

async fn http_get_json(addr: SocketAddr, path: &str) -> Result<Value> {
    let url = format!("http://{addr}{path}");
    let body: Value = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(body)
}

async fn http_status(addr: SocketAddr, path: &str, method: reqwest::Method) -> Result<u16> {
    let url = format!("http://{addr}{path}");
    let resp = reqwest::Client::new()
        .request(method, &url)
        .timeout(Duration::from_secs(2))
        .send()
        .await?;
    Ok(resp.status().as_u16())
}

// ─────────── tests ───────────

#[tokio::test]
async fn portfolio_returns_burner_chain_and_strategies() -> Result<()> {
    let (mut _server, addr) = spawn_with_ui_default().await?;
    let body = http_get_json(addr, "/api/portfolio").await?;
    assert!(body.is_object(), "portfolio must be JSON object: {body}");
    let burner = body
        .get("burner")
        .and_then(Value::as_str)
        .expect("burner field present");
    assert!(burner.starts_with("0x"), "burner is a hex address: {burner}");
    assert!(
        body.get("refreshed_at").and_then(Value::as_str).is_some(),
        "refreshed_at present"
    );
    assert!(
        body.get("idle_balances").and_then(Value::as_array).is_some(),
        "idle_balances is an array"
    );
    let strategies = body
        .get("strategies")
        .and_then(Value::as_array)
        .expect("strategies array");
    // Fresh DB → empty.
    assert!(strategies.is_empty());
    // Force the child to exit cleanly by dropping the stdin handle.
    drop(_server);
    Ok(())
}

#[tokio::test]
async fn strategies_route_returns_list_envelope() -> Result<()> {
    let (mut _server, addr) = spawn_with_ui_default().await?;
    let body = http_get_json(addr, "/api/strategies").await?;
    let strategies = body
        .get("strategies")
        .and_then(Value::as_array)
        .expect("strategies key with array");
    assert!(strategies.is_empty(), "fresh DB has no strategies");
    drop(_server);
    Ok(())
}

#[tokio::test]
async fn policy_route_returns_current_and_history() -> Result<()> {
    let (mut _server, addr) = spawn_with_ui_default().await?;
    let body = http_get_json(addr, "/api/policy").await?;
    let current = body.get("current").expect("current envelope");
    // Fresh DB has no policy → loaded=false.
    assert_eq!(
        current.get("loaded").and_then(Value::as_bool),
        Some(false),
        "fresh DB → policy not loaded"
    );
    let history = body.get("history").expect("history envelope");
    let revisions = history
        .get("revisions")
        .and_then(Value::as_array)
        .expect("revisions array");
    assert!(revisions.is_empty(), "fresh DB has no policy revisions");
    drop(_server);
    Ok(())
}

#[tokio::test]
async fn triggers_runs_routes_return_arrays() -> Result<()> {
    let (mut _server, addr) = spawn_with_ui_default().await?;
    let triggers = http_get_json(addr, "/api/triggers").await?;
    assert!(
        triggers.get("triggers").and_then(Value::as_array).is_some(),
        "triggers route returns array"
    );
    let runs = http_get_json(addr, "/api/runs").await?;
    assert!(
        runs.get("runs").and_then(Value::as_array).is_some(),
        "runs route returns array"
    );
    drop(_server);
    Ok(())
}

#[tokio::test]
async fn post_is_rejected_with_405() -> Result<()> {
    let (mut _server, addr) = spawn_with_ui_default().await?;
    let status = http_status(addr, "/api/portfolio", reqwest::Method::POST).await?;
    assert_eq!(status, 405, "POST must 405 — UI is observation-only");
    drop(_server);
    Ok(())
}

#[tokio::test]
async fn disabled_via_env_var_does_not_bind() -> Result<()> {
    // Spawn the server with OSMCP_NO_UI=1 and confirm nothing is listening
    // on 8473. We can't easily check via reqwest (a closed port returns
    // ConnectionRefused immediately), so probe the port with a blocking
    // bind: if the bind succeeds, nobody is using the port.
    let mut server = spawn_with_ui_disabled_env().await?;
    // Drive a no-op initialize round-trip so the MCP path is alive.
    let _ = send(
        &mut server,
        json!({
            "jsonrpc": "2.0", "id": 99, "method": "resources/templates/list", "params": {}
        }),
    )
    .await;

    // Quick check that 8473 is unowned. There's a race if the host has
    // another process using 8473, but with OSMCP_NO_UI=1 our server is
    // guaranteed not to take it.
    let probe = StdTcpListener::bind(("127.0.0.1", executor_mcp::web::DEFAULT_UI_PORT));
    // If 8473 is busy for an unrelated reason, hit a fallback port instead.
    let port_status = probe.is_ok();
    drop(probe);

    if !port_status {
        eprintln!("disabled_via_env_var_does_not_bind: 8473 busy on host — test inconclusive");
    }

    // Verify the UI never logged its url line: grep stderr would be racy.
    // Instead, attempt an HTTP request to localhost:8473 and require it
    // either refuses or doesn't return one of our routes. (When 8473 is
    // squatted by something else, the response will not be our JSON shape.)
    let url = format!(
        "http://127.0.0.1:{}/api/strategies",
        executor_mcp::web::DEFAULT_UI_PORT
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()?;
    match client.get(&url).send().await {
        Err(_) => {
            // Refused — exactly what we want.
        }
        Ok(resp) => {
            // Got a response from SOMETHING — but it must not be ours. Our
            // `/api/strategies` returns JSON with a `strategies` array.
            let txt = resp.text().await.unwrap_or_default();
            let parsed: serde_json::Result<Value> = serde_json::from_str(&txt);
            if let Ok(v) = parsed {
                assert!(
                    v.get("strategies").and_then(Value::as_array).is_none(),
                    "OSMCP_NO_UI=1 but /api/strategies returned our envelope: {v}"
                );
            }
        }
    }
    drop(server);
    Ok(())
}

#[tokio::test]
async fn fallback_picks_next_free_port_when_default_busy() -> Result<()> {
    // Squat on 8473 BEFORE spawning the server. The server must log a port
    // that isn't 8473 (or skip the test if 8473 is already busy).
    let squat = StdTcpListener::bind(("127.0.0.1", executor_mcp::web::DEFAULT_UI_PORT));
    let squat = match squat {
        Ok(l) => l,
        Err(_) => {
            eprintln!(
                "fallback_picks_next_free_port: 8473 already busy on host — test inconclusive"
            );
            return Ok(());
        }
    };
    // Keep the squat alive for the full test.
    let _squat_guard = squat;

    let (mut _server, addr) = spawn_with_ui_default().await?;
    assert_ne!(
        addr.port(),
        executor_mcp::web::DEFAULT_UI_PORT,
        "fallback must pick a non-default port when 8473 is busy"
    );
    // Quick smoke: the chosen port actually serves requests.
    let body = http_get_json(addr, "/api/strategies").await?;
    assert!(body.is_object());
    drop(_server);
    sleep(Duration::from_millis(50)).await;
    Ok(())
}
