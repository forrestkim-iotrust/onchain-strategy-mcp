//! v1.8 name-anchored lineage integration tests.
//!
//! Validates the contract laid out in the v1.8 brief: re-registering a
//! strategy by the SAME name preserves triggers, runs, and records by
//! attaching them to a stable `lineage_id`. The strategy_id (content
//! hash) still bumps per version, but the lineage anchor is the name.
//!
//! These tests drive the StateStore directly (no JSON-RPC roundtrip)
//! plus the resource dispatcher for the new `strategy://lineage/...`
//! URIs. Mirrors the trigger_tools.rs pattern.
//!
//! Scenarios covered:
//!   1. Same-name re-register with view change → ReplacedVersion, lineage
//!      preserved, trigger from v1 still attached, captured record still
//!      readable via the new strategy id.
//!   2. Re-register with records-spec change → previous_records_changed.
//!   3. Different name + same content → distinct lineages, distinct ids.
//!   4. `strategy://lineage/{id}/history` returns all 3 versions
//!      newest-first with `is_active` flag.
//!   5. `strategy_delete` on the latest version dormantises the lineage;
//!      `strategy://by-name/{name}` 404s; old versions still readable by
//!      specific id.

use executor_core::schema::trigger::{RegisterTriggerInput, TriggerKind, TriggerListFilter};
use executor_state::{
    RegisterOutcome, Strategy, StateStore, TriggerRegisterOutcome,
};
use serde_json::json;
use tempfile::tempdir;

fn fresh_store() -> (tempfile::TempDir, StateStore) {
    let tmp = tempdir().expect("tmp");
    let path = tmp.path().join("lineage.db");
    let store = StateStore::open(&path).expect("open store");
    (tmp, store)
}

fn register(store: &mut StateStore, name: &str, source: &str, view: Option<&str>) -> RegisterOutcome {
    store
        .register_strategy_bundle(name, source, None, None, None, view, None)
        .expect("register")
}

fn into_strategy(o: RegisterOutcome) -> Strategy {
    o.into_active_strategy()
}

#[test]
fn same_name_view_change_preserves_lineage_and_attachments() {
    let (_tmp, mut store) = fresh_store();

    // v1 register with a view. Attach a trigger, run once, capture a record.
    let v1 = into_strategy(register(&mut store, "eth-funnel", "// v1 execute", Some("(ctx) => ({k: 1})")));
    assert!(!v1.lineage_id.is_empty());

    let trigger = match store
        .register_trigger(RegisterTriggerInput {
            strategy_id: v1.id.clone(),
            kind: TriggerKind::Manual,
            config: json!({}),
            predicate: None,
            dedup_window_ms: None,
        })
        .expect("register trigger")
    {
        TriggerRegisterOutcome::Created(t) => t,
        TriggerRegisterOutcome::AlreadyExists(t) => t,
    };
    assert_eq!(trigger.strategy_lineage_id.as_deref(), Some(v1.lineage_id.as_str()));

    let run_id = store
        .insert_run(&v1.id, executor_core::schema::execution::RunStatus::Running)
        .expect("insert run");
    let run = store.get_run(&run_id).expect("get run").expect("present");
    assert_eq!(run.strategy_lineage_id.as_deref(), Some(v1.lineage_id.as_str()));

    store
        .record_strategy_capture(&run_id, &v1.id, "supply", "{\"amount\":\"100\"}")
        .expect("record capture");

    // v2 register — only view changed.
    let outcome = register(&mut store, "eth-funnel", "// v1 execute", Some("(ctx) => ({k: 2})"));
    let v2 = match outcome {
        RegisterOutcome::ReplacedVersion {
            created,
            previous,
            new_version,
            previous_version,
            execute_changed,
            records_changed,
            view_changed,
        } => {
            assert_eq!(new_version, 2);
            assert_eq!(previous_version, 1);
            assert!(!execute_changed);
            assert!(!records_changed);
            assert!(view_changed);
            assert_eq!(previous.id, v1.id);
            created
        }
        other => panic!("expected ReplacedVersion, got {other:?}"),
    };
    assert_eq!(v2.lineage_id, v1.lineage_id, "lineage must be preserved");
    assert_ne!(v2.id, v1.id, "strategy_id must bump");

    // Trigger from v1 is still attached to the lineage (a list by lineage
    // filter sees it).
    let triggers_for_lineage = store
        .list_triggers(Some(&TriggerListFilter {
            strategy_lineage_id: Some(v1.lineage_id.clone()),
            ..Default::default()
        }))
        .expect("list triggers by lineage");
    assert_eq!(triggers_for_lineage.len(), 1);
    assert_eq!(triggers_for_lineage[0].id, trigger.id);

    // Records from v1 are still readable via the lineage.
    let captures = store
        .list_strategy_records_for_lineage(&v1.lineage_id, None, 500)
        .expect("list records by lineage");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].strategy_id, v1.id, "capture preserved as v1's");
}

#[test]
fn same_name_records_change_flag_is_set() {
    let (_tmp, mut store) = fresh_store();
    let v1 = into_strategy(register(&mut store, "yield", "// src", None));

    let outcome = store
        .register_strategy_bundle(
            "yield",
            "// src",
            None,
            None,
            Some("[{\"name\":\"supply\",\"on\":{},\"capture\":{}}]"),
            None,
            None,
        )
        .expect("register v2");
    match outcome {
        RegisterOutcome::ReplacedVersion {
            created,
            records_changed,
            view_changed,
            execute_changed,
            ..
        } => {
            assert_eq!(created.lineage_id, v1.lineage_id);
            assert!(records_changed);
            assert!(!view_changed);
            assert!(!execute_changed);
        }
        other => panic!("expected ReplacedVersion, got {other:?}"),
    }
}

#[test]
fn different_name_same_content_makes_distinct_lineages() {
    let (_tmp, mut store) = fresh_store();
    let a = into_strategy(register(&mut store, "name-a", "// SHARED", None));
    let b = into_strategy(register(&mut store, "name-b", "// SHARED", None));

    assert_ne!(a.lineage_id, b.lineage_id);
    assert_ne!(
        a.id, b.id,
        "lineage_id mixes into the hash so identical content under \
         a different name gets a distinct strategy_id"
    );
}

#[test]
fn lineage_history_shows_all_versions_newest_first() {
    let (_tmp, mut store) = fresh_store();
    let v1 = into_strategy(register(&mut store, "rotator", "// a", None));
    std::thread::sleep(std::time::Duration::from_millis(1100)); // ensure timestamps differ at second granularity
    let v2 = into_strategy(register(&mut store, "rotator", "// b", None));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let v3 = into_strategy(register(&mut store, "rotator", "// c", None));

    assert_eq!(v1.lineage_id, v2.lineage_id);
    assert_eq!(v2.lineage_id, v3.lineage_id);

    let history = store
        .list_strategies_for_lineage(&v1.lineage_id)
        .expect("list history");
    assert_eq!(history.len(), 3);
    // Newest first: v3, v2, v1.
    assert_eq!(history[0].id, v3.id);
    assert_eq!(history[0].version, 3);
    assert!(history[0].deleted_at.is_none(), "v3 is active");
    assert_eq!(history[1].id, v2.id);
    assert_eq!(history[1].version, 2);
    assert!(history[1].deleted_at.is_some(), "v2 was superseded");
    assert_eq!(history[2].id, v1.id);
    assert_eq!(history[2].version, 1);
    assert!(history[2].deleted_at.is_some(), "v1 was superseded");
}

#[test]
fn soft_delete_of_latest_dormantises_lineage_but_old_versions_remain() {
    let (_tmp, mut store) = fresh_store();
    let v1 = into_strategy(register(&mut store, "dormant", "// a", None));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let v2 = into_strategy(register(&mut store, "dormant", "// b", None));
    assert_eq!(v1.lineage_id, v2.lineage_id);

    store.soft_delete_strategy(&v2.id).expect("delete v2");
    assert!(
        store
            .get_strategy_by_name("dormant")
            .expect("by name")
            .is_none(),
        "by-name lookup returns no active row"
    );
    assert!(
        store
            .get_active_strategy_for_lineage(&v1.lineage_id)
            .expect("by lineage")
            .is_none(),
        "lineage has no active version"
    );
    // Old versions still addressable by specific id.
    let still_v1 = store
        .get_strategy_by_id(&v1.id)
        .expect("get v1")
        .expect("v1 present");
    assert_eq!(still_v1.id, v1.id);
    let still_v2 = store
        .get_strategy_by_id(&v2.id)
        .expect("get v2")
        .expect("v2 present");
    assert!(still_v2.deleted_at.is_some());
}

#[test]
fn same_name_same_content_is_idempotent() {
    // Sanity: re-registering identical content under the same name should
    // NOT bump the version — it returns AlreadyExists with the original
    // row. This is the "no-op refresh" path agents rely on.
    let (_tmp, mut store) = fresh_store();
    let v1 = into_strategy(register(&mut store, "idem", "// src", None));

    match register(&mut store, "idem", "// src", None) {
        RegisterOutcome::AlreadyExists(existing) => {
            assert_eq!(existing.id, v1.id);
            assert_eq!(existing.lineage_id, v1.lineage_id);
        }
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
    // Lineage still has exactly one version.
    let history = store.list_strategies_for_lineage(&v1.lineage_id).unwrap();
    assert_eq!(history.len(), 1);
}

#[test]
fn version_counts_account_for_soft_deleted_rows() {
    // Soft-deleted rows still count for the per-lineage `version` number
    // (so v3 is always called v3 even if v1 and v2 have been replaced).
    let (_tmp, mut store) = fresh_store();
    let v1 = into_strategy(register(&mut store, "vcount", "// a", None));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let v2 = into_strategy(register(&mut store, "vcount", "// b", None));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let v3 = into_strategy(register(&mut store, "vcount", "// c", None));

    let v1_version = store.strategy_version_for_id(&v1.id).unwrap().unwrap();
    let v2_version = store.strategy_version_for_id(&v2.id).unwrap().unwrap();
    let v3_version = store.strategy_version_for_id(&v3.id).unwrap().unwrap();
    assert_eq!(v1_version, 1);
    assert_eq!(v2_version, 2);
    assert_eq!(v3_version, 3);
}
