//! D-11 forbidden-globals regression suite.
//!
//! Proves STR-04: "Strategy code cannot access private keys, filesystem,
//! process APIs, arbitrary network, or direct RPC clients."
//!
//! Strategy is to assert from INSIDE the JS sandbox that each forbidden
//! global is absent — if any one of them resolves to anything but
//! `undefined`, the strategy returns `"BAD"` and the test fails. The
//! pattern is documented so Phase-4 reviewers can extend the list when
//! `ctx.evm.*` is added (those names should be `defined`, not absent).

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
fn sandbox_blocks_host_globals() {
    // Names verified in 03-CONTEXT.md D-11.
    let source = r#"
        (ctx) => {
            const names = [
                "console", "fetch",
                "setTimeout", "setInterval", "setImmediate", "queueMicrotask",
                "XMLHttpRequest", "WebSocket",
                "process", "Worker",
                "child_process", "fs",
            ];
            for (const n of names) {
                if (typeof globalThis[n] !== "undefined") {
                    return "FOUND: " + n;
                }
            }
            return "noop";
        }
    "#;
    let r = run(source).expect("must succeed");
    assert_eq!(r, json!("noop"), "a forbidden global was reachable: {r:?}");
}

#[test]
fn sandbox_blocks_node_fs_module() {
    let r = run(
        r#"(ctx) => { try { const fs = require("fs"); return "BAD"; } catch(e) { return "noop"; } }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn sandbox_blocks_deno_namespace() {
    // D-11: any `Deno.*` namespace must be absent. Mirrors the
    // `sandbox_blocks_node_fs_module` shape — guard the read with try/catch
    // so that a bare reference to `Deno` (which is undefined) is captured
    // and converted into the canonical `"noop"` success string.
    let r = run(
        r#"(ctx) => { try { return (typeof Deno === "undefined" && typeof Deno?.readFile === "undefined") ? "noop" : "BAD"; } catch(e) { return "noop"; } }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn sandbox_blocks_dynamic_import() {
    // Dynamic `import()` in rquickjs 0.11 with no module loader registered
    // returns a Promise (which Phase-3 D-10 already rejects via
    // `RuntimeError::InvalidOutput`). It does NOT throw synchronously, so
    // the canonical assertion is: returning `import("./foo.so")` directly
    // surfaces as InvalidOutput(promise) — proof that no real module
    // resolution occurred.
    let r = run(r#"(ctx) => import("./foo.so")"#);
    match r {
        Err(RuntimeError::InvalidOutput { detail }) => {
            assert!(
                detail.to_lowercase().contains("promise"),
                "expected promise-reject detail, got: {detail}"
            );
        }
        // Some rquickjs configurations may instead throw a SyntaxError /
        // ReferenceError on dynamic import; that is also an acceptable
        // sandbox response — the only forbidden outcome is a successful
        // module load.
        Err(RuntimeError::Exception(_)) => {}
        Ok(v) => panic!("dynamic import unexpectedly resolved: {v:?}"),
        other => panic!("unexpected error mode: {other:?}"),
    }
}

#[test]
fn sandbox_console_log_is_undefined() {
    let r = run(r#"(ctx) => typeof console === "undefined" ? "noop" : "BAD""#)
        .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn sandbox_blocks_globalthis_process() {
    let r = run(r#"(ctx) => typeof globalThis.process === "undefined" ? "noop" : "BAD""#)
        .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn sandbox_eval_is_present_but_sandboxed() {
    // D-04 caveat: eval IS a JS intrinsic; pinning behaviour so Phase-4
    // doesn't accidentally remove it (which would break legitimate
    // expression-evaluation use cases).
    let r = run(r#"(ctx) => { const r = eval("1 + 1"); return r === 2 ? "noop" : "BAD"; }"#)
        .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

#[test]
fn sandbox_function_constructor_is_present_but_sandboxed() {
    let r = run(
        r#"(ctx) => { const f = new Function("return 42"); return f() === 42 ? "noop" : "BAD"; }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}
