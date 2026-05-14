//! v1.5 Track 1A — policy_revisions storage tests.
//!
//! Covers:
//! - First `set_active_policy` produces an active row with no prior active.
//! - Second call deactivates the prior row and replaces it.
//! - `get_active_policy` returns the most-recently-set revision.
//! - `list_policy_revisions` returns newest-first.
//! - Schema invariant: at any time at most one row has `is_active = 1`
//!   (enforced by the partial unique index + the wrapping transaction).

use executor_state::StateStore;
use tempfile::TempDir;

fn open_store() -> (TempDir, StateStore) {
    let tmp = TempDir::new().expect("tmp");
    let store = StateStore::open(&tmp.path().join("state.db")).expect("open store");
    (tmp, store)
}

fn count_active(store: &StateStore) -> i64 {
    executor_state::policy_revisions::__test_count_active(store.__test_conn()).unwrap()
}

#[test]
fn first_set_creates_active_row() {
    let (_tmp, mut store) = open_store();
    assert!(store.get_active_policy().unwrap().is_none());

    let rev = store
        .set_active_policy(r#"{"chains":{"allow":[31337]}}"#, Some("first"))
        .expect("set first");

    assert!(!rev.revision_id.is_empty());
    assert_eq!(rev.rationale.as_deref(), Some("first"));
    assert!(!rev.set_at.is_empty());
    assert_eq!(count_active(&store), 1);

    let got = store.get_active_policy().unwrap().unwrap();
    assert_eq!(got.revision_id, rev.revision_id);
    assert_eq!(got.body_json, r#"{"chains":{"allow":[31337]}}"#);
}

#[test]
fn second_set_replaces_first() {
    let (_tmp, mut store) = open_store();
    let r1 = store
        .set_active_policy(r#"{"chains":{"allow":[31337]}}"#, Some("first"))
        .expect("set first");
    let r2 = store
        .set_active_policy(r#"{"chains":{"allow":[8453]}}"#, Some("second"))
        .expect("set second");

    assert_ne!(r1.revision_id, r2.revision_id);
    assert_eq!(count_active(&store), 1, "exactly one row remains active");

    let got = store.get_active_policy().unwrap().unwrap();
    assert_eq!(got.revision_id, r2.revision_id);
    assert_eq!(got.body_json, r#"{"chains":{"allow":[8453]}}"#);
}

#[test]
fn list_revisions_returns_full_history_and_marks_active() {
    let (_tmp, mut store) = open_store();
    // Adjacent set_active calls within the same millisecond collide on
    // `set_at`, so we can't rely on insertion order for the tie-break
    // (ULID random suffix is not monotonic). What we CAN assert is that
    // (a) all three revisions show up in the listing, (b) exactly one is
    // marked active, and (c) the active one is the most-recently-set.
    let r1 = store
        .set_active_policy(r#"{"chains":{"allow":[1]}}"#, Some("a"))
        .unwrap();
    let r2 = store
        .set_active_policy(r#"{"chains":{"allow":[2]}}"#, Some("b"))
        .unwrap();
    let r3 = store
        .set_active_policy(r#"{"chains":{"allow":[3]}}"#, Some("c"))
        .unwrap();

    let list = store.list_policy_revisions(10).unwrap();
    assert_eq!(list.len(), 3);

    let active_rows: Vec<&_> = list.iter().filter(|r| r.is_active).collect();
    assert_eq!(active_rows.len(), 1, "exactly one row is active");
    assert_eq!(active_rows[0].revision_id, r3.revision_id);

    let ids: Vec<&str> = list.iter().map(|r| r.revision_id.as_str()).collect();
    assert!(ids.contains(&r1.revision_id.as_str()));
    assert!(ids.contains(&r2.revision_id.as_str()));
    assert!(ids.contains(&r3.revision_id.as_str()));

    // Ordering: set_at is the primary key; with ties on millis the
    // ordering is by revision_id DESC. Regardless of which iteration the
    // listing comes back in, set_at MUST be non-increasing.
    for w in list.windows(2) {
        assert!(
            w[0].set_at >= w[1].set_at,
            "list_revisions ordering: set_at must be non-increasing — got {} then {}",
            w[0].set_at,
            w[1].set_at,
        );
    }
}

#[test]
fn list_revisions_returns_distinct_timestamps_newest_first() {
    let (_tmp, mut store) = open_store();
    // Force distinct millisecond timestamps so the newest-first
    // assertion is robust against same-ms collisions.
    let r1 = store
        .set_active_policy(r#"{"v":1}"#, Some("a"))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let r2 = store
        .set_active_policy(r#"{"v":2}"#, Some("b"))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let r3 = store
        .set_active_policy(r#"{"v":3}"#, Some("c"))
        .unwrap();

    let list = store.list_policy_revisions(10).unwrap();
    assert_eq!(list[0].revision_id, r3.revision_id);
    assert_eq!(list[1].revision_id, r2.revision_id);
    assert_eq!(list[2].revision_id, r1.revision_id);
    assert!(list[0].is_active);
}

#[test]
fn list_revisions_respects_cap() {
    let (_tmp, mut store) = open_store();
    for i in 0..5u32 {
        store
            .set_active_policy(&format!(r#"{{"chains":{{"allow":[{i}]}}}}"#), None)
            .unwrap();
    }
    // Cap at 200 is enforced inside the helper even when caller asks for huge n.
    let list = store.list_policy_revisions(99_999).unwrap();
    assert_eq!(list.len(), 5);

    let list = store.list_policy_revisions(2).unwrap();
    assert_eq!(list.len(), 2, "limit honoured");
}

#[test]
fn at_most_one_active_row_invariant() {
    // The partial unique index on (is_active) WHERE is_active = 1 prevents
    // two active rows from ever coexisting. Even if a regression in
    // set_active forgot to deactivate the prior row, the second INSERT
    // would fail at the constraint level — not silently corrupt the table.
    let (_tmp, mut store) = open_store();
    for i in 0..3u32 {
        store
            .set_active_policy(&format!(r#"{{"v":{i}}}"#), Some("iter"))
            .unwrap();
        assert_eq!(
            count_active(&store),
            1,
            "after iter {i}: active row count drifted from 1",
        );
    }
}

#[test]
fn get_active_returns_none_when_empty() {
    let (_tmp, store) = open_store();
    assert!(store.get_active_policy().unwrap().is_none());
    let list = store.list_policy_revisions(20).unwrap();
    assert!(list.is_empty());
}
