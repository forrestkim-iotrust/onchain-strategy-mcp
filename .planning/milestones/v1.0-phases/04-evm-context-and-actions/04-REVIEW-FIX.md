---
phase: 04-evm-context-and-actions
applied_at: 2026-04-27T00:00:00Z
review_path: .planning/phases/04-evm-context-and-actions/04-REVIEW.md
findings_in_scope: 10
fixed: 10
skipped: 0
status: resolved
---

# Phase 4: Code Review Fix Report

**Applied at:** 2026-04-27
**Source review:** `.planning/phases/04-evm-context-and-actions/04-REVIEW.md`
**Iteration:** 1

**Summary:**
- Findings in scope: 10 (2 BLOCKER + 8 WARNING)
- Fixed: 10
- Skipped: 0
- Workspace tests: **353 passed** (was 349; +2 BR-01/BR-02 regressions, +1 WR-04 unit, +1 WR-08 unit; one IN-* style nit cleaned up)
- Clippy: clean (`cargo clippy --workspace --all-targets -- -D warnings`)

## Fixed Issues

### WR-01: `block_in_place` invoked from inside `spawn_blocking` thread

**Files modified:** `crates/strategy-js/src/sandbox.rs`
**Commit:** `bfd7d5e`
**Applied fix:** Removed `tokio::task::block_in_place(|| handle.block_on(dispatch))` wrapper at all four sites (readContract / erc20 / native_balance / native_block_number). Replaced with direct `handle.block_on(dispatch)` per D-04 spec. The transient runtime fallback (`Builder::new_current_thread()` for unit tests outside any tokio context) is preserved unchanged.
**Verify:** `cargo test -p strategy-js` → 112 passed.

### WR-05: `validate_address` short-circuit subtle / not unit-tested

**Files modified:** `crates/executor-evm/src/action.rs`
**Commit:** `8ffe7e2`
**Applied fix:** Added `body.bytes().all(|b| b.is_ascii_hexdigit())` early-return immediately after stripping the `0x` prefix and before the alpha-case classification. Mirrors `address::checksum`'s gate; defense-in-depth for the lenient EIP-55 path.
**Verify:** `cargo test -p executor-evm` → 65 passed.

### WR-06: Stale comment + `.ok()` swallowing in `tools.rs::strategy_run`

**Files modified:** `crates/executor-mcp/src/tools.rs`
**Commit:** `ccdb496`
**Applied fix:** Replaced `let evm_provider = self.evm_provider().await.ok();` with a `match` that surfaces `evm_provider().await` errors as `-32017 evm_rpc_error` via `map_evm_error(e, &run_id)`. Updated comment to clarify URL/timeout validation already ran at server boot — only the near-impossible reqwest connection-builder failure can reach this site, and we no longer hide it. Added `map_evm_error` to the `errors::*` import list.
**Verify:** `cargo test -p executor-mcp` → 93 passed.

### WR-04: Revert-reason text unsanitized on the wire

**Files modified:** `crates/executor-evm/src/read.rs`
**Commit:** `f11c20d`
**Applied fix:** Added `sanitize_revert_reason(s: &str) -> String` (`pub(crate)`) that:
  - strips ASCII control chars (`\x00-\x1F` including `\n`, `\r`, `\t`, `\x1b`) and `\x7f`,
  - caps length at 256 bytes (truncates at UTF-8 char boundary, appends `…`).
Called from `classify_provider_error` between `try_extract_revert_reason` and `EvmError::Revert` construction. Added a unit test covering control-char stripping, length cap, and the spoof-prefix scenario.
**Verify:** `cargo test -p executor-evm --lib` → 55 passed (1 new).

### WR-08: ABI-arg `address` accepted any case (no checksum gate)

**Files modified:** `crates/executor-evm/src/dyn_abi.rs`, `crates/executor-evm/tests/dyn_abi_roundtrip.rs`
**Commit:** `4540fc8`
**Applied fix:** In `js_value_to_dyn_sol`'s `DynSolType::Address` arm, replaced `Address::from_str(s)` with `crate::action::validate_address(s)?` so address-typed ABI args go through the same lenient EIP-55 validator as the top-level action `address` field. Removed unused `alloy_primitives::Address` import. Added a regression test `address_mixed_case_bad_checksum_rejected_as_abi_arg` asserting `evm encode error: bad_address` is emitted for mixed-case-bad-checksum input.
**Verify:** `cargo test -p executor-evm --test dyn_abi_roundtrip` → 12 passed (1 new).

### WR-07: BigInt rejection in readContract args lacks D-03 stable wording

**Files modified:** `crates/strategy-js/src/sandbox.rs`
**Commit:** `355c13f`
**Applied fix:** Added a pre-walk before `qjs_value_to_json` in `read_contract_host_binding`: iterate the args array, detect `Type::BigInt` at each `args[i]`, and throw the D-03 stable rejection message:
  > `args[i] must be a decimal string, got BigInt — use ctx.units.parseUnits(...) to produce one`
Mirrors `require_string_field`'s builder-side wording. The `qjs_value_to_json` BigInt branch is preserved as the fallback for non-args paths.
**Verify:** `cargo test -p strategy-js` → 112 passed.

### WR-03: `block_number_resolved` payload field absent

**Files modified:** `crates/strategy-js/src/sandbox.rs`
**Commit:** `0a735cc`
**Applied fix:** Added `resolve_block_number(provider, cfg, tag) -> Option<u64>` helper:
  - `BlockTag::Number(n)` → `Some(n)` (verbatim, no extra RPC),
  - `BlockTag::Latest|Pending` → one extra `executor_evm::native::native_block_number(...)` call. On failure, log via `tracing::warn!` and return `None`.
Inserted at all three D-13 payload sites (readContract / erc20 / native_balance). When `Some(n)`, payload gains `block_number_resolved: n`; on failure, the field is omitted (D-13 contract preserved as optional). Updated provider-consume sites to `provider.clone()` so the resolver can run after dispatch.
**Verify:** `cargo test -p strategy-js` → 112 passed. (No anvil-bound test added; resolver is exercised by the existing native/erc20 anvil tests.)

### WR-02: D-02 alloy-isolation wording mismatch (documentation only)

**Files modified:** `crates/strategy-js/Cargo.toml`, `.planning/phases/04-evm-context-and-actions/04-CONTEXT.md`
**Commit:** `f1f14ac`
**Applied fix:** Reworded both the Cargo.toml comment and the D-02 entry in CONTEXT.md to clarify that strategy-js source does not name `alloy` directly (no `use alloy::*`), and that `cargo tree` correctly shows alloy transitively via executor-evm's re-exports of `DynProvider` / `EvmError` — that re-export is the actual contract. No code change.
**Verify:** `cargo test --workspace` → 353 passed (no regression).

### BR-02: ABI 64 KiB cap not enforced at JSON-output gate

**Files modified:** `crates/executor-mcp/src/tools.rs`, `crates/executor-mcp/tests/stdio_handshake.rs`
**Commit:** `902f8da`
**Applied fix:** In `validate_strategy_output`, after the `from_value::<Action>` loop succeeds, walk `actions` and for each `Action::ContractCall(cc)` call `executor_evm::action::dry_run_abi_encode(&cc.abi, &cc.function, &cc.args)`. Errors map to `-32018 strategy_invalid_output` with stable detail prefix `action[{i}] (contract_call): {EvmError::Display}` — wire-safe per MR-01 (no raw alloy / serde text). Added regression test `strategy_run_rejects_hand_built_oversize_abi` that hand-constructs `{kind:"contract_call", abi: <1 MiB>...}` (bypassing the builder) and asserts `-32018` with `abi_oversize` / `evm encode error` in detail.
**Verify:** `cargo test -p executor-mcp --test stdio_handshake strategy_run_rejects_hand_built_oversize_abi` → 1 passed; full executor-mcp suite → 94 passed.

### BR-01: D-12 EVM `data.kind` taxonomy unreachable on production wire

**Files modified:**
  - `crates/executor-evm/src/error.rs` (Decode/Encode `category`: `&'static str` → `Cow<'static, str>`)
  - `crates/executor-evm/src/{action,address,dyn_abi,native,read,units}.rs` (construction sites wrapped with `Cow::Borrowed(...)`; helper fns wrap internally; test extractors switched to `&str` / `as_ref()`)
  - `crates/executor-mcp/src/errors.rs` (test-only EvmError construction sites updated)
  - `crates/strategy-js/src/sandbox.rs` (`classify_message` extended)
  - `crates/executor-mcp/tests/stdio_handshake.rs` (regression test added)
**Commit:** `1aecad0`
**Applied fix:**
  1. **Schema flexibility (per IN-03 carry-forward):** changed `EvmError::Decode { category }` and `EvmError::Encode { category }` from `&'static str` to `Cow<'static, str>`. Existing call sites preserved as zero-cost `Cow::Borrowed("...")`; the re-classification path can use `Cow::Owned(runtime_string)`.
  2. **Re-classification:** in `strategy-js::sandbox::classify_message`, after the existing oom / stack-overflow / interrupted heuristics, added prefix-matching that reconstructs `RuntimeError::Evm(EvmError::*)` from the stable Display strings:
      - `"evm rpc error: timeout"` → `Evm(Timeout)`
      - `"evm rpc error: transport"` → `Evm(Transport { detail_for_log: "<re-thrown from JS>" })`
      - `"evm provider config error"` → `Evm(Config { ... })`
      - `"evm revert: <reason>"` → `Evm(Revert { reason, ... })`
      - `"evm decode error: <category>"` → `Evm(Decode { category: Cow::Owned(...) })`
      - `"evm encode error: <category>"` → `Evm(Encode { category: Cow::Owned(...) })`
     The body is first stripped of an optional leading `Error: ` (QuickJS Error stringification) and then sliced from the rightmost taxonomy prefix occurrence — this peels off builder context (`"ctx.actions.contractCall: ..."`) and host-binding `args[i]:` prefixes so the underlying `EvmError::Display` is matched at the tail.
  3. **Regression test:** `strategy_run_evm_error_surfaces_typed_data_kind` calls `ctx.actions.contractCall` with malformed ABI and asserts `data.kind ∈ {evm_decode_error, evm_rpc_error, evm_revert}` — explicitly NOT `"exception"`. Pre-fix this test would have produced `kind = "exception"` (verified via initial test failure before the slice-from-prefix logic was added).
  4. **Clippy hygiene:** collapsed three nested `if let Some(...) { if let Some(...) { ... } }` payload-mutation blocks introduced by WR-03 into a single tuple destructure; converted the BR-02 builder gate to use `&&`-let chaining.
**Verify:** `cargo test --workspace` → 353 passed; `cargo clippy --workspace --all-targets -- -D warnings` → clean.

## Skipped Issues

None.

---

_Fixed: 2026-04-27_
_Fixer: gsd-code-fixer (automated review-fix workflow)_
_Iteration: 1_
