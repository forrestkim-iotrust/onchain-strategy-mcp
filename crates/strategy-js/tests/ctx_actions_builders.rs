#![allow(non_snake_case)]
//! Phase 4 D-08 / D-09 / D-15a action builder tests for
//! `ctx.actions.{contractCall, rawCall, erc20Transfer, erc20Approve, nativeTransfer}`.
//!
//! Builders are pure synchronous host functions — provider is irrelevant.
//! Tests run with `CtxStub` (no provider, no journal). For round-trip
//! checks, the JSON returned by the builder is fed through serde
//! deserialization against `executor_core::schema::action::Action` —
//! the same call `executor-mcp::validate_strategy_output` performs.

use executor_core::schema::action::Action;
use serde_json::{Value, json};
use strategy_js::{CtxStub, RuntimeError, Sandbox};

fn run(source: &str) -> Result<Value, RuntimeError> {
    let mut host = CtxStub {
        strategy_id: "0".repeat(64),
        run_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        ..CtxStub::default()
    };
    Sandbox::execute(source, &mut host)
}

const ADDR1: &str = "0x0000000000000000000000000000000000000001";
const ADDR2: &str = "0x0000000000000000000000000000000000000002";
const ADDR3: &str = "0x0000000000000000000000000000000000000003";

const TRANSFER_ABI: &str = r#"[
    {"type":"function","name":"transfer","inputs":[
        {"name":"to","type":"address"},
        {"name":"amount","type":"uint256"}
    ],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}
]"#;

// ─── contract_call ────────────────────────────────────────────────────────

#[test]
fn contract_call_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.contractCall({{
            address: "{ADDR1}",
            abi: {abi},
            function: "transfer",
            args: ["{ADDR2}", "1000"]
        }})]"#,
        abi = serde_json::to_string(TRANSFER_ABI).unwrap()
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["kind"].as_str(), Some("contract_call"));
    assert_eq!(arr[0]["address"].as_str(), Some(ADDR1));
    assert_eq!(arr[0]["function"].as_str(), Some("transfer"));
    assert_eq!(arr[0]["value"].as_str(), Some("0"));
    // Round-trip via serde
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::ContractCall(_)));
}

#[test]
fn contract_call_builder_accepts_abi_array_form() {
    // abi as JS array of fragments (no JSON.stringify on the JS side).
    let src = format!(
        r#"(ctx) => [ctx.actions.contractCall({{
            address: "{ADDR1}",
            abi: [{{
                type: "function",
                name: "f",
                inputs: [],
                outputs: [],
                stateMutability: "nonpayable"
            }}],
            function: "f",
            args: []
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("contract_call"));
    // abi field carries the serialized JSON string
    assert!(arr[0]["abi"].as_str().expect("abi string").contains("\"f\""));
}

#[test]
fn contract_call_builder_rejects_bad_address() {
    let bad_address = "0xZZZZZ";
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.contractCall({{
                    address: "{bad_address}",
                    abi: "[]",
                    function: "f",
                    args: []
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("address") || msg.contains("encode error"),
        "expected stable address-rejection message, got: {msg}"
    );
}

#[test]
fn contract_call_builder_rejects_oversize_abi() {
    // 70 KiB ABI — over the 64 KiB cap.
    let src = format!(
        r#"(ctx) => {{
            const big = "x".repeat(70 * 1024);
            try {{
                ctx.actions.contractCall({{
                    address: "{ADDR1}",
                    abi: big,
                    function: "f",
                    args: []
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    assert!(
        msg.contains("abi_oversize") || msg.contains("65536") || msg.contains("64"),
        "expected oversize abi rejection, got: {msg}"
    );
}

#[test]
fn contract_call_builder_rejects_function_missing_in_abi() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.contractCall({{
                    address: "{ADDR1}",
                    abi: {abi},
                    function: "nonexistent",
                    args: []
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#,
        abi = serde_json::to_string(TRANSFER_ABI).unwrap()
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    assert!(
        msg.contains("abi_function_missing") || msg.contains("does not contain"),
        "expected function-missing rejection, got: {msg}"
    );
}

// ─── raw_call ─────────────────────────────────────────────────────────────

#[test]
fn raw_call_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.rawCall({{
            address: "{ADDR1}",
            data: "0xdeadbeef"
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("raw_call"));
    assert_eq!(arr[0]["data"].as_str(), Some("0xdeadbeef"));
    assert_eq!(arr[0]["value"].as_str(), Some("0"));
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::RawCall(_)));
}

#[test]
fn raw_call_builder_rejects_bare_hex() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.rawCall({{
                    address: "{ADDR1}",
                    data: "deadbeef"
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    assert!(
        msg.contains("bad_calldata") || msg.contains("0x-prefixed"),
        "expected calldata 0x-prefix rejection, got: {msg}"
    );
}

// ─── erc20_transfer / erc20_approve ───────────────────────────────────────

#[test]
fn erc20_transfer_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.erc20Transfer({{
            token: "{ADDR1}",
            to:    "{ADDR2}",
            amount: "1000"
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("erc20_transfer"));
    assert_eq!(arr[0]["amount"].as_str(), Some("1000"));
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::Erc20Transfer(_)));
}

#[test]
fn erc20_transfer_builder_rejects_negative_amount() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Transfer({{
                    token: "{ADDR1}",
                    to:    "{ADDR2}",
                    amount: "-1"
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    assert!(
        msg.contains("bad_decimal") || msg.contains("non-negative"),
        "expected negative-amount rejection, got: {msg}"
    );
}

#[test]
fn erc20_transfer_builder_rejects_bigint_amount() {
    // RESEARCH Pitfall 2 / D-03 — BigInt at builder input MUST surface a
    // stable D-03 message, NOT a confused JSON-conversion error.
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Transfer({{
                    token: "{ADDR1}",
                    to:    "{ADDR2}",
                    amount: 100n
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    let lower = msg.to_lowercase();
    assert!(
        lower.contains("bigint") || lower.contains("decimal string"),
        "expected stable BigInt rejection message, got: {msg}"
    );
}

#[test]
fn erc20_approve_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.erc20Approve({{
            token:   "{ADDR1}",
            spender: "{ADDR3}",
            amount:  "0"
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("erc20_approve"));
    assert_eq!(arr[0]["spender"].as_str(), Some(ADDR3));
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::Erc20Approve(_)));
}

// ─── native_transfer ──────────────────────────────────────────────────────

#[test]
fn native_transfer_builder_returns_valid_json() {
    let src = format!(
        r#"(ctx) => [ctx.actions.nativeTransfer({{
            to:    "{ADDR2}",
            value: "1000000000000000000"
        }})]"#
    );
    let r = run(&src).expect("must succeed");
    let arr = r.as_array().expect("array");
    assert_eq!(arr[0]["kind"].as_str(), Some("native_transfer"));
    assert_eq!(arr[0]["value"].as_str(), Some("1000000000000000000"));
    let action: Action =
        serde_json::from_value(arr[0].clone()).expect("Action deserialize");
    assert!(matches!(action, Action::NativeTransfer(_)));
}

#[test]
fn native_transfer_builder_rejects_negative_value() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.nativeTransfer({{
                    to:    "{ADDR2}",
                    value: "-1"
                }});
                return "BAD: did not throw";
            }} catch (e) {{
                return e.message;
            }}
        }}"#
    );
    let r = run(&src).expect("must succeed");
    let msg = r.as_str().unwrap_or_default();
    assert!(
        msg.contains("bad_decimal") || msg.contains("non-negative"),
        "expected negative-value rejection, got: {msg}"
    );
}

// ─── HR-01 carry-forward ───────────────────────────────────────────────────

#[test]
fn sandbox_blocks_host_globals_after_phase4_action_builders_added() {
    // D-15a: FORBIDDEN_GLOBALS_SCRUB still runs BEFORE all ctx.* host
    // bindings install. The Phase-3 D-11 absent globals MUST stay absent
    // even with the new Phase-4 builders.
    let r = run(
        r#"(ctx) => {
            const ok =
                typeof console === "undefined" &&
                typeof fetch === "undefined" &&
                typeof process === "undefined" &&
                typeof setTimeout === "undefined" &&
                typeof ctx.actions.contractCall === "function" &&
                typeof ctx.actions.rawCall === "function" &&
                typeof ctx.actions.erc20Transfer === "function" &&
                typeof ctx.actions.erc20Approve === "function" &&
                typeof ctx.actions.nativeTransfer === "function";
            return ok ? "noop" : "BAD";
        }"#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}

// ─── builders not on globalThis ────────────────────────────────────────────

#[test]
fn action_builders_are_namespaced_under_ctx() {
    let r = run(
        r#"(ctx) => (typeof globalThis.contractCall === "undefined" &&
                     typeof globalThis.rawCall === "undefined" &&
                     typeof globalThis.erc20Transfer === "undefined") ? "noop" : "BAD""#,
    )
    .expect("must succeed");
    assert_eq!(r, json!("noop"));
}
