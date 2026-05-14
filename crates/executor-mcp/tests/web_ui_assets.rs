//! v1.6 Track 6B — integration tests for the local web UI static assets.
//!
//! The frontend is embedded via `include_str!` and served from three
//! routes: `/index.html`, `/static/style.css`, `/static/app.js`. Plus a
//! 307 redirect from `/` to `/index.html#portfolio`. This test asserts:
//! - each asset route returns 200 with the right content-type
//! - `/index.html` contains every tab label so the shell stays intact
//! - `/` 307s to `/index.html#portfolio`
//!
//! We deliberately do not exercise the JS in a headless browser — the
//! Rust test surface is integrity-only.

#![allow(clippy::print_stdout, clippy::print_stderr)]

mod common;

use std::{
    net::{Ipv4Addr, SocketAddr},
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::Result;
use common::initialize;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

/// Mirror of `web_api::wait_for_ui_url` (kept local so the two test files
/// stay independent). Walks stderr until we see the `🌐 UI: http://...`
/// sentinel and returns the bound socket address.
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
                let needle = "\u{1F310} UI: http://127.0.0.1:";
                if let Some(idx) = line.find(needle) {
                    let tail = &line[idx + needle.len()..];
                    let end = tail
                        .find(|c: char| !c.is_ascii_digit())
                        .unwrap_or(tail.len());
                    if let Ok(port) = tail[..end].parse::<u16>() {
                        // Drain remaining stderr so the child's pipe never blocks.
                        let mut rest = reader;
                        tokio::spawn(async move {
                            while let Ok(Some(_)) = rest.next_line().await {}
                        });
                        return Some(SocketAddr::new(
                            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
                            port,
                        ));
                    }
                }
            }
            Ok(Ok(None)) => return None,
            Ok(Err(_)) | Err(_) => return None,
        }
    }
}

async fn spawn_with_ui() -> Result<(common::ServerProc, SocketAddr)> {
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
    initialize(&mut server).await?;
    Ok((server, addr))
}

async fn http_head(addr: SocketAddr, path: &str) -> Result<reqwest::Response> {
    let url = format!("http://{addr}{path}");
    let resp = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()?
        .get(&url)
        .send()
        .await?;
    Ok(resp)
}

#[tokio::test]
async fn style_css_served_with_correct_mime() -> Result<()> {
    let (_server, addr) = spawn_with_ui().await?;
    let resp = http_head(addr, "/static/style.css").await?;
    assert_eq!(resp.status().as_u16(), 200);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/css"),
        "expected text/css content-type, got: {ct}"
    );
    let body = resp.text().await?;
    assert!(!body.trim().is_empty(), "style.css body must be non-empty");
    Ok(())
}

#[tokio::test]
async fn app_js_served_with_correct_mime() -> Result<()> {
    let (_server, addr) = spawn_with_ui().await?;
    let resp = http_head(addr, "/static/app.js").await?;
    assert_eq!(resp.status().as_u16(), 200);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/javascript"),
        "expected application/javascript content-type, got: {ct}"
    );
    let body = resp.text().await?;
    assert!(!body.trim().is_empty(), "app.js body must be non-empty");
    Ok(())
}

#[tokio::test]
async fn root_redirects_to_portfolio_tab() -> Result<()> {
    let (_server, addr) = spawn_with_ui().await?;
    let resp = http_head(addr, "/").await?;
    // axum's `Redirect::temporary` emits 307.
    assert_eq!(
        resp.status().as_u16(),
        307,
        "expected 307 temporary redirect for `/`"
    );
    let loc = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert_eq!(loc, "/index.html#portfolio", "redirect must land on the portfolio tab");
    Ok(())
}

#[tokio::test]
async fn index_html_contains_all_five_tab_labels() -> Result<()> {
    let (_server, addr) = spawn_with_ui().await?;
    let resp = http_head(addr, "/index.html").await?;
    assert_eq!(resp.status().as_u16(), 200);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/html"),
        "expected text/html content-type, got: {ct}"
    );
    let body = resp.text().await?;
    for label in ["Portfolio", "Strategies", "Policy", "Triggers", "History"] {
        assert!(
            body.contains(label),
            "/index.html must contain tab label `{label}`; body was:\n{body}"
        );
    }
    // The shell must reference both static assets so the browser loads them.
    assert!(
        body.contains("/static/style.css"),
        "/index.html must link to /static/style.css"
    );
    assert!(
        body.contains("/static/app.js"),
        "/index.html must link to /static/app.js"
    );
    Ok(())
}
