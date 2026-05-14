//! v1.2 Stream F — live log subscription worker integration test.
//!
//! Skipped by default. Opt in by exporting
//!   `ALCHEMY_WSS_URL=wss://.../v2/<key>`
//! and running `cargo test --test trigger_log -- --ignored`.
//!
//! Strategy: subscribe to USDC `Transfer` events on Base mainnet (no
//! recipient filter — Base USDC sees several transfers per block, so a 30s
//! soak comfortably yields >= 1 trigger_events row with a real txHash in
//! the event_json payload.

use std::path::PathBuf;

use anyhow::Result;
use executor_core::schema::trigger::{RegisterTriggerInput, TriggerKind};
use executor_mcp::ExecutorServer;
use executor_mcp::config::Config;
use executor_state::StateStore;
use executor_state::TriggerRegisterOutcome;

/// Base mainnet USDC contract.
const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
/// `Transfer(address,address,uint256)` event signature (topic0).
const TRANSFER_TOPIC0: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

fn make_config(state_path: &str, wss_url: &str) -> Config {
    let mut cfg = Config::default();
    cfg.state.path = state_path.to_string();
    cfg.trigger.mempool_wss_url = Some(wss_url.to_string());
    cfg
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live Alchemy WSS — opt-in via ALCHEMY_WSS_URL env var"]
async fn log_worker_fires_against_live_alchemy_wss() -> Result<()> {
    let Ok(wss_url) = std::env::var("ALCHEMY_WSS_URL") else {
        // Live opt-in only; default test run leaves this ignored AND no-op.
        return Ok(());
    };

    let tmp = tempfile::tempdir()?;
    let db_path: PathBuf = tmp.path().join("trigger.db");
    let db_str = db_path.to_str().unwrap().to_string();

    let trigger_id = {
        let mut store = StateStore::open(&db_path)?;
        let outcome = store.register_strategy(
            "log_noop_strategy",
            "(ctx) => \"noop\"",
            None,
            None,
        )?;
        let sid = match outcome {
            executor_state::strategies::RegisterOutcome::Created(s) => s.id,
            executor_state::strategies::RegisterOutcome::AlreadyExists(s) => s.id,
            executor_state::strategies::RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        };
        let outcome = store.register_trigger(RegisterTriggerInput {
            strategy_id: sid,
            kind: TriggerKind::Log,
            config: serde_json::json!({
                "address": USDC_BASE,
                "topics": [TRANSFER_TOPIC0]
            }),
            predicate: None,
            dedup_window_ms: None,
        })?;
        match outcome {
            TriggerRegisterOutcome::Created(t) => t.id,
            TriggerRegisterOutcome::AlreadyExists(t) => t.id,
        }
    };

    let cfg = make_config(&db_str, &wss_url);
    let _server = ExecutorServer::from_config(&cfg)?;

    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

    let conn = rusqlite::Connection::open(&db_path)?;
    let mut stmt = conn.prepare(
        "SELECT event_json FROM trigger_events WHERE trigger_id = ?1",
    )?;
    let rows: Vec<String> = stmt
        .query_map(rusqlite::params![trigger_id], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    assert!(
        !rows.is_empty(),
        "expected >= 1 log trigger_events in 30s soak on Base USDC, got 0",
    );
    let has_tx_hash = rows.iter().any(|j| {
        serde_json::from_str::<serde_json::Value>(j)
            .ok()
            .and_then(|v| {
                v.get("transactionHash")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .is_some_and(|h| h.starts_with("0x") && h.len() == 66)
    });
    assert!(has_tx_hash, "no event_json row has a valid transactionHash");
    Ok(())
}
