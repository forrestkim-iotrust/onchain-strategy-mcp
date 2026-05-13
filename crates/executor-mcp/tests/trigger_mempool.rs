//! v1.2 Stream E — live mempool worker integration test.
//!
//! Skipped by default. Opt in by:
//!   1. Building with `--features mempool-live-tests`, AND
//!   2. Exporting `ALCHEMY_WSS_URL=wss://.../v2/<key>`.
//!
//! When either condition is absent the test is a noop (logs a message and
//! returns) so the default `cargo test --workspace` run stays hermetic.
//!
//! Confirmed Alchemy push cadence on Base mainnet for the USDC contract is
//! typically several txs per second, so a 5s soak comfortably yields >=1
//! trigger_events row.

use std::path::PathBuf;

use anyhow::Result;
use executor_core::schema::trigger::{RegisterTriggerInput, TriggerKind};
use executor_mcp::ExecutorServer;
use executor_mcp::config::Config;
use executor_state::StateStore;
use executor_state::TriggerRegisterOutcome;

/// Base mainnet USDC — high-volume pending tx target for the smoke test.
const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

fn make_config(state_path: &str, wss_url: &str) -> Config {
    let mut cfg = Config::default();
    cfg.state.path = state_path.to_string();
    cfg.trigger.mempool_wss_url = Some(wss_url.to_string());
    cfg
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "live Alchemy WSS — opt-in via ALCHEMY_WSS_URL env var"]
async fn mempool_worker_fires_against_live_alchemy_wss() -> Result<()> {
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
            "mempool_noop_strategy",
            "(ctx) => \"noop\"",
            None,
            None,
        )?;
        let sid = match outcome {
            executor_state::strategies::RegisterOutcome::Created(s) => s.id,
            executor_state::strategies::RegisterOutcome::AlreadyExists(s) => s.id,
        };
        let outcome = store.register_trigger(RegisterTriggerInput {
            strategy_id: sid,
            kind: TriggerKind::Mempool,
            config: serde_json::json!({ "to_address": [USDC_BASE] }),
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

    // Soak: 5s on Base mainnet USDC is plenty.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let conn = rusqlite::Connection::open(&db_path)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trigger_events WHERE trigger_id = ?1",
        rusqlite::params![trigger_id],
        |r| r.get(0),
    )?;
    assert!(
        count >= 1,
        "expected >= 1 mempool trigger_events in 5s soak on Base USDC, got {count}",
    );
    Ok(())
}
