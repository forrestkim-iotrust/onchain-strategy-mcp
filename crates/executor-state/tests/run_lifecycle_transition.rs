//! D-12 transition-guarded run-status update tests (closes 02-REVIEW MR-01).

mod common;

use common::fresh_memory_store;
use executor_core::schema::execution::RunStatus;
use executor_state::{RegisterOutcome, StateError};

fn seed_strategy(store: &mut executor_state::StateStore, name: &str, source: &str) -> String {
    match store.register_strategy(name, source, None, None).unwrap() {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    }
}

#[test]
fn update_run_status_with_transition_advances_queued_to_running() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();

    store
        .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Running)
        .expect("queued→running ok");

    let row = store.get_run(&rid).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::Running);
    assert!(
        row.finished_at.is_none(),
        "Running must not set finished_at"
    );
}

#[test]
fn update_run_status_with_transition_rejects_unexpected_from() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();

    // Row is Queued, caller asserts Running → reject.
    let err = store
        .update_run_status_with_transition(&rid, RunStatus::Running, RunStatus::Succeeded)
        .expect_err("transition mismatch must error");
    match err {
        StateError::InvalidInput(msg) => {
            let lower = msg.to_lowercase();
            assert!(
                lower.contains("not in expected state") || lower.contains("running"),
                "message should mention the from-state mismatch: {msg}"
            );
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }

    // Row must NOT have mutated.
    let row = store.get_run(&rid).unwrap().unwrap();
    assert_eq!(
        row.status,
        RunStatus::Queued,
        "row must not be mutated on transition reject"
    );
}

#[test]
fn update_run_status_with_transition_rejects_phase5_reserved_target() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();

    let err = store
        .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Canceled)
        .expect_err("reserved target must be rejected");
    match err {
        StateError::InvalidInput(msg) => {
            let lower = msg.to_lowercase();
            assert!(
                lower.contains("reserved")
                    || lower.contains("phase 5")
                    || lower.contains("phase 6"),
                "message should mention reserved: {msg}"
            );
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn update_run_status_with_transition_rejects_missing_run() {
    let mut store = fresh_memory_store();
    let _sid = seed_strategy(&mut store, "arb", "// code");

    let err = store
        .update_run_status_with_transition(
            "01ZZZZZZZZZZZZZZZZZZZZZZZZ",
            RunStatus::Queued,
            RunStatus::Running,
        )
        .expect_err("missing run must error");
    match err {
        StateError::NotFound(msg) => {
            assert!(
                msg.contains("01ZZZZZZZZZZZZZZZZZZZZZZZZ"),
                "NotFound should reference the run id: {msg}"
            );
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn update_run_status_with_transition_sets_finished_at_on_succeeded() {
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();

    store
        .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Running)
        .unwrap();
    store
        .update_run_status_with_transition(&rid, RunStatus::Running, RunStatus::Succeeded)
        .unwrap();

    let row = store.get_run(&rid).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::Succeeded);
    assert!(
        row.finished_at.is_some(),
        "Succeeded must populate finished_at"
    );
}

#[test]
fn update_run_status_with_transition_accepts_running_to_simulation_denied() {
    // Phase 5 D-10: Running → SimulationDenied is a legal terminal transition.
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();
    store
        .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Running)
        .unwrap();
    store
        .update_run_status_with_transition(&rid, RunStatus::Running, RunStatus::SimulationDenied)
        .expect("Phase 5 unblocks Running → SimulationDenied");
    let row = store.get_run(&rid).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::SimulationDenied);
    assert!(
        row.finished_at.is_some(),
        "SimulationDenied must populate finished_at"
    );
}

#[test]
fn update_run_status_with_transition_accepts_running_to_policy_denied() {
    // Phase 5 D-10: Running → PolicyDenied is a legal terminal transition.
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();
    store
        .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Running)
        .unwrap();
    store
        .update_run_status_with_transition(&rid, RunStatus::Running, RunStatus::PolicyDenied)
        .expect("Phase 5 unblocks Running → PolicyDenied");
    let row = store.get_run(&rid).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::PolicyDenied);
    assert!(
        row.finished_at.is_some(),
        "PolicyDenied must populate finished_at"
    );
}

#[test]
fn update_run_status_with_transition_rejects_phase5_terminal_denial_transitions() {
    for (terminal, target) in [
        (RunStatus::SimulationDenied, RunStatus::SimulationDenied),
        (RunStatus::SimulationDenied, RunStatus::Running),
        (RunStatus::PolicyDenied, RunStatus::PolicyDenied),
        (RunStatus::PolicyDenied, RunStatus::Running),
    ] {
        let mut store = fresh_memory_store();
        let sid = seed_strategy(&mut store, "terminal", "// code");
        let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();
        store
            .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Running)
            .unwrap();
        store
            .update_run_status_with_transition(&rid, RunStatus::Running, terminal)
            .unwrap();

        let err = store
            .update_run_status_with_transition(&rid, terminal, target)
            .expect_err("terminal denial transition must be rejected");
        match err {
            StateError::InvalidInput(msg) => assert!(
                msg.contains("terminal state"),
                "expected terminal-state rejection, got {msg}"
            ),
            other => panic!("expected InvalidInput, got {other:?}"),
        }

        let row = store.get_run(&rid).unwrap().unwrap();
        assert_eq!(row.status, terminal);
        assert!(
            row.finished_at.is_some(),
            "terminal denial finished_at must survive rejected transition"
        );
    }
}

#[test]
fn update_run_status_with_transition_does_not_overwrite_finished_at_on_re_succeed() {
    // STRICT D-12: Succeeded → * is Disallowed (terminal). The guard MUST
    // reject Succeeded → Succeeded with InvalidInput.
    let mut store = fresh_memory_store();
    let sid = seed_strategy(&mut store, "arb", "// code");
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();

    store
        .update_run_status_with_transition(&rid, RunStatus::Queued, RunStatus::Running)
        .unwrap();
    store
        .update_run_status_with_transition(&rid, RunStatus::Running, RunStatus::Succeeded)
        .unwrap();

    let after_succeed = store.get_run(&rid).unwrap().unwrap();
    let original_finished_at = after_succeed
        .finished_at
        .clone()
        .expect("finished_at populated");

    // Attempt the disallowed self-transition.
    let err = store
        .update_run_status_with_transition(&rid, RunStatus::Succeeded, RunStatus::Succeeded)
        .expect_err("Succeeded → Succeeded must be rejected (D-12 terminal)");
    match err {
        StateError::InvalidInput(_) => {}
        other => panic!("expected InvalidInput for terminal self-transition, got {other:?}"),
    }

    // finished_at must be unchanged.
    let row = store.get_run(&rid).unwrap().unwrap();
    assert_eq!(row.status, RunStatus::Succeeded);
    assert_eq!(
        row.finished_at,
        Some(original_finished_at),
        "finished_at must not change after a rejected re-succeed"
    );
}
