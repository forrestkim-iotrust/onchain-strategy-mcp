//! Journal repository contract tests (D-06): journal_source_reads /
//! journal_actions / journal_logs CRUD + ordering + FK + phase3_emittable gate.

mod common;

use common::fresh_memory_store;
use executor_core::schema::execution::{JournalActionOutcome, RunStatus};
use executor_state::{RegisterOutcome, StateError};

fn seed_strategy(store: &mut executor_state::StateStore, name: &str, source: &str) -> String {
    match store.register_strategy(name, source, None, None).unwrap() {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
    }
}

fn fresh_run(store: &mut executor_state::StateStore, name: &str) -> (String, String) {
    let sid = seed_strategy(store, name, &format!("// {name}"));
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();
    (sid, rid)
}

#[test]
fn record_source_read_inserts_row() {
    let mut store = fresh_memory_store();
    let (sid, rid) = fresh_run(&mut store, "src1");

    let id = store
        .record_source_read(&rid, "strategy_source", &sid, None)
        .expect("record_source_read");
    assert_eq!(id.len(), 26, "ULID is 26 chars");

    let rows = store.list_source_reads_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "strategy_source");
    assert_eq!(rows[0].target, sid);
    assert_eq!(rows[0].payload_json, None);
    assert_eq!(rows[0].run_id, rid);
}

#[test]
fn record_source_read_supports_phase4_kinds() {
    let mut store = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut store, "src2");

    let payload = "{\"address\":\"0xdeadbeef\"}";
    store
        .record_source_read(&rid, "evm_call", "0xdeadbeef", Some(payload))
        .unwrap();

    let rows = store.list_source_reads_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "evm_call");
    assert_eq!(rows[0].target, "0xdeadbeef");
    assert_eq!(rows[0].payload_json.as_deref(), Some(payload));
}

#[test]
fn record_source_read_rejects_orphan_run_id() {
    let mut store = fresh_memory_store();

    let err = store
        .record_source_read("01ZZZZZZZZZZZZZZZZZZZZZZZZ", "strategy_source", "abc", None)
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

#[test]
fn record_action_outcome_inserts_row_for_each_phase3_emittable_variant() {
    let mut store = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut store, "act1");

    let variants = [
        JournalActionOutcome::Noop,
        JournalActionOutcome::Actions,
        JournalActionOutcome::ValidationError,
        JournalActionOutcome::RuntimeError,
    ];
    for v in variants {
        store.record_action_outcome(&rid, v, "{}").unwrap();
    }

    let rows = store.list_actions_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 4);
    let outcomes: Vec<_> = rows.iter().map(|r| r.outcome).collect();
    for v in variants {
        assert!(outcomes.contains(&v), "missing outcome {v:?}");
    }
}

#[test]
fn record_action_outcome_accepts_phase5_terminal_outcomes() {
    // Phase 5 D-10: `phase3_emittable → phase5_emittable` widens to allow
    // SimulationFailure + PolicyDenied. The Phase-3-era reservation test is
    // inverted: the gate now ACCEPTS both terminal outcomes.
    let mut store = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut store, "act2");

    for outcome in [
        JournalActionOutcome::SimulationFailure,
        JournalActionOutcome::PolicyDenied,
    ] {
        store
            .record_action_outcome(&rid, outcome, "{}")
            .expect("Phase 5 widens phase5_emittable to allow this terminal outcome");
    }

    let rows = store.list_actions_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 2);
    let outcomes: Vec<_> = rows.iter().map(|r| r.outcome).collect();
    assert!(outcomes.contains(&JournalActionOutcome::SimulationFailure));
    assert!(outcomes.contains(&JournalActionOutcome::PolicyDenied));
}

#[test]
fn record_log_inserts_row_with_ulid_and_rfc3339() {
    let mut store = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut store, "log1");

    let id = store.record_log(&rid, "hello").unwrap();
    assert_eq!(id.len(), 26);
    for c in id.chars() {
        assert!(
            c.is_ascii_alphanumeric() && !c.is_ascii_lowercase(),
            "non-Crockford char in ULID: {c:?}"
        );
        assert!(!matches!(c, 'I' | 'L' | 'O' | 'U'));
    }

    let rows = store.list_logs_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].message, "hello");
    // RFC3339 — chrono parses it.
    chrono::DateTime::parse_from_rfc3339(&rows[0].recorded_at).expect("rfc3339");
}

#[test]
fn list_logs_for_run_returns_insertion_order() {
    let mut store = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut store, "log2");

    store
        .__test_record_log_with_time(&rid, "a", "2026-04-27T00:00:01Z")
        .unwrap();
    store
        .__test_record_log_with_time(&rid, "b", "2026-04-27T00:00:02Z")
        .unwrap();
    store
        .__test_record_log_with_time(&rid, "c", "2026-04-27T00:00:03Z")
        .unwrap();

    let rows = store.list_logs_for_run(&rid).unwrap();
    let msgs: Vec<&str> = rows.iter().map(|r| r.message.as_str()).collect();
    assert_eq!(msgs, vec!["a", "b", "c"]);
}

#[test]
fn list_actions_for_run_excludes_other_runs() {
    let mut store = fresh_memory_store();
    let sid_a = seed_strategy(&mut store, "alpha", "// alpha");
    let sid_b = seed_strategy(&mut store, "beta", "// beta");
    let rid_a = store.insert_run(&sid_a, RunStatus::Queued).unwrap();
    let rid_b = store.insert_run(&sid_b, RunStatus::Queued).unwrap();

    store
        .record_action_outcome(&rid_a, JournalActionOutcome::Noop, "{}")
        .unwrap();
    store
        .record_action_outcome(&rid_b, JournalActionOutcome::Actions, "{}")
        .unwrap();

    let only_a = store.list_actions_for_run(&rid_a).unwrap();
    assert_eq!(only_a.len(), 1);
    assert_eq!(only_a[0].outcome, JournalActionOutcome::Noop);
    assert_eq!(only_a[0].run_id, rid_a);
}
