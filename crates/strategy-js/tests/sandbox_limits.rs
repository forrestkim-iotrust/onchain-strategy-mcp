//! Resource-limit regression tests (D-03). Each test must complete within
//! `WALL_CLOCK_MS + 500ms` of wall time, otherwise the interrupt handler
//! is broken.

use std::time::{Duration, Instant};
use strategy_js::{CtxStub, RuntimeError, Sandbox};

fn run(source: &str) -> Result<serde_json::Value, RuntimeError> {
    let mut host = CtxStub {
        strategy_id: "0".repeat(64),
        strategy_name: "test".into(),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        logs: Vec::new(),
        event: None,
    };
    Sandbox::execute(source, &mut host)
}

#[test]
fn wall_clock_interrupt_terminates_infinite_loop() {
    let start = Instant::now();
    let r = run("(ctx) => { while(true){} }");
    let elapsed = start.elapsed();
    assert!(matches!(r, Err(RuntimeError::Timeout)), "got: {r:?}");
    assert!(
        elapsed < Duration::from_millis(strategy_js::limits::WALL_CLOCK_MS + 500),
        "interrupt fired too late: {elapsed:?}"
    );
}

#[test]
fn memory_limit_terminates_oom_strategy() {
    let r = run(
        "(ctx) => { let a=[]; while(true) a.push(new Array(1000000).fill(0)); }",
    );
    // Could be Oom OR Timeout depending on which fires first; assert the
    // run did NOT succeed and is one of the two expected fail modes.
    match r {
        Err(RuntimeError::Oom) | Err(RuntimeError::Timeout) => {}
        other => panic!("expected Oom or Timeout, got: {other:?}"),
    }
}

#[test]
fn stack_limit_terminates_recursive_strategy() {
    let r = run("(ctx) => { function f(){f();} f(); }");
    // Stack overflow may surface as StackOverflow OR as a thrown Exception
    // depending on rquickjs version; both prove the cap fires.
    match r {
        Err(RuntimeError::StackOverflow) | Err(RuntimeError::Exception(_)) => {}
        other => panic!("expected StackOverflow or Exception, got: {other:?}"),
    }
}
