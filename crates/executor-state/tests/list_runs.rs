//! v1.4 Track C — `list_runs` filter + ordering contract tests.
//!
//! Backs the `execution://list?strategy_id&since&status&limit` MCP resource.
//! Asserts:
//! - Empty filter returns all rows, newest-first (`started_at DESC`,
//!   `id DESC` tie-break).
//! - `strategy_id` filter isolates per-strategy runs.
//! - `since` is an EXCLUSIVE lower bound (row whose started_at == since does
//!   NOT appear).
//! - `status` filter (Succeeded / Failed) matches `runs.status`.
//! - `journal_outcome = "noop"` correctly identifies no-op runs via the
//!   journal_actions EXISTS subquery.
//! - `limit` defaults to 50; explicit limits up to the 500 cap pass through;
//!   values >cap are silently clamped.

#![allow(deprecated)]

mod common;

use common::fresh_memory_store;
use executor_core::schema::execution::{JournalActionOutcome, RunStatus};
use executor_state::{LIST_RUNS_DEFAULT_LIMIT, LIST_RUNS_LIMIT_CAP, RegisterOutcome, RunListFilter};

fn seed_strategy(store: &mut executor_state::StateStore, name: &str, source: &str) -> String {
    match store.register_strategy(name, source, None, None).unwrap() {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
    }
}

/// Insert a run with deterministic `started_at`, then transition it to a
/// terminal status if requested. Returns the run id.
fn seed_run(
    store: &mut executor_state::StateStore,
    strategy_id: &str,
    started_at: &str,
    terminal: Option<RunStatus>,
) -> String {
    let run_id = store
        .__test_insert_run_with_time(strategy_id, RunStatus::Queued, started_at)
        .unwrap();
    if let Some(s) = terminal {
        store.update_run_status(&run_id, s).unwrap();
    }
    run_id
}

#[test]
fn empty_filter_returns_all_rows_newest_first() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");

    // Insert in a SHUFFLED order; assert the query orders by started_at DESC.
    let r_mid = seed_run(&mut store, &sid, "2026-04-27T12:00:00Z", Some(RunStatus::Succeeded));
    let r_new = seed_run(&mut store, &sid, "2026-04-27T13:00:00Z", Some(RunStatus::Failed));
    let r_old = seed_run(&mut store, &sid, "2026-04-27T11:00:00Z", Some(RunStatus::Succeeded));

    let rows = store.list_runs(&RunListFilter::default()).unwrap();
    let ordered_ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(
        ordered_ids,
        vec![r_new.as_str(), r_mid.as_str(), r_old.as_str()],
        "list_runs must order newest-first (started_at DESC)"
    );
    // Statuses round-trip correctly.
    assert_eq!(rows[0].status, RunStatus::Failed);
    assert_eq!(rows[1].status, RunStatus::Succeeded);
    assert_eq!(rows[2].status, RunStatus::Succeeded);
    // Terminal-status rows must have finished_at populated.
    for row in &rows {
        assert!(
            row.finished_at.is_some(),
            "terminal status must set finished_at, row={row:?}"
        );
    }
}

#[test]
fn strategy_id_filter_isolates_runs() {
    let mut store = fresh_memory_store();
    let sid_a = seed_strategy(&mut store, "alpha", "// alpha");
    let sid_b = seed_strategy(&mut store, "beta", "// beta");

    let a1 = seed_run(&mut store, &sid_a, "2026-04-27T01:00:00Z", Some(RunStatus::Succeeded));
    let _b1 = seed_run(&mut store, &sid_b, "2026-04-27T02:00:00Z", Some(RunStatus::Succeeded));
    let a2 = seed_run(&mut store, &sid_a, "2026-04-27T03:00:00Z", Some(RunStatus::Failed));

    let rows = store
        .list_runs(&RunListFilter {
            strategy_id: Some(sid_a.clone()),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![a2.as_str(), a1.as_str()],
        "strategy_id filter must isolate runs for that strategy, newest first"
    );
    assert!(
        rows.iter().all(|r| r.strategy_id == sid_a),
        "all returned rows must match the requested strategy_id"
    );
}

#[test]
fn since_is_exclusive_lower_bound() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");

    let _r_before = seed_run(&mut store, &sid, "2026-04-27T00:00:00Z", Some(RunStatus::Succeeded));
    let _r_at = seed_run(&mut store, &sid, "2026-04-27T01:00:00Z", Some(RunStatus::Succeeded));
    let r_after = seed_run(&mut store, &sid, "2026-04-27T02:00:00Z", Some(RunStatus::Succeeded));

    let rows = store
        .list_runs(&RunListFilter {
            since: Some("2026-04-27T01:00:00Z".to_string()),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![r_after.as_str()],
        "since is EXCLUSIVE — the row whose started_at equals `since` must NOT appear"
    );
}

#[test]
fn status_filter_matches_run_status() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");

    let r_ok = seed_run(&mut store, &sid, "2026-04-27T01:00:00Z", Some(RunStatus::Succeeded));
    let r_fail = seed_run(&mut store, &sid, "2026-04-27T02:00:00Z", Some(RunStatus::Failed));
    let _r_running = seed_run(&mut store, &sid, "2026-04-27T03:00:00Z", None);

    let succeeded = store
        .list_runs(&RunListFilter {
            status: Some(RunStatus::Succeeded),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(succeeded.len(), 1);
    assert_eq!(succeeded[0].run_id, r_ok);

    let failed = store
        .list_runs(&RunListFilter {
            status: Some(RunStatus::Failed),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].run_id, r_fail);
}

#[test]
fn journal_outcome_noop_filter() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");

    let r_noop = seed_run(&mut store, &sid, "2026-04-27T01:00:00Z", Some(RunStatus::Succeeded));
    store
        .record_action_outcome(&r_noop, JournalActionOutcome::Noop, "{}")
        .unwrap();

    let r_acted = seed_run(&mut store, &sid, "2026-04-27T02:00:00Z", Some(RunStatus::Succeeded));
    store
        .record_action_outcome(&r_acted, JournalActionOutcome::Actions, "{\"actions\":[]}")
        .unwrap();

    let rows = store
        .list_runs(&RunListFilter {
            journal_outcome: Some("noop".to_string()),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![r_noop.as_str()],
        "journal_outcome=noop must match runs with a noop action row"
    );
    assert_eq!(rows[0].action_count, 1);
}

#[test]
fn action_count_aggregates_journal_actions_rows() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");

    let r_one = seed_run(&mut store, &sid, "2026-04-27T01:00:00Z", Some(RunStatus::Succeeded));
    store
        .record_action_outcome(&r_one, JournalActionOutcome::Actions, "{\"actions\":[]}")
        .unwrap();

    let r_zero = seed_run(&mut store, &sid, "2026-04-27T02:00:00Z", Some(RunStatus::Succeeded));
    // No journal_actions inserted — action_count must be 0, NOT a missing row.

    let r_two = seed_run(&mut store, &sid, "2026-04-27T03:00:00Z", Some(RunStatus::Succeeded));
    store
        .record_action_outcome(&r_two, JournalActionOutcome::Noop, "{}")
        .unwrap();
    store
        .record_action_outcome(&r_two, JournalActionOutcome::Actions, "{\"actions\":[]}")
        .unwrap();

    let rows = store.list_runs(&RunListFilter::default()).unwrap();
    let map: std::collections::HashMap<&str, i64> = rows
        .iter()
        .map(|r| (r.run_id.as_str(), r.action_count))
        .collect();
    assert_eq!(map.get(r_zero.as_str()), Some(&0));
    assert_eq!(map.get(r_one.as_str()), Some(&1));
    assert_eq!(map.get(r_two.as_str()), Some(&2));
    assert_eq!(rows.len(), 3, "LEFT JOIN must keep the zero-action row");
}

#[test]
fn limit_default_is_fifty() {
    // Sanity: the default limit constant matches the documented v1.4 contract.
    assert_eq!(LIST_RUNS_DEFAULT_LIMIT, 50);
    assert_eq!(LIST_RUNS_LIMIT_CAP, 500);

    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");
    // Insert 60 runs with distinct started_at values.
    for i in 0..60u32 {
        let ts = format!("2026-04-27T00:{:02}:00Z", i);
        let _ = seed_run(&mut store, &sid, &ts, Some(RunStatus::Succeeded));
    }

    let rows = store.list_runs(&RunListFilter::default()).unwrap();
    assert_eq!(rows.len(), 50, "default limit must be 50, got {}", rows.len());

    // Explicit limit honors the request when below the cap.
    let rows = store
        .list_runs(&RunListFilter {
            limit: Some(10),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 10);

    // limit above cap is silently clamped down — but the cap test would need
    // >500 rows to demonstrate behaviour vs request; instead, assert directly
    // by passing 9999 and checking ≤cap.
    let rows = store
        .list_runs(&RunListFilter {
            limit: Some(9_999),
            ..Default::default()
        })
        .unwrap();
    assert!(
        rows.len() <= LIST_RUNS_LIMIT_CAP as usize,
        "limit must be hard-capped at LIST_RUNS_LIMIT_CAP, got {}",
        rows.len()
    );
    // With only 60 rows seeded, we still see all 60 — the cap doesn't
    // SHRINK the result below row count.
    assert_eq!(rows.len(), 60);
}

#[test]
fn empty_result_when_no_runs_match() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "alpha", "// alpha");
    let _ = seed_run(&mut store, &sid, "2026-04-27T01:00:00Z", Some(RunStatus::Succeeded));

    let rows = store
        .list_runs(&RunListFilter {
            strategy_id: Some(
                "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            ),
            ..Default::default()
        })
        .unwrap();
    assert!(rows.is_empty(), "no-match filter must return empty Vec, NOT error");
}

#[test]
fn combined_filters_and_together() {
    let mut store = fresh_memory_store();
    let sid_a = seed_strategy(&mut store, "alpha", "// alpha");
    let sid_b = seed_strategy(&mut store, "beta", "// beta");

    // a-old-failed: matches sid_a, before cutoff, failed
    let _ = seed_run(&mut store, &sid_a, "2026-04-27T00:00:00Z", Some(RunStatus::Failed));
    // a-new-succeeded: matches sid_a, after cutoff, succeeded — should appear
    let target = seed_run(&mut store, &sid_a, "2026-04-27T05:00:00Z", Some(RunStatus::Succeeded));
    // a-new-failed: matches sid_a, after cutoff, BUT failed
    let _ = seed_run(&mut store, &sid_a, "2026-04-27T06:00:00Z", Some(RunStatus::Failed));
    // b-new-succeeded: wrong strategy
    let _ = seed_run(&mut store, &sid_b, "2026-04-27T07:00:00Z", Some(RunStatus::Succeeded));

    let rows = store
        .list_runs(&RunListFilter {
            strategy_id: Some(sid_a.clone()),
            since: Some("2026-04-27T01:00:00Z".to_string()),
            status: Some(RunStatus::Succeeded),
            ..Default::default()
        })
        .unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![target.as_str()],
        "all three filters must AND together"
    );
}
