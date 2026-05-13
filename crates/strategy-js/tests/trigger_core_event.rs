//! v1.2 Trigger Core — Stream B sandbox extension tests.
//!
//! Covers:
//! - `ctx.event` is `null` when the host returns `None`
//! - `ctx.event` is the projected object when the host returns `Some(...)`
//! - `Sandbox::evaluate_predicate` returns `Ok(true)` / `Ok(false)` per spec
//! - predicate returns `Ok(false)` defensively on non-bool / throw / timeout

use std::time::{Duration, Instant};

use serde_json::json;
use strategy_js::{CtxStub, RuntimeError, Sandbox};

fn make_host_with_event(event: Option<serde_json::Value>) -> CtxStub {
    CtxStub {
        strategy_id: "0".repeat(64),
        strategy_name: "trigger".into(),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        logs: Vec::new(),
        event,
    }
}

#[test]
fn ctx_event_is_null_when_host_returns_none() {
    let mut host = make_host_with_event(None);
    let r =
        Sandbox::execute("(ctx) => (ctx.event === null) ? 'noop' : 'fail'", &mut host).unwrap();
    assert_eq!(r, json!("noop"), "ctx.event must be null when host has no event");
}

#[test]
fn ctx_event_is_object_when_host_returns_some() {
    let payload = json!({
        "kind": "interval",
        "tick": 7,
        "nested": { "addr": "0x1234" },
    });
    let mut host = make_host_with_event(Some(payload.clone()));
    let r = Sandbox::execute(
        "(ctx) => ({ kind: ctx.event.kind, tick: ctx.event.tick, addr: ctx.event.nested.addr })",
        &mut host,
    )
    .unwrap();
    assert_eq!(
        r,
        json!({ "kind": "interval", "tick": 7, "addr": "0x1234" }),
    );
}

#[test]
fn predicate_returns_true_for_simple_true_predicate() {
    let event = json!({ "tick": 1 });
    let r = Sandbox::evaluate_predicate("(event) => event.tick === 1", &event).unwrap();
    assert!(r, "predicate returning true should produce Ok(true)");
}

#[test]
fn predicate_returns_false_when_function_returns_false() {
    let event = json!({ "tick": 2 });
    let r = Sandbox::evaluate_predicate("(event) => event.tick === 1", &event).unwrap();
    assert!(!r);
}

#[test]
fn predicate_returns_false_when_function_returns_non_boolean() {
    // Non-boolean (number, string, null, object) must coerce to false via the
    // `=== true` guard in the wrapper.
    for src in [
        "(event) => 1",
        "(event) => 'true'",
        "(event) => null",
        "(event) => ({ ok: true })",
        "(event) => undefined",
    ] {
        let r = Sandbox::evaluate_predicate(src, &json!({})).unwrap();
        assert!(!r, "non-bool predicate `{src}` must coerce to false, got true");
    }
}

#[test]
fn predicate_returns_false_when_function_throws() {
    let r = Sandbox::evaluate_predicate(
        "(event) => { throw new Error('boom'); }",
        &json!({}),
    )
    .unwrap();
    assert!(!r, "throwing predicate must return Ok(false)");
}

#[test]
fn predicate_respects_wall_clock_budget() {
    // Same wall-clock as Sandbox::execute (D-03 WALL_CLOCK_MS). An infinite
    // loop must be interrupted and produce Ok(false) — not hang, not Err.
    let start = Instant::now();
    let r = Sandbox::evaluate_predicate("(event) => { while (true) {} }", &json!({})).unwrap();
    let elapsed = start.elapsed();
    assert!(!r, "timed-out predicate must collapse to Ok(false)");
    assert!(
        elapsed < Duration::from_millis(5_000),
        "predicate must respect wall-clock budget; elapsed={elapsed:?}"
    );
}

/// Smoke: engine init failures are the ONLY case `evaluate_predicate` returns
/// Err. We can't easily force one in a unit test (would need host-level OOM),
/// so instead we assert that the typical "obviously syntactically broken"
/// source still produces `Ok(false)` — i.e. parse errors are caught as
/// exceptions, not propagated as engine-init failures.
#[test]
fn predicate_syntax_error_returns_false_not_err() {
    let r = Sandbox::evaluate_predicate("@@@not valid js@@@", &json!({}));
    match r {
        Ok(false) => {}
        Ok(true) => panic!("syntax-error predicate must not return true"),
        Err(RuntimeError::EngineInit(_)) => {
            panic!("syntax error must not surface as EngineInit")
        }
        Err(e) => panic!("syntax error must collapse to Ok(false), got Err({e:?})"),
    }
}
