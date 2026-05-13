//! D-04 ctx surface behavioural tests using CtxStub.

use serde_json::json;
use strategy_js::{CtxHost, CtxStub, RuntimeError, Sandbox};

fn run_with_host(source: &str, host: &mut CtxStub) -> Result<serde_json::Value, RuntimeError> {
    Sandbox::execute(source, host)
}

fn make_host() -> CtxStub {
    CtxStub {
        strategy_id: "0".repeat(64),
        strategy_name: "arb".into(),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        logs: Vec::new(),
        event: None,
    }
}

/// CtxStub::now_millis returns 0 — that's all D-04 needs at the trait level
/// (the production-time clock comes from `RuntimeContext`'s NowMillisProvider
/// in Plan 03-02 Task 3). Here we just assert the injected snapshot equals 0.
#[test]
fn ctx_strategy_id_is_injected() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => ctx.strategy.id", &mut host).unwrap();
    assert_eq!(r, json!("0".repeat(64)));
}

#[test]
fn ctx_strategy_name_is_injected() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => ctx.strategy.name", &mut host).unwrap();
    assert_eq!(r, json!("arb"));
}

#[test]
fn ctx_run_id_is_injected() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => ctx.run.id", &mut host).unwrap();
    assert_eq!(r, json!("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
}

#[test]
fn ctx_now_returns_injected_millis() {
    // CtxStub::now_millis is hardcoded to 0; assert ctx.now() observes that.
    let mut host = make_host();
    let r = run_with_host("(ctx) => ctx.now()", &mut host).unwrap();
    // QuickJS represents the f64 0.0 as Type::Int; our walker emits a JSON
    // integer. The agent-visible value is 0 either way.
    let n = r.as_f64().expect("ctx.now() returns a JS number");
    assert_eq!(n, 0.0);
    assert!(host.now_millis() == 0);
}

/// Pin the f64 representation when ctx.now() returns a millisecond timestamp
/// that does not fit in i32. The walker should emit a JSON Number with no
/// precision loss for values < 2^53.
#[test]
fn ctx_now_preserves_large_millis() {
    // Use a direct host that returns a large-but-i53-safe value.
    struct BigHost;
    impl CtxHost for BigHost {
        fn strategy_id(&self) -> &str { "x" }
        fn strategy_name(&self) -> &str { "x" }
        fn run_id(&self) -> &str { "x" }
        fn now_millis(&self) -> i64 { 1_700_000_000_000 }
        fn append_log(&mut self, _m: String) {}
    }
    let mut h = BigHost;
    let r = Sandbox::execute("(ctx) => ctx.now()", &mut h).unwrap();
    let n = r.as_f64().expect("number");
    assert_eq!(n, 1_700_000_000_000.0);
}

#[test]
fn ctx_log_buffers_messages() {
    let mut host = make_host();
    let r = run_with_host(
        "(ctx) => { ctx.log(\"hello\", 42, true); ctx.log(\"again\"); return \"noop\"; }",
        &mut host,
    )
    .unwrap();
    assert_eq!(r, json!("noop"));
    assert_eq!(host.logs, vec!["hello 42 true".to_string(), "again".to_string()]);
}

/// Document the JS String() coercion observed for ctx.log args. JS spec:
///   String(1) => "1"
///   String(2.5) => "2.5"
///   String(null) => "null"
///   String(undefined) => "undefined"
///   String([1,2]) => "1,2"
#[test]
fn ctx_log_coerces_args_to_strings() {
    let mut host = make_host();
    let r = run_with_host(
        "(ctx) => { ctx.log(1, 2.5, null, undefined, [1,2]); return \"noop\"; }",
        &mut host,
    )
    .unwrap();
    assert_eq!(r, json!("noop"));
    assert_eq!(host.logs.len(), 1);
    assert_eq!(host.logs[0], "1 2.5 null undefined 1,2");
}

#[test]
fn ctx_actions_noop_returns_noop_string() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => ctx.actions.noop()", &mut host).unwrap();
    assert_eq!(r, json!("noop"));
}

#[test]
fn ctx_object_shape_matches_d04() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => Object.keys(ctx)", &mut host).unwrap();
    let mut actual: Vec<String> = serde_json::from_value(r).expect("array of strings");
    actual.sort();
    let mut expected = vec![
        "actions".to_string(),
        "address".to_string(), // Phase 4 D-11: ctx.address.* sub-namespace (04-04).
        "event".to_string(),   // v1.2 Trigger Core: ctx.event (null when no trigger).
        "evm".to_string(),     // Phase 4 D-04: ctx.evm.* sub-namespace.
        "log".to_string(),
        "now".to_string(),
        "run".to_string(),
        "strategy".to_string(),
        "units".to_string(),   // Phase 4 D-10: ctx.units.* sub-namespace (04-04).
    ];
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn ctx_strategy_object_shape() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => Object.keys(ctx.strategy)", &mut host).unwrap();
    let mut actual: Vec<String> = serde_json::from_value(r).unwrap();
    actual.sort();
    assert_eq!(actual, vec!["id".to_string(), "name".to_string()]);
}

#[test]
fn ctx_run_object_shape() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => Object.keys(ctx.run)", &mut host).unwrap();
    let actual: Vec<String> = serde_json::from_value(r).unwrap();
    assert_eq!(actual, vec!["id".to_string()]);
}

#[test]
fn ctx_actions_object_shape() {
    // Phase 4 D-08 / 04-03: ctx.actions gains 5 builder bindings on top of
    // the Phase-3 `noop`. Insertion order is preserved by V8 / QuickJS for
    // string keys, so we pin the exact sequence.
    let mut host = make_host();
    let r = run_with_host("(ctx) => Object.keys(ctx.actions)", &mut host).unwrap();
    let actual: Vec<String> = serde_json::from_value(r).unwrap();
    assert_eq!(
        actual,
        vec![
            "noop".to_string(),
            "contractCall".to_string(),
            "rawCall".to_string(),
            "erc20Transfer".to_string(),
            "erc20Approve".to_string(),
            "nativeTransfer".to_string(),
        ]
    );
}

#[test]
fn ctx_log_no_op_when_no_args() {
    let mut host = make_host();
    let r = run_with_host("(ctx) => { ctx.log(); return \"noop\"; }", &mut host).unwrap();
    assert_eq!(r, json!("noop"));
    // Empty arg list → empty join → single empty-string entry.
    assert_eq!(host.logs, vec!["".to_string()]);
}

#[test]
fn ctx_does_not_leak_between_runs() {
    // First call mutates the per-call ctx — second call must observe a fresh one.
    let mut host_a = make_host();
    let _ = run_with_host(
        "(ctx) => { ctx.HACK = 1; return \"noop\"; }",
        &mut host_a,
    )
    .unwrap();

    let mut host_b = make_host();
    let r = run_with_host(
        "(ctx) => typeof ctx.HACK === 'undefined' ? \"noop\" : \"BAD\"",
        &mut host_b,
    )
    .unwrap();
    assert_eq!(r, json!("noop"));
}
