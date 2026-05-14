//! Strategy repository contract tests (D-01..D-02, D-07a..c, D-09 partial).

mod common;

use common::fresh_memory_store;
use executor_state::{RegisterOutcome, StateError, strategies::hash_source};

#[test]
fn hash_source_matches_fips_vectors() {
    assert_eq!(
        hash_source(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hash_source("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn register_then_get_by_id_roundtrip() {
    let mut store = fresh_memory_store();
    let tags = vec!["arb".to_string(), "usdc".to_string()];
    let outcome = store
        .register_strategy("arb", "// code", Some("desc"), Some(&tags))
        .expect("register");

    let created = match outcome {
        RegisterOutcome::Created(s) => s,
        _ => panic!("first register must be Created"),
    };
    // v1.8: fresh-lineage ids fold the minted ULID into the hash, so they
    // are not bit-identical to `hash_source(source)`. They are still
    // 64-char hex (sha256), and `lineage_id` is the per-name anchor.
    assert_eq!(created.id.len(), 64);
    assert!(!created.lineage_id.is_empty(), "fresh register mints a lineage_id");
    assert_eq!(created.name, "arb");
    assert_eq!(created.description.as_deref(), Some("desc"));
    assert_eq!(created.tags.as_deref(), Some(&tags[..]));

    let by_id = store
        .get_strategy_by_id(&created.id)
        .expect("get_by_id")
        .expect("present");
    assert_eq!(by_id.source, "// code");
    assert_eq!(by_id.tags.as_deref(), Some(&tags[..]));
    assert_eq!(by_id.lineage_id, created.lineage_id);

    let by_name = store
        .get_strategy_by_name("arb")
        .expect("get_by_name")
        .expect("present");
    assert_eq!(by_name.id, created.id);
}

/// v1.8: same-name same-content idempotency. The second register MUST
/// return AlreadyExists with the same id + lineage_id.
#[test]
fn register_idempotent_same_name_same_content() {
    let mut store = fresh_memory_store();
    let first_id = match store
        .register_strategy("arb", "src-A", Some("first-desc"), None)
        .expect("register first")
    {
        RegisterOutcome::Created(s) => s.id,
        other => panic!("first register must be Created, got {other:?}"),
    };

    let second = store
        .register_strategy("arb", "src-A", Some("ignored"), None)
        .expect("register same name same content");
    let existing = match second {
        RegisterOutcome::AlreadyExists(s) => s,
        other => panic!("second register must be AlreadyExists, got {other:?}"),
    };
    assert_eq!(existing.id, first_id);
    assert_eq!(existing.description.as_deref(), Some("first-desc"));
}

/// v1.8: different-name same-content registers a NEW lineage with a new id
/// (the lineage_id folds into the hash so byte-identical content under a
/// different name gets a distinct row).
#[test]
fn register_different_name_same_content_makes_new_lineage() {
    let mut store = fresh_memory_store();
    let first = match store
        .register_strategy("arb", "src-A", None, None)
        .expect("register first")
    {
        RegisterOutcome::Created(s) => s,
        other => panic!("expected Created, got {other:?}"),
    };

    let second = match store
        .register_strategy("arb-copy", "src-A", None, None)
        .expect("register second")
    {
        RegisterOutcome::Created(s) => s,
        other => panic!("expected Created (new lineage), got {other:?}"),
    };
    assert_ne!(first.id, second.id, "distinct lineages must yield distinct ids");
    assert_ne!(
        first.lineage_id, second.lineage_id,
        "different names mint different lineage_ids"
    );
}

/// v1.8: same-name different-content soft-deletes the old row and inserts
/// a new version under the SAME lineage_id (ReplacedVersion outcome).
#[test]
fn register_same_name_different_content_replaces_version() {
    let mut store = fresh_memory_store();
    let v1 = match store
        .register_strategy("arb", "src-A", None, None)
        .expect("register v1")
    {
        RegisterOutcome::Created(s) => s,
        other => panic!("v1 must be Created, got {other:?}"),
    };

    let outcome = store
        .register_strategy("arb", "src-B", None, None)
        .expect("register v2");
    match outcome {
        RegisterOutcome::ReplacedVersion {
            created,
            previous,
            new_version,
            previous_version,
            execute_changed,
            records_changed,
            view_changed,
        } => {
            assert_eq!(created.lineage_id, v1.lineage_id);
            assert_eq!(previous.id, v1.id);
            assert!(previous.deleted_at.is_some(), "old version must be soft-deleted");
            assert_eq!(new_version, 2);
            assert_eq!(previous_version, 1);
            assert!(execute_changed);
            assert!(!records_changed);
            assert!(!view_changed);
        }
        other => panic!("expected ReplacedVersion, got {other:?}"),
    }
}

// Quieten the unused-import warning for hash_source / StateError now that
// the old NameConflict assertion is gone.
#[allow(dead_code)]
fn _refs() {
    let _ = hash_source;
    let _: Option<StateError> = None;
}

#[test]
fn list_excludes_source_column() {
    let mut store = fresh_memory_store();
    store.register_strategy("a", "src1", None, None).unwrap();
    store.register_strategy("b", "src2", None, None).unwrap();

    let items = store.list_strategies(false).unwrap();
    assert_eq!(items.len(), 2);
    // StrategySummary type does not have a `source` field — compile-time guarantee.
    // Sanity: names present.
    let names: Vec<_> = items.iter().map(|i| i.name.clone()).collect();
    assert!(names.contains(&"a".to_string()) && names.contains(&"b".to_string()));
}

#[test]
fn list_filters_deleted_by_default() {
    let mut store = fresh_memory_store();
    let id1 = match store.register_strategy("a", "s1", None, None).unwrap() {
        RegisterOutcome::Created(s) => s.id,
        _ => unreachable!(),
    };
    store.register_strategy("b", "s2", None, None).unwrap();
    store.soft_delete_strategy(&id1).unwrap();

    assert_eq!(store.list_strategies(false).unwrap().len(), 1);
    assert_eq!(store.list_strategies(true).unwrap().len(), 2);
}

#[test]
fn get_by_id_returns_deleted() {
    let mut store = fresh_memory_store();
    let id = match store.register_strategy("a", "src", None, None).unwrap() {
        RegisterOutcome::Created(s) => s.id,
        _ => unreachable!(),
    };
    store.soft_delete_strategy(&id).unwrap();

    let row = store.get_strategy_by_id(&id).unwrap().expect("present");
    assert!(row.deleted_at.is_some());
}

#[test]
fn get_by_name_only_returns_active() {
    let mut store = fresh_memory_store();
    let id = match store.register_strategy("a", "src", None, None).unwrap() {
        RegisterOutcome::Created(s) => s.id,
        _ => unreachable!(),
    };
    store.soft_delete_strategy(&id).unwrap();

    assert!(store.get_strategy_by_name("a").unwrap().is_none());
}

#[test]
fn soft_delete_is_idempotent() {
    let mut store = fresh_memory_store();
    let id = match store.register_strategy("a", "src", None, None).unwrap() {
        RegisterOutcome::Created(s) => s.id,
        _ => unreachable!(),
    };

    let t1 = store.soft_delete_strategy(&id).unwrap();
    let t2 = store.soft_delete_strategy(&id).unwrap();
    assert_eq!(t1, t2, "second delete must return the original deleted_at");

    let row = store.get_strategy_by_id(&id).unwrap().unwrap();
    assert_eq!(row.deleted_at.as_deref(), Some(t1.as_str()));
}

#[test]
fn soft_delete_missing_returns_not_found() {
    let mut store = fresh_memory_store();
    let err = store.soft_delete_strategy("nope").expect_err("missing");
    assert!(matches!(err, StateError::NotFound(_)));
}
