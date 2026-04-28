//! Phase 5 D-09 / MR-04 carry-forward — `journal_decisions.seq` is a per-run
//! monotonic counter assigned at INSERT, with `UNIQUE (run_id, seq)` as a
//! schema-level backstop. Mirrors `journal_source_read_seq.rs`.

mod common;

use common::fresh_memory_store;
use executor_core::schema::execution::RunStatus;
use executor_state::{DecisionGate, DecisionVerdict, RegisterOutcome, StateStore};
use serde_json::json;

fn fresh_run(store: &mut StateStore, name: &str) -> (String, String) {
    let outcome = store
        .register_strategy(name, &format!("// {name}"), None, None)
        .unwrap();
    let sid = match outcome {
        RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
    };
    let rid = store.insert_run(&sid, RunStatus::Queued).unwrap();
    (sid, rid)
}

#[test]
fn record_decision_assigns_monotonic_seq_within_run() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "decision-seq-1");
    s.record_decision(&rid, 0, DecisionGate::Policy, DecisionVerdict::Pass, None, None, None)
        .unwrap();
    s.record_decision(&rid, 0, DecisionGate::Simulation, DecisionVerdict::Pass, None, None, None)
        .unwrap();
    s.record_decision(&rid, 1, DecisionGate::Policy, DecisionVerdict::Pass, None, None, None)
        .unwrap();
    let rows = s.list_decisions_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].seq, 0);
    assert_eq!(rows[1].seq, 1);
    assert_eq!(rows[2].seq, 2);
}

#[test]
fn list_decisions_orders_by_recorded_at_then_seq() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "decision-seq-2");
    let t = "2026-04-27T00:00:00.000Z";
    s.__test_record_decision_with_time(
        &rid,
        0,
        DecisionGate::Policy,
        DecisionVerdict::Pass,
        None,
        None,
        None,
        t,
    )
    .unwrap();
    s.__test_record_decision_with_time(
        &rid,
        1,
        DecisionGate::Policy,
        DecisionVerdict::Pass,
        None,
        None,
        None,
        t,
    )
    .unwrap();
    s.__test_record_decision_with_time(
        &rid,
        2,
        DecisionGate::Simulation,
        DecisionVerdict::Pass,
        None,
        None,
        None,
        t,
    )
    .unwrap();
    let rows = s.list_decisions_for_run(&rid).unwrap();
    let idxs: Vec<i64> = rows.iter().map(|r| r.action_index).collect();
    assert_eq!(idxs, vec![0, 1, 2], "tie-break on per-run seq");
    let seqs: Vec<i64> = rows.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2]);
}

#[test]
fn seq_is_per_run_not_global() {
    let mut s = fresh_memory_store();
    let (_sid1, rid1) = fresh_run(&mut s, "decision-seq-3a");
    let (_sid2, rid2) = fresh_run(&mut s, "decision-seq-3b");
    s.record_decision(&rid1, 0, DecisionGate::Policy, DecisionVerdict::Pass, None, None, None)
        .unwrap();
    s.record_decision(&rid2, 0, DecisionGate::Policy, DecisionVerdict::Pass, None, None, None)
        .unwrap();
    let r1 = s.list_decisions_for_run(&rid1).unwrap();
    let r2 = s.list_decisions_for_run(&rid2).unwrap();
    assert_eq!(r1[0].seq, 0);
    assert_eq!(r2[0].seq, 0, "per-run not global counter");
}

#[test]
fn record_decision_persists_payload_round_trip() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "decision-seq-4");
    let payload = json!({"action_kind":"erc20_transfer","selector":"0xa9059cbb"});
    s.record_decision(
        &rid,
        0,
        DecisionGate::Policy,
        DecisionVerdict::Pass,
        None,
        None,
        Some(&payload),
    )
    .unwrap();
    let rows = s.list_decisions_for_run(&rid).unwrap();
    let stored: serde_json::Value =
        serde_json::from_str(rows[0].payload_json.as_ref().unwrap()).unwrap();
    assert_eq!(stored, payload);
}

#[test]
fn record_decision_fail_carries_rule_and_detail() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "decision-seq-5");
    s.record_decision(
        &rid,
        2,
        DecisionGate::Policy,
        DecisionVerdict::Fail,
        Some("chain_not_allowed"),
        Some("chain 999 not in allowlist"),
        None,
    )
    .unwrap();
    let rows = s.list_decisions_for_run(&rid).unwrap();
    assert_eq!(rows[0].verdict, "fail");
    assert_eq!(rows[0].gate, "policy");
    assert_eq!(rows[0].rule.as_deref(), Some("chain_not_allowed"));
    assert_eq!(
        rows[0].detail.as_deref(),
        Some("chain 999 not in allowlist")
    );
    assert_eq!(rows[0].action_index, 2);
}

#[test]
fn record_decision_skipped_verdict_emits_string() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "decision-seq-6");
    s.record_decision(
        &rid,
        1,
        DecisionGate::Simulation,
        DecisionVerdict::Skipped,
        None,
        None,
        None,
    )
    .unwrap();
    let rows = s.list_decisions_for_run(&rid).unwrap();
    assert_eq!(rows[0].verdict, "skipped");
    assert_eq!(rows[0].gate, "simulation");
}
