//! D-05 Shape B + D-10 promise rejection tests.

use serde_json::json;
use strategy_js::{CtxStub, RuntimeError, Sandbox};

fn run(source: &str) -> Result<serde_json::Value, RuntimeError> {
    let mut host = CtxStub {
        strategy_id: "0".repeat(64),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        ..CtxStub::default()
    };
    Sandbox::execute(source, &mut host)
}

#[test]
fn execute_runs_minimal_noop_strategy() {
    let r = run("(ctx) => \"noop\"").expect("noop strategy must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn execute_runs_action_array_strategy() {
    let r = run("(ctx) => [{kind:\"noop\"}]").expect("must succeed");
    assert_eq!(r, json!([{"kind": "noop"}]));
}

#[test]
fn execute_runs_empty_array_strategy() {
    let r = run("(ctx) => []").expect("must succeed");
    assert_eq!(r, json!([]));
}

#[test]
fn execute_rejects_top_level_string_source() {
    let r = run("\"noop\"");
    match r {
        Err(RuntimeError::InvalidOutput { detail }) => {
            assert!(
                detail.to_lowercase().contains("function")
                    || detail.contains("(ctx) =>"),
                "detail missing Shape-B hint: {detail}"
            );
        }
        other => panic!("expected InvalidOutput, got: {other:?}"),
    }
}

#[test]
fn execute_rejects_top_level_object_source() {
    let r = run("({kind: \"noop\"})");
    assert!(matches!(r, Err(RuntimeError::InvalidOutput { .. })), "got: {r:?}");
}

#[test]
fn execute_rejects_promise_return() {
    let r = run("(ctx) => Promise.resolve(\"noop\")");
    match r {
        Err(RuntimeError::InvalidOutput { detail }) => {
            assert!(
                detail.to_lowercase().contains("promise"),
                "detail missing 'promise': {detail}"
            );
        }
        other => panic!("expected InvalidOutput(promise), got: {other:?}"),
    }
}

#[test]
fn execute_propagates_thrown_error_message() {
    let r = run("(ctx) => { throw new Error(\"nope\"); }");
    match r {
        Err(RuntimeError::Exception(msg)) => {
            assert!(msg.contains("nope"), "msg missing 'nope': {msg}");
        }
        other => panic!("expected Exception, got: {other:?}"),
    }
}

#[test]
fn execute_runs_distinct_invocations_independently() {
    // Prove no global state leaks between calls (fresh Runtime per call).
    let _ = run("(ctx) => { globalThis.LEAKED = 1; return \"noop\"; }").unwrap();
    let r = run(
        "(ctx) => typeof globalThis.LEAKED === 'undefined' ? \"noop\" : \"BAD\"",
    )
    .unwrap();
    assert_eq!(r, json!("noop"), "global leaked across runs");
}
