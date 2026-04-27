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
    assert_eq!(created.id, hash_source("// code"));
    assert_eq!(created.name, "arb");
    assert_eq!(created.description.as_deref(), Some("desc"));
    assert_eq!(created.tags.as_deref(), Some(&tags[..]));

    let by_id = store
        .get_strategy_by_id(&created.id)
        .expect("get_by_id")
        .expect("present");
    assert_eq!(by_id.source, "// code");
    assert_eq!(by_id.tags.as_deref(), Some(&tags[..]));

    let by_name = store
        .get_strategy_by_name("arb")
        .expect("get_by_name")
        .expect("present");
    assert_eq!(by_name.id, created.id);
}

#[test]
fn register_idempotent_same_source() {
    let mut store = fresh_memory_store();
    let first = store
        .register_strategy("arb", "src-A", Some("first-desc"), None)
        .expect("register first");
    let first_id = match &first {
        RegisterOutcome::Created(s) => s.id.clone(),
        _ => panic!(),
    };

    // Re-register same source with a DIFFERENT name + description.
    // Must return AlreadyExists carrying the ORIGINAL row.
    let second = store
        .register_strategy("renamed", "src-A", Some("ignored"), None)
        .expect("register same source");
    let existing = match second {
        RegisterOutcome::AlreadyExists(s) => s,
        _ => panic!("second register must be AlreadyExists"),
    };
    assert_eq!(existing.id, first_id);
    assert_eq!(existing.name, "arb"); // immutability — original name preserved
    assert_eq!(existing.description.as_deref(), Some("first-desc"));
}

#[test]
fn register_conflict_same_name_different_source() {
    let mut store = fresh_memory_store();
    store
        .register_strategy("arb", "src-A", None, None)
        .expect("first register");

    let err = store
        .register_strategy("arb", "src-B", None, None)
        .expect_err("conflict expected");
    match err {
        StateError::NameConflict {
            attempted_name,
            existing_strategy_id,
            ..
        } => {
            assert_eq!(attempted_name, "arb");
            assert_eq!(existing_strategy_id, hash_source("src-A"));
        }
        other => panic!("expected NameConflict, got {other:?}"),
    }
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
