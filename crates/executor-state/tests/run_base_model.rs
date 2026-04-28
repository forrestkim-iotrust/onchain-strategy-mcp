//! Run repository contract tests (D-04b, D-05a, D-05c).
//!
//! NOTE: This file intentionally exercises the deprecated legacy
//! `update_run_status` API (MR-02) to lock in pre-D-12 contract semantics
//! (reserved-variant gate, terminal `finished_at` autofill, NotFound).
//! The transition-guarded variant has its own tests in
//! `update_run_status_with_transition.rs` etc.
#![allow(deprecated)]

mod common;

use common::fresh_memory_store;
use executor_core::schema::execution::RunStatus;
use executor_state::{RegisterOutcome, StateError};

fn seed_strategy(store: &mut executor_state::StateStore, name: &str, source: &str) -> String {
    match store.register_strategy(name, source, None, None).unwrap() {
        RegisterOutcome::Created(s) => s.id,
        _ => unreachable!(),
    }
}

#[test]
fn run_roundtrip_insert_get_update_status() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let run_id = store
        .insert_run(&strategy_id, RunStatus::Queued)
        .expect("insert_run");
    assert_eq!(run_id.len(), 26, "ULID is 26 chars Crockford Base32");

    let row = store.get_run(&run_id).unwrap().expect("present");
    assert_eq!(row.status, RunStatus::Queued);
    assert!(!row.started_at.is_empty());
    assert!(row.finished_at.is_none());

    store
        .update_run_status(&run_id, RunStatus::Running)
        .expect("update→running");
    assert_eq!(store.get_run(&run_id).unwrap().unwrap().status, RunStatus::Running);

    store
        .update_run_status(&run_id, RunStatus::Succeeded)
        .expect("update→succeeded");
    let final_row = store.get_run(&run_id).unwrap().unwrap();
    assert_eq!(final_row.status, RunStatus::Succeeded);
    assert!(final_row.finished_at.is_some(), "terminal status fills finished_at");
}

#[test]
fn phase5_emittable_rejects_phase6_reserved_variant() {
    // Phase 5 D-10 widens phase5_emittable to allow {SimulationDenied,
    // PolicyDenied}. Only Canceled stays Phase-6-reserved.
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let err = store
        .insert_run(&strategy_id, RunStatus::Canceled)
        .expect_err("Canceled stays reserved beyond Phase 5");
    match err {
        StateError::InvalidInput(msg) => {
            assert!(
                msg.to_lowercase().contains("reserved"),
                "message should mention reserved: {msg}"
            );
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }

    // All Phase 5 emittable variants succeed.
    for ok in [
        RunStatus::Queued,
        RunStatus::Running,
        RunStatus::Succeeded,
        RunStatus::Failed,
        RunStatus::SimulationDenied,
        RunStatus::PolicyDenied,
    ] {
        store.insert_run(&strategy_id, ok).expect("phase5 status ok");
    }
}

// ─────────── Plan 02-03 lifecycle / ordering / shape contracts ───────────

#[test]
fn update_run_status_sets_finished_at_on_succeeded() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let run_id = store.insert_run(&strategy_id, RunStatus::Queued).unwrap();
    store.update_run_status(&run_id, RunStatus::Running).unwrap();
    let mid = store.get_run(&run_id).unwrap().unwrap();
    assert!(
        mid.finished_at.is_none(),
        "Running must NOT set finished_at; got {:?}",
        mid.finished_at
    );

    store.update_run_status(&run_id, RunStatus::Succeeded).unwrap();
    let final_row = store.get_run(&run_id).unwrap().unwrap();
    assert_eq!(final_row.status, RunStatus::Succeeded);
    let fa = final_row.finished_at.expect("Succeeded must populate finished_at");
    assert!(!fa.is_empty(), "finished_at must be a non-empty RFC3339");
}

#[test]
fn update_run_status_sets_finished_at_on_failed() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let run_id = store.insert_run(&strategy_id, RunStatus::Queued).unwrap();
    store.update_run_status(&run_id, RunStatus::Failed).unwrap();
    let row = store.get_run(&run_id).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::Failed);
    let fa = row.finished_at.expect("Failed must populate finished_at");
    assert!(!fa.is_empty());
}

#[test]
fn update_run_status_leaves_finished_at_none_on_queued_or_running() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let run_id = store.insert_run(&strategy_id, RunStatus::Queued).unwrap();
    let q = store.get_run(&run_id).unwrap().unwrap();
    assert!(q.finished_at.is_none(), "Queued: finished_at must be None");

    store.update_run_status(&run_id, RunStatus::Running).unwrap();
    let r = store.get_run(&run_id).unwrap().unwrap();
    assert!(r.finished_at.is_none(), "Running: finished_at must be None");
}

#[test]
fn update_run_status_on_missing_id_returns_not_found() {
    let mut store = fresh_memory_store();
    let _ = seed_strategy(&mut store, "arb", "// code");

    let err = store
        .update_run_status("01HGXNONEXISTENTRUNIDXXXXX", RunStatus::Running)
        .expect_err("update on missing id must error");
    match err {
        StateError::NotFound(msg) => {
            assert!(
                msg.contains("01HGXNONEXISTENTRUNIDXXXXX"),
                "NotFound should reference the run id, got {msg}"
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn insert_run_returns_ulid_shape() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let id = store.insert_run(&strategy_id, RunStatus::Queued).unwrap();
    assert_eq!(id.len(), 26, "ULID must be 26 chars, got {}", id.len());
    // Crockford Base32: digits + uppercase letters EXCEPT I, L, O, U.
    for c in id.chars() {
        assert!(
            c.is_ascii_alphanumeric() && !c.is_ascii_lowercase(),
            "non-Crockford char in ULID: {c:?} (id={id})"
        );
        assert!(
            !matches!(c, 'I' | 'L' | 'O' | 'U'),
            "Crockford-excluded char in ULID: {c:?} (id={id})"
        );
    }
}

#[test]
fn list_runs_for_strategy_orders_by_started_at_asc() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    // Use the test-only helper to plant deterministic timestamps so the
    // ordering assertion does not depend on now_rfc3339's seconds granularity.
    let id_b = store
        .__test_insert_run_with_time(
            &strategy_id,
            RunStatus::Queued,
            "2026-04-27T00:00:02Z",
        )
        .unwrap();
    let id_a = store
        .__test_insert_run_with_time(
            &strategy_id,
            RunStatus::Queued,
            "2026-04-27T00:00:01Z",
        )
        .unwrap();
    let id_c = store
        .__test_insert_run_with_time(
            &strategy_id,
            RunStatus::Queued,
            "2026-04-27T00:00:03Z",
        )
        .unwrap();

    let rows = store.list_runs_for_strategy(&strategy_id).unwrap();
    assert_eq!(rows.len(), 3);
    let ordered_ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ordered_ids,
        vec![id_a.as_str(), id_b.as_str(), id_c.as_str()],
        "rows must be ordered by started_at ASC"
    );
    // Sanity — started_at values monotonically non-decreasing.
    for w in rows.windows(2) {
        assert!(
            w[0].started_at <= w[1].started_at,
            "started_at not ASC: {:?} > {:?}",
            w[0].started_at,
            w[1].started_at
        );
    }
}

#[test]
fn list_runs_for_strategy_excludes_other_strategy_runs() {
    let mut store = fresh_memory_store();
    let sid_a = seed_strategy(&mut store, "alpha", "// alpha");
    let sid_b = seed_strategy(&mut store, "beta", "// beta");

    let id_a = store.insert_run(&sid_a, RunStatus::Queued).unwrap();
    let _id_b = store.insert_run(&sid_b, RunStatus::Queued).unwrap();

    let only_a = store.list_runs_for_strategy(&sid_a).unwrap();
    assert_eq!(only_a.len(), 1, "should see only strategy A's run");
    assert_eq!(only_a[0].id, id_a);
    assert_eq!(only_a[0].strategy_id, sid_a);
}

#[test]
fn update_run_status_rejects_reserved_variant() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    let run_id = store.insert_run(&strategy_id, RunStatus::Queued).unwrap();

    // Phase 5 D-10: only Canceled stays Phase-6-reserved.
    let err = store
        .update_run_status(&run_id, RunStatus::Canceled)
        .expect_err("Canceled stays reserved beyond Phase 5");
    match err {
        StateError::InvalidInput(msg) => {
            assert!(
                msg.to_lowercase().contains("reserved"),
                "message should mention reserved: {msg}"
            );
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }

    // Status in DB must be unchanged (still Queued) — gate must run BEFORE the
    // UPDATE statement.
    let row = store.get_run(&run_id).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::Queued);
    assert!(row.finished_at.is_none());
}

#[test]
fn run_fk_enforced_by_foreign_keys_pragma() {
    let mut store = fresh_memory_store();
    let err = store
        .insert_run("nonexistent_strategy_id", RunStatus::Queued)
        .expect_err("FK violation");
    match err {
        StateError::Storage(msg) => {
            assert!(
                msg.to_uppercase().contains("FOREIGN KEY"),
                "expected FK message, got: {msg}"
            );
        }
        other => panic!("expected Storage(FK), got {other:?}"),
    }
}
