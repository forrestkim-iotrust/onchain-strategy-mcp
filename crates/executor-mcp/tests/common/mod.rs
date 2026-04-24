#![allow(dead_code, unreachable_pub)]
//! Shared integration-test helpers. Plan 02/03 import the `spawn_server`,
//! `send`, `recv`, and `initialize` helpers; Plan 01 only exercises
//! `spawn_server` from the `harness_compiles` smoke test.

use anyhow::Result;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{Duration, timeout};

pub struct ServerProc {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

pub async fn spawn_server() -> Result<ServerProc> {
    let bin = env!("CARGO_BIN_EXE_executor-mcp");
    let mut child = Command::new(bin)
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // Drain stderr so the child's stderr pipe never blocks.
    let stderr = child.stderr.take().expect("stderr piped");
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buf = String::new();
        while reader.read_line(&mut buf).await.unwrap_or(0) > 0 {
            buf.clear();
        }
    });

    let stdin = child.stdin.take().expect("stdin piped");
    let stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
    Ok(ServerProc {
        child,
        stdin,
        stdout,
    })
}

pub async fn send(proc: &mut ServerProc, msg: Value) -> Result<()> {
    let line = serde_json::to_string(&msg)? + "\n";
    proc.stdin.write_all(line.as_bytes()).await?;
    proc.stdin.flush().await?;
    Ok(())
}

pub async fn recv(proc: &mut ServerProc) -> Result<Value> {
    let mut line = String::new();
    timeout(Duration::from_secs(5), proc.stdout.read_line(&mut line)).await??;
    // KEY ASSERTION: every stdout line must parse as JSON-RPC 2.0.
    let v: Value = serde_json::from_str(line.trim_end()).map_err(|e| {
        anyhow::anyhow!("stdout line is not JSON-RPC: {:?} — line={:?}", e, line)
    })?;
    assert_eq!(
        v.get("jsonrpc").and_then(Value::as_str),
        Some("2.0"),
        "message missing jsonrpc: 2.0"
    );
    Ok(v)
}

pub async fn initialize(proc: &mut ServerProc) -> Result<Value> {
    send(
        proc,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "phase1-test", "version": "0" }
            }
        }),
    )
    .await?;
    let res = recv(proc).await?;
    send(
        proc,
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    )
    .await?;
    Ok(res)
}
