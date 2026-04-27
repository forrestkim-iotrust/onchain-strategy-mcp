//! Phase 4 D-15d / MR-04 carry-forward — `journal_source_reads.seq` is a
//! per-run monotonic counter assigned at INSERT, with `UNIQUE (run_id, seq)`
//! as a schema-level backstop.

mod common;

use common::fresh_memory_store;
use executor_core::schema::execution::RunStatus;
use executor_state::RegisterOutcome;

fn fresh_run(store: &mut executor_state::StateStore, name: &str) -> (String, String) {
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
fn record_source_read_assigns_monotonic_seq_within_run() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "evm-seq-1");

    s.record_source_read(&rid, "evm_read", "0xaaa:foo", None)
        .unwrap();
    s.record_source_read(&rid, "evm_read", "0xbbb:bar", None)
        .unwrap();
    s.record_source_read(&rid, "evm_read", "0xccc:baz", None)
        .unwrap();

    let rows = s.list_source_reads_for_run(&rid).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].seq, 0);
    assert_eq!(rows[1].seq, 1);
    assert_eq!(rows[2].seq, 2);
}

#[test]
fn list_source_reads_orders_by_recorded_at_then_seq() {
    let mut s = fresh_memory_store();
    let (_sid, rid) = fresh_run(&mut s, "evm-seq-2");

    // Two same-instant inserts — without `seq` tie-break, ULID id ordering
    // would not necessarily reflect insertion order.
    let t = "2026-04-27T00:00:00Z";
    s.__test_record_source_read_with_time(&rid, "evm_read", "0xa:f", None, t)
        .unwrap();
    s.__test_record_source_read_with_time(&rid, "evm_read", "0xb:g", None, t)
        .unwrap();
    s.__test_record_source_read_with_time(&rid, "evm_read", "0xc:h", None, t)
        .unwrap();

    let rows = s.list_source_reads_for_run(&rid).unwrap();
    let targets: Vec<&str> = rows.iter().map(|r| r.target.as_str()).collect();
    assert_eq!(targets, vec!["0xa:f", "0xb:g", "0xc:h"]);
    let seqs: Vec<i64> = rows.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2]);
}

#[test]
fn seq_is_per_run_not_global() {
    let mut s = fresh_memory_store();
    let (_sid_a, rid_a) = fresh_run(&mut s, "alpha-evm");
    let (_sid_b, rid_b) = fresh_run(&mut s, "beta-evm");

    s.record_source_read(&rid_a, "evm_read", "0xa:f", None)
        .unwrap();
    s.record_source_read(&rid_a, "evm_read", "0xa:g", None)
        .unwrap();
    s.record_source_read(&rid_b, "evm_read", "0xb:f", None)
        .unwrap();

    let rows_a = s.list_source_reads_for_run(&rid_a).unwrap();
    let rows_b = s.list_source_reads_for_run(&rid_b).unwrap();
    assert_eq!(rows_a.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(rows_b.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![0]);
}
