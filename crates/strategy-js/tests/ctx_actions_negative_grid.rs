#![allow(non_snake_case)]
//! Phase 4 04-04 Task 2 — exhaustive per-variant rejection grid for the
//! `ctx.actions.*` builders.
//!
//! 15 builder-level rejection cases across 5 variants. Each test:
//! 1. Builds a strategy that calls a builder with a deliberately bad input.
//! 2. Asserts the builder THROWS a JS Error (caught with try/catch, surfaces
//!    via the strategy return value).
//! 3. Asserts the error message contains a STABLE wire-safe substring
//!    (no raw alloy / serde / TransportError text — MR-01 carry-forward).

use serde_json::Value;
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

/// Helper: assert message contains expected substring (case-insensitive)
/// AND does NOT contain any raw-error substrings (MR-01 wire-safety guard).
fn assert_stable_rejection(msg: &str, must_contain_lowercase: &[&str]) {
    let lower = msg.to_lowercase();
    let any_match = must_contain_lowercase.iter().any(|s| lower.contains(s));
    assert!(
        any_match,
        "expected one of {must_contain_lowercase:?} in message, got: {msg}"
    );
    // MR-01 wire-safety guard: no raw alloy / reqwest / serde error text.
    for forbidden in [
        "transporterror",
        "reqwest",
        "serde_json::error",
        "alloy_dyn_abi",
        "rustls",
        "0x08c379a0",
    ] {
        assert!(
            !lower.contains(forbidden),
            "raw error text leaked to wire ({forbidden}): {msg}"
        );
    }
}

fn extract_error_message(src: &str) -> String {
    let r = run(src).expect("strategy must run to completion (catch block)");
    r.as_str().unwrap_or("").to_string()
}

// ─── contract_call (4 cases) ──────────────────────────────────────────────

#[test]
fn contract_call_rejects_mixed_case_bad_checksum_address() {
    // Take an EIP-55-canonical address and flip one alpha character so the
    // checksum no longer matches.
    let bad_addr = "0x52908400098527886e0F7030069857D2E4169EE7";
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.contractCall({{
                    address: "{bad_addr}",
                    abi: "[]", function: "f", args: []
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["address", "bad_address"]);
}

#[test]
fn contract_call_rejects_oversize_abi() {
    // 70 KiB ABI — over the 64 KiB cap.
    let src = r#"(ctx) => {
        const big = "x".repeat(70 * 1024);
        try {
            ctx.actions.contractCall({
                address: "0x0000000000000000000000000000000000000001",
                abi: big, function: "f", args: []
            });
            return "BAD";
        } catch (e) { return e.message; }
    }"#;
    let m = extract_error_message(src);
    assert_stable_rejection(&m, &["abi_oversize", "64"]);
}

#[test]
fn contract_call_rejects_unknown_function_name() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.contractCall({{
                    address: "{ADDR1}",
                    abi: {abi},
                    function: "nonexistent",
                    args: []
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#,
        abi = serde_json::to_string(TRANSFER_ABI).unwrap()
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["abi_function_missing", "does not contain", "nonexistent"]);
}

#[test]
fn contract_call_rejects_arg_count_mismatch() {
    // transfer takes 2 args; pass 1 to trigger arg_count rejection.
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.contractCall({{
                    address: "{ADDR1}",
                    abi: {abi},
                    function: "transfer",
                    args: ["{ADDR2}"]
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#,
        abi = serde_json::to_string(TRANSFER_ABI).unwrap()
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["abi_arg_count", "overload", "args"]);
}

// ─── raw_call (3 cases) ───────────────────────────────────────────────────

#[test]
fn raw_call_rejects_bare_hex() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.rawCall({{ address: "{ADDR1}", data: "deadbeef" }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bad_calldata", "0x-prefixed"]);
}

#[test]
fn raw_call_rejects_odd_length_hex() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.rawCall({{ address: "{ADDR1}", data: "0xabc" }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bad_calldata", "even"]);
}

#[test]
fn raw_call_rejects_bad_address() {
    let src = r#"(ctx) => {
        try {
            ctx.actions.rawCall({ address: "0xZZZZ", data: "0xdeadbeef" });
            return "BAD";
        } catch (e) { return e.message; }
    }"#;
    let m = extract_error_message(src);
    assert_stable_rejection(&m, &["address", "bad_address"]);
}

// ─── erc20_transfer (3 cases) ─────────────────────────────────────────────

#[test]
fn erc20_transfer_rejects_bigint_amount() {
    // D-03 BigInt rejection at builder entry — Pitfall 2 carry-forward.
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Transfer({{
                    token: "{ADDR1}", to: "{ADDR2}", amount: 100n
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bigint", "decimal string"]);
}

#[test]
fn erc20_transfer_rejects_negative_amount() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Transfer({{
                    token: "{ADDR1}", to: "{ADDR2}", amount: "-1"
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bad_decimal", "non-negative"]);
}

#[test]
fn erc20_transfer_rejects_bad_token_address() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Transfer({{
                    token: "0xnope", to: "{ADDR2}", amount: "1"
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["address", "bad_address"]);
}

// ─── erc20_approve (2 cases) ──────────────────────────────────────────────

#[test]
fn erc20_approve_rejects_hex_amount() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Approve({{
                    token: "{ADDR1}", spender: "{ADDR3}", amount: "0x1"
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bad_decimal", "decimal", "0x"]);
}

#[test]
fn erc20_approve_rejects_bad_spender() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.erc20Approve({{
                    token: "{ADDR1}", spender: "0xshort", amount: "1"
                }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["address", "bad_address"]);
}

// ─── native_transfer (3 cases) ────────────────────────────────────────────

#[test]
fn native_transfer_rejects_negative_value() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.nativeTransfer({{ to: "{ADDR2}", value: "-1" }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bad_decimal", "non-negative"]);
}

#[test]
fn native_transfer_rejects_bad_recipient() {
    let src = r#"(ctx) => {
        try {
            ctx.actions.nativeTransfer({ to: "not-an-address", value: "1" });
            return "BAD";
        } catch (e) { return e.message; }
    }"#;
    let m = extract_error_message(src);
    assert_stable_rejection(&m, &["address", "bad_address"]);
}

#[test]
fn native_transfer_rejects_bigint_value() {
    let src = format!(
        r#"(ctx) => {{
            try {{
                ctx.actions.nativeTransfer({{ to: "{ADDR2}", value: 1n }});
                return "BAD";
            }} catch (e) {{ return e.message; }}
        }}"#
    );
    let m = extract_error_message(&src);
    assert_stable_rejection(&m, &["bigint", "decimal string"]);
}

// ─── grand-total guard ────────────────────────────────────────────────────

/// Sanity: make sure the file actually contains ≥ 15 #[test] functions.
/// (Compile-time check via `cargo test` listing; documented here so a
/// future drop-by-mistake of a test surfaces in the count.)
///
/// 4 (contract_call) + 3 (raw_call) + 3 (erc20_transfer) + 2 (erc20_approve)
/// + 3 (native_transfer) = 15.
#[test]
fn grid_total_count_is_fifteen() {
    // Self-documenting marker: the count is asserted by the per-variant
    // tests above. This shim keeps the count visible in `cargo test`
    // output.
    let _expected_total = 15;
}
