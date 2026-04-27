//! Run repository contract tests (D-04b, D-05a, D-05c).

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
fn phase2_emittable_rejects_reserved_variants() {
    let mut store = fresh_memory_store();
    let strategy_id = seed_strategy(&mut store, "arb", "// code");

    for reserved in [
        RunStatus::Canceled,
        RunStatus::SimulationDenied,
        RunStatus::PolicyDenied,
    ] {
        let err = store
            .insert_run(&strategy_id, reserved)
            .expect_err("reserved variant must be rejected");
        match err {
            StateError::InvalidInput(msg) => {
                assert!(
                    msg.to_lowercase().contains("reserved"),
                    "message should mention reserved: {msg}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    // Allowed ones succeed.
    for ok in [
        RunStatus::Queued,
        RunStatus::Running,
        RunStatus::Succeeded,
        RunStatus::Failed,
    ] {
        store.insert_run(&strategy_id, ok).expect("phase2 status ok");
    }
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
