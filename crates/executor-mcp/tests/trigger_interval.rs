//! v1.2 Trigger Core (Stream D) integration test.
//!
//! Boots `ExecutorServer::from_config`, which spawns the dispatcher task and
//! a worker pool seeded from `triggers` rows pre-inserted via `StateStore`.
//! After waiting ~1.5s with a 200ms interval trigger, we expect >= 5
//! `trigger_events` rows, each with a non-NULL `run_id`.

use std::path::PathBuf;

use anyhow::Result;
use executor_mcp::ExecutorServer;
use executor_mcp::config::Config;
use executor_state::StateStore;
use executor_core::schema::trigger::{RegisterTriggerInput, TriggerKind};
use executor_state::TriggerRegisterOutcome;

fn make_config(state_path: &str) -> Config {
    let mut cfg = Config::default();
    cfg.state.path = state_path.to_string();
    cfg
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interval_worker_fires_and_records_events() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let db_path: PathBuf = tmp.path().join("trigger.db");
    let db_str = db_path.to_str().unwrap().to_string();

    // 1. Seed strategy + trigger directly via StateStore (no MCP tool yet
    //    for trigger_register — Stream C lands that).
    let trigger_id = {
        let mut store = StateStore::open(&db_path)?;
        let outcome = store.register_strategy(
            "trigger_noop_strategy",
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
            kind: TriggerKind::Interval,
            config: serde_json::json!({ "interval_ms": 200 }),
            predicate: None,
            dedup_window_ms: None,
            note: None,
        })?;
        match outcome {
            TriggerRegisterOutcome::Created(t) => t.id,
            TriggerRegisterOutcome::AlreadyExists(t) => t.id,
        }
    };

    // 2. Boot the server — spawns dispatcher + interval worker for our trigger.
    let cfg = make_config(&db_str);
    let _server = ExecutorServer::from_config(&cfg)?;

    // 3. Wait for ticks.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // 4. Inspect SQLite directly.
    let conn = rusqlite::Connection::open(&db_path)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trigger_events WHERE trigger_id = ?1",
        rusqlite::params![trigger_id],
        |r| r.get(0),
    )?;
    assert!(
        count >= 5,
        "expected >= 5 trigger_events at 200ms cadence over 1.5s, got {count}",
    );

    let null_run_ids: i64 = conn.query_row(
        "SELECT COUNT(*) FROM trigger_events WHERE trigger_id = ?1 AND run_id IS NULL",
        rusqlite::params![trigger_id],
        |r| r.get(0),
    )?;
    assert_eq!(
        null_run_ids, 0,
        "every fired trigger_event should have a non-NULL run_id; got {null_run_ids} NULLs",
    );

    Ok(())
}
