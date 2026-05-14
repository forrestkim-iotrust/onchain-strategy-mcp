//! v1.2 Trigger Core — Stream C integration tests.
//!
//! Exercises the MCP-facing trigger CRUD by driving `StateStore` and the
//! Stream A helper types directly (no stdio JSON-RPC roundtrip — keeps the
//! test fast and focused on the CRUD contract that the tool handlers expose).
//! The 7 `trigger_*` MCP tools delegate to these same `StateStore` methods,
//! so this test is the operational floor for those handlers.

use executor_core::schema::trigger::{RegisterTriggerInput, TriggerKind, TriggerListFilter};
use executor_state::{StateStore, TriggerRegisterOutcome};
use serde_json::json;
use tempfile::tempdir;

fn fresh_store() -> (tempfile::TempDir, StateStore) {
    let tmp = tempdir().expect("tmp");
    let path = tmp.path().join("trigger_tools.db");
    let mut store = StateStore::open(&path).expect("open store");
    // Seed a strategy so the FK constraint on triggers.strategy_id is satisfied.
    store
        .register_strategy(
            "trigger-fixture",
            "return \"noop\";",
            Some("trigger CRUD fixture"),
            None,
        )
        .expect("seed strategy");
    (tmp, store)
}

fn fixture_strategy_id(store: &StateStore) -> String {
    store
        .get_strategy_by_name("trigger-fixture")
        .expect("get strategy")
        .expect("seeded strategy present")
        .id
}

#[test]
fn trigger_crud_round_trip() {
    let (_tmp, mut store) = fresh_store();
    let strategy_id = fixture_strategy_id(&store);

    // 1. register (interval kind, with config carrying interval_ms)
    let input = RegisterTriggerInput {
        strategy_id: strategy_id.clone(),
        kind: TriggerKind::Interval,
        config: json!({ "interval_ms": 1000 }),
        predicate: None,
        dedup_window_ms: None,
        note: None,
    };
    let outcome = store.register_trigger(input.clone()).expect("register");
    let trigger_id = match outcome {
        TriggerRegisterOutcome::Created(t) => {
            // config_json round-trips with the exact interval_ms value.
            let parsed: serde_json::Value =
                serde_json::from_str(&t.config_json).expect("config_json parse");
            assert_eq!(parsed["interval_ms"], json!(1000));
            assert!(t.enabled);
            assert_eq!(t.kind, TriggerKind::Interval);
            t.id
        }
        TriggerRegisterOutcome::AlreadyExists(_) => panic!("expected Created on first insert"),
    };

    // 2. idempotent register — same source returns AlreadyExists with same id.
    let again = store.register_trigger(input).expect("idempotent register");
    match again {
        TriggerRegisterOutcome::AlreadyExists(t) => assert_eq!(t.id, trigger_id),
        TriggerRegisterOutcome::Created(_) => panic!("second register should be idempotent"),
    }

    // 3. list — no filter sees the row.
    let all = store.list_triggers(None).expect("list");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, trigger_id);

    // 4. get — full row.
    let got = store
        .get_trigger(&trigger_id)
        .expect("get")
        .expect("present");
    assert_eq!(got.id, trigger_id);
    assert!(got.enabled);

    // 5. disable, then filter enabled=true should exclude it.
    store
        .set_trigger_enabled(&trigger_id, false)
        .expect("disable");
    let enabled_only = store
        .list_triggers(Some(&TriggerListFilter {
            enabled: Some(true),
            ..Default::default()
        }))
        .expect("list enabled");
    assert!(enabled_only.is_empty(), "disabled trigger filtered out");

    let disabled_only = store
        .list_triggers(Some(&TriggerListFilter {
            enabled: Some(false),
            ..Default::default()
        }))
        .expect("list disabled");
    assert_eq!(disabled_only.len(), 1);

    // 6. enable again, kind+strategy_id filters return it.
    store
        .set_trigger_enabled(&trigger_id, true)
        .expect("enable");
    let by_kind = store
        .list_triggers(Some(&TriggerListFilter {
            kind: Some(TriggerKind::Interval),
            ..Default::default()
        }))
        .expect("list by kind");
    assert_eq!(by_kind.len(), 1);
    let by_strategy = store
        .list_triggers(Some(&TriggerListFilter {
            strategy_id: Some(strategy_id.clone()),
            ..Default::default()
        }))
        .expect("list by strategy");
    assert_eq!(by_strategy.len(), 1);

    // 7. delete — returns true; subsequent get returns None (the "404" case).
    let deleted = store.delete_trigger(&trigger_id).expect("delete");
    assert!(deleted);
    let after = store.get_trigger(&trigger_id).expect("get after delete");
    assert!(after.is_none(), "deleted trigger absent");

    // 8. delete again is idempotent and returns false.
    let deleted_again = store.delete_trigger(&trigger_id).expect("delete idempotent");
    assert!(!deleted_again);
}

#[test]
fn trigger_register_unknown_strategy_is_not_found() {
    let (_tmp, mut store) = fresh_store();
    let input = RegisterTriggerInput {
        strategy_id: "deadbeef".to_string(),
        kind: TriggerKind::Manual,
        config: json!({}),
        predicate: None,
        dedup_window_ms: None,
        note: None,
    };
    let err = store.register_trigger(input).expect_err("strategy must exist");
    let msg = format!("{err}");
    assert!(msg.contains("not found"), "got: {msg}");
}

#[test]
fn trigger_events_round_trip_and_limit() {
    let (_tmp, mut store) = fresh_store();
    let strategy_id = fixture_strategy_id(&store);
    let input = RegisterTriggerInput {
        strategy_id,
        kind: TriggerKind::Manual,
        config: json!({}),
        predicate: None,
        dedup_window_ms: None,
        note: None,
    };
    let trigger = match store.register_trigger(input).expect("register") {
        TriggerRegisterOutcome::Created(t) => t,
        TriggerRegisterOutcome::AlreadyExists(_) => unreachable!(),
    };

    // Record three events.
    for i in 0..3 {
        store
            .record_trigger_event(
                &trigger.id,
                Some(&format!("{{\"i\":{i}}}")),
                None,
                None,
                None,
            )
            .expect("record event");
    }

    let events = store.list_trigger_events(&trigger.id, 50).expect("list");
    assert_eq!(events.len(), 3);

    // Limit clamps result size.
    let one = store.list_trigger_events(&trigger.id, 1).expect("list 1");
    assert_eq!(one.len(), 1);
}
