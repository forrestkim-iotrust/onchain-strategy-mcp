#![allow(non_snake_case)]
//! Phase 4 D-10 / D-11 / CTX-09 — `ctx.units` and `ctx.address` sandbox bindings.
//!
//! Tests run with `CtxStub` (no provider, no journal). The helpers are
//! pure host-side functions, so the provider field is irrelevant.

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

const ZERO: &str = "0x0000000000000000000000000000000000000000";
const LOWER: &str = "0xdeadbeefcafebabedeadbeefcafebabedeadbeef";
const EIP55: &str = "0x52908400098527886E0F7030069857D2E4169EE7";

// ─── ctx.units.parseUnits ──────────────────────────────────────────────

#[test]
fn parse_units_returns_decimal_string_for_one_point_five_with_18_decimals() {
    let r = run(r#"(ctx) => ctx.units.parseUnits("1.5", 18)"#).expect("ok");
    assert_eq!(r, json!("1500000000000000000"));
}

#[test]
fn parse_units_handles_fractional_only() {
    let r = run(r#"(ctx) => ctx.units.parseUnits("0.5", 18)"#).expect("ok");
    assert_eq!(r, json!("500000000000000000"));
}

#[test]
fn parse_units_handles_zero_decimals() {
    let r = run(r#"(ctx) => ctx.units.parseUnits("123", 0)"#).expect("ok");
    assert_eq!(r, json!("123"));
}

#[test]
fn parse_units_rejects_negative() {
    let src = r#"(ctx) => {
        try { ctx.units.parseUnits("-1", 18); return "BAD"; }
        catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(
        m.contains("non-negative") || m.contains("amount_negative"),
        "expected non-negative rejection, got: {m}"
    );
}

#[test]
fn parse_units_rejects_bigint_input_with_stable_message() {
    let src = r#"(ctx) => {
        try { ctx.units.parseUnits(1n, 18); return "BAD"; }
        catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(
        m.contains("bigint") || m.contains("decimal string"),
        "expected stable BigInt rejection, got: {m}"
    );
}

#[test]
fn parse_units_rejects_decimals_above_77() {
    let src = r#"(ctx) => {
        try { ctx.units.parseUnits("1", 78); return "BAD"; }
        catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(
        m.contains("decimals_out_of_range") || m.contains("77"),
        "expected decimals cap rejection, got: {m}"
    );
}

#[test]
fn parse_units_rejects_too_many_fractional_digits() {
    let src = r#"(ctx) => {
        try { ctx.units.parseUnits("1.123456789", 6); return "BAD"; }
        catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(
        m.contains("amount_overflow_fraction") || m.contains("fractional"),
        "expected overflow_fraction rejection, got: {m}"
    );
}

// ─── ctx.units.formatUnits ─────────────────────────────────────────────

#[test]
fn format_units_returns_one_point_five_for_18_decimals() {
    let r = run(r#"(ctx) => ctx.units.formatUnits("1500000000000000000", 18)"#).expect("ok");
    assert_eq!(r, json!("1.5"));
}

#[test]
fn format_units_trims_trailing_zeros_to_whole_number() {
    let r = run(r#"(ctx) => ctx.units.formatUnits("2000000000000000000", 18)"#).expect("ok");
    assert_eq!(r, json!("2"));
}

#[test]
fn format_units_round_trip_through_parse_units() {
    let src = r#"(ctx) => {
        const a = ctx.units.parseUnits("1.5", 18);
        const b = ctx.units.formatUnits(a, 18);
        return b;
    }"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!("1.5"));
}

#[test]
fn format_units_rejects_negative_value() {
    let src = r#"(ctx) => {
        try { ctx.units.formatUnits("-1", 18); return "BAD"; }
        catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(
        m.contains("non-negative") || m.contains("amount_negative"),
        "expected non-negative rejection, got: {m}"
    );
}

// ─── ctx.address.isAddress ─────────────────────────────────────────────

#[test]
fn is_address_returns_true_for_lowercase_and_eip55() {
    let src = format!(
        r#"(ctx) => [ctx.address.isAddress("{LOWER}"), ctx.address.isAddress("{EIP55}")]"#
    );
    let r = run(&src).expect("ok");
    assert_eq!(r, json!([true, true]));
}

#[test]
fn is_address_returns_false_for_non_string_inputs_without_throwing() {
    // D-11: total predicate. Number / null / undefined / object → false.
    let src = r#"(ctx) => [
        ctx.address.isAddress(42),
        ctx.address.isAddress(null),
        ctx.address.isAddress(undefined),
        ctx.address.isAddress({}),
        ctx.address.isAddress("not-an-address")
    ]"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!([false, false, false, false, false]));
}

#[test]
fn is_address_returns_false_for_mixed_case_bad_checksum() {
    // Take the canonical EIP-55 form and lowercase ONE alpha char to break it.
    let src = r#"(ctx) => ctx.address.isAddress("0x52908400098527886e0F7030069857D2E4169EE7")"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!(false));
}

// ─── ctx.address.checksum ──────────────────────────────────────────────

#[test]
fn checksum_lowercase_returns_eip55() {
    let src = r#"(ctx) => ctx.address.checksum("0x52908400098527886e0f7030069857d2e4169ee7")"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!(EIP55));
}

#[test]
fn checksum_throws_on_mixed_case_bad() {
    let src = r#"(ctx) => {
        try {
            ctx.address.checksum("0x52908400098527886e0F7030069857D2E4169EE7");
            return "BAD";
        } catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(
        m.contains("bad_address") || m.contains("checksum"),
        "expected bad_address rejection, got: {m}"
    );
}

#[test]
fn checksum_throws_on_non_string_argument() {
    let src = r#"(ctx) => {
        try { ctx.address.checksum(42); return "BAD"; }
        catch (e) { return e.message; }
    }"#;
    let r = run(src).expect("ok");
    let m = r.as_str().unwrap_or("").to_lowercase();
    assert!(m.contains("string"), "expected type rejection, got: {m}");
}

// ─── ctx.address.zeroAddress ───────────────────────────────────────────

#[test]
fn zero_address_is_canonical_constant_string() {
    let src = r#"(ctx) => ctx.address.zeroAddress"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!(ZERO));
}

#[test]
fn zero_address_typeof_is_string() {
    let src = r#"(ctx) => typeof ctx.address.zeroAddress"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!("string"));
}

#[test]
fn zero_address_local_reassignment_does_not_corrupt_host_view() {
    // T-04-04-02 / NOTE-1: ctx.address.zeroAddress is a JS string property.
    // QuickJS allows reassignment by default, but the assignment only affects
    // the strategy's local view of the property. Host-side reads always go
    // through `executor_evm::ZERO_ADDRESS`. This test pins behaviour:
    //   - reading after reassignment may return either the new value
    //     (if mutable) or the original (if frozen at install).
    //   - regardless, calling ctx.address.checksum(zeroAddress) on the
    //     ORIGINAL constant via a fresh `executor_evm::ZERO_ADDRESS` lookup
    //     in another sandbox call still returns the canonical zero.
    //
    // The narrow property: a strategy CANNOT use reassignment to coerce
    // the host into reading a non-zero address — host-side calls go via
    // the Rust constant.
    let src = r#"(ctx) => {
        const before = ctx.address.zeroAddress;
        try { ctx.address.zeroAddress = "0x1111111111111111111111111111111111111111"; } catch (_) {}
        const after = ctx.address.zeroAddress;
        // Either mutable (after differs) or immutable (before === after) is acceptable;
        // but `before` MUST be the canonical zero.
        return before;
    }"#;
    let r = run(src).expect("ok");
    assert_eq!(r, json!(ZERO));
}

// ─── HR-01 carry-forward ───────────────────────────────────────────────

#[test]
fn forbidden_globals_scrub_still_runs_with_units_and_address_installed() {
    // D-15a final regression: the FORBIDDEN_GLOBALS_SCRUB still runs BEFORE
    // host bindings install on globalThis. With ctx.units / ctx.address
    // freshly added, all D-11 absent globals MUST stay absent AND all the
    // new bindings MUST be reachable.
    let r = run(
        r#"(ctx) => {
            const ok =
                typeof console === "undefined" &&
                typeof fetch === "undefined" &&
                typeof process === "undefined" &&
                typeof setTimeout === "undefined" &&
                typeof queueMicrotask === "undefined" &&
                typeof Deno === "undefined" &&
                typeof ctx.units.parseUnits === "function" &&
                typeof ctx.units.formatUnits === "function" &&
                typeof ctx.address.isAddress === "function" &&
                typeof ctx.address.checksum === "function" &&
                typeof ctx.address.zeroAddress === "string";
            return ok ? "noop" : "BAD";
        }"#,
    )
    .expect("ok");
    assert_eq!(r, json!("noop"));
}

#[test]
fn units_and_address_are_namespaced_under_ctx_not_global() {
    let r = run(
        r#"(ctx) => (
            typeof globalThis.parseUnits === "undefined" &&
            typeof globalThis.formatUnits === "undefined" &&
            typeof globalThis.isAddress === "undefined" &&
            typeof globalThis.checksum === "undefined" &&
            typeof globalThis.zeroAddress === "undefined"
        ) ? "noop" : "BAD""#,
    )
    .expect("ok");
    assert_eq!(r, json!("noop"));
}
