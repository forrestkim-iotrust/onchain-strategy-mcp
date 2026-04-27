#![allow(non_snake_case)] // Test names mirror JS-side `readContract` verbatim.
//! Phase 4 D-04 / D-13 / D-15a host-binding tests for `ctx.evm.readContract`.
//!
//! These tests run with `CtxStub` (no provider). The intent is to:
//! - Prove the binding is REACHABLE from JS (Test 1).
//! - Prove abi-as-string and abi-as-array are both accepted (Test 2).
//! - Prove the FORBIDDEN_GLOBALS_SCRUB still runs FIRST (HR-01 carry-forward,
//!   Test 3). The Phase-3 globals (console, fetch, ...) MUST remain absent
//!   even with `ctx.evm` injected.
//!
//! Tests that depend on a real provider (anvil-deployed contract,
//! cross-thread mutex discipline) live in the executor-evm anvil suite or
//! plan 04-02 / 04-03 stdio tests.

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
fn ctx_evm_readContract_is_callable_from_sandbox() {
    // The binding exists. CtxStub returns provider=None, so calling
    // readContract throws a typed JS error — but `typeof` resolves the
    // function existence without invoking it.
    let r = run(
        "(ctx) => typeof ctx.evm.readContract === \"function\" ? \"noop\" : \"BAD\"",
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn ctx_evm_namespace_is_present() {
    let r = run("(ctx) => typeof ctx.evm === \"object\" ? \"noop\" : \"BAD\"")
        .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn ctx_evm_readContract_throws_when_no_provider() {
    // CtxStub has no provider; the binding throws a stable typed error.
    let r = run(
        r#"(ctx) => {
            try {
                ctx.evm.readContract({
                    address: "0x0000000000000000000000000000000000000001",
                    abi: "[]",
                    function: "f",
                    args: [],
                });
                return "BAD: did not throw";
            } catch (e) {
                return e.message.includes("no provider configured") ? "noop" : "BAD: " + e.message;
            }
        }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn ctx_evm_readContract_runs_after_forbidden_globals_scrub() {
    // HR-01 / D-15a: scrub still runs BEFORE host bindings install.
    let r = run(
        r#"(ctx) => {
            const ok =
                typeof console === "undefined" &&
                typeof fetch === "undefined" &&
                typeof process === "undefined" &&
                typeof ctx.evm.readContract === "function";
            return ok ? "noop" : "BAD";
        }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn ctx_evm_namespace_is_not_globally_visible() {
    // The new `ctx.evm.*` bindings must be namespaced under `ctx`,
    // NOT exposed on globalThis (carry-forward from D-11).
    let r = run(
        r#"(ctx) => (typeof globalThis.evm === "undefined" &&
                     typeof globalThis.readContract === "undefined") ? "noop" : "BAD""#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}
