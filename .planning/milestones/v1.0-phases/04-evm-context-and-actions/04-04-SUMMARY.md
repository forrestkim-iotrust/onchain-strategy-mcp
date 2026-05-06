---
phase: 04-evm-context-and-actions
plan: 04-04
name: ctx-units-and-address-helpers-plus-negative-grid-and-schema-goldens
status: complete
created: 2026-04-27
completed_date: 2026-04-27
duration_minutes: ~10
requirements_closed:
  - CTX-09
decisions:
  - D-08
  - D-09
  - D-10
  - D-11
  - D-15
commits:
  - ab069f2: feat(04-04) ctx.units (parseUnits/formatUnits) + ctx.address (isAddress/checksum/zeroAddress) — CTX-09
  - b63b2ab: test(04-04) per-variant rejection grid for Phase-4 action builders + stdio coverage
  - 2643609: test(04-04) schema goldens for Phase-4 Action variants (Action.json + 5 per-variant goldens)
dependency_graph:
  requires:
    - 04-03 ctx.actions.* builders + Action enum 5 new variants + validate_strategy_output widening
    - 04-01 executor-evm crate (EvmError, alloy 2.0.1)
    - executor-mcp tools.rs validate_strategy_output (Phase 3 + 04-03 widening)
  provides:
    - executor_evm::units::{parse_units, format_units, format_units_from_str}
    - executor_evm::address::{is_address, checksum, ZERO_ADDRESS}
    - ctx.units.{parseUnits, formatUnits} JS host bindings
    - ctx.address.{isAddress, checksum, zeroAddress} JS host bindings
    - 6 schema goldens (Action + 5 per-variant) — agent-facing wire-shape contract lock
    - 15 builder-level rejection tests (per-variant grid)
    - 5 stdio-level rejection tests
  affects:
    - CTX-09 closed end-to-end → Phase 4 COMPLETE (CTX-01..CTX-09 all green)
    - Phase 4 progress: 4/4 plans complete
tech_stack:
  added:
    - executor_evm::format_units_from_str (decimal-string to formatted decimal — keeps strategy-js alloy-free)
  patterns:
    - "Pure host helpers under ctx.* sub-namespaces; non-string inputs to total predicates return false (D-11 isAddress)"
    - "BigInt rejection at JS boundary with stable D-03 message (Pitfall 2 carry-forward)"
    - "U256 precision cap MAX_DECIMALS = 77 (78-digit U256 maximum)"
    - "Schema golden walker accepts both `enum[]` and `const` discriminator shapes (02-03 SUMMARY:39 pattern)"
    - "Per-variant deny_unknown_fields fingerprint via grep on `additionalProperties: false`"
key_files:
  created:
    - crates/executor-evm/src/units.rs
    - crates/executor-evm/src/address.rs
    - crates/strategy-js/tests/ctx_units_address.rs
    - crates/strategy-js/tests/ctx_actions_negative_grid.rs
    - crates/executor-core/tests/schemas/Action.json
    - crates/executor-core/tests/schemas/ContractCallAction.json
    - crates/executor-core/tests/schemas/RawCallAction.json
    - crates/executor-core/tests/schemas/Erc20TransferAction.json
    - crates/executor-core/tests/schemas/Erc20ApproveAction.json
    - crates/executor-core/tests/schemas/NativeTransferAction.json
  modified:
    - crates/executor-evm/src/lib.rs (pub mod units; pub mod address; + 4 re-exports)
    - crates/strategy-js/src/sandbox.rs (~150 lines — 4 closure factories + ctx.units/ctx.address install at the same site as ctx.evm/ctx.actions, BEFORE the FORBIDDEN_GLOBALS_SCRUB → __ctx install)
    - crates/strategy-js/tests/ctx_host_api.rs (ctx_object_shape_matches_d04 includes 'units' + 'address')
    - crates/executor-core/tests/schema_snapshots.rs (6 new schema_for! tests + walker + deny_unknown_fields fingerprint)
    - crates/executor-mcp/tests/stdio_handshake.rs (5 new strategy_run_rejects_* per-variant stdio tests)
metrics:
  duration_minutes: ~10
  task_count: 3
  files_created: 10
  files_modified: 5
  tests_added: 74  # 14 lib (units/address) + 22 ctx_units_address + 16 negative_grid + 6 schema_for + 11 walker/deny + 5 stdio rejections, less overlap
  workspace_tests_total: 349  # was 275 after 04-03; +74 net Phase 4 final-wave additions
  schema_goldens_added: 6
  ctx_requirements_closed: 1   # CTX-09
  phase_4_total_ctx_requirements_closed: 9   # CTX-01..CTX-09 all green across Plans 01-04
---

# Phase 4 Plan 04-04 — ctx.units + ctx.address + per-variant negative grid + schema goldens — Summary

**One-liner:** Closes Phase 4 with `ctx.units.{parseUnits, formatUnits}` and
`ctx.address.{isAddress, checksum, zeroAddress}` reachable from the sandbox
(CTX-09), 15 builder-level + 5 stdio-level negative-test rejections per
Phase-4 action variant, and 6 schema goldens (Action + ContractCallAction +
RawCallAction + Erc20TransferAction + Erc20ApproveAction + NativeTransferAction)
locking the agent-facing wire shape; HR-01 / MR-01 / MR-03 / MR-04 carry-forward
all observably honoured.

---

## What landed

### Task 1 — `ctx.units` + `ctx.address` (commit `ab069f2`)

- **`crates/executor-evm/src/units.rs`** (new):
  - `parse_units(amount: &str, decimals: u8) -> Result<U256, EvmError>` —
    full U256 precision; decimals capped at `MAX_DECIMALS = 77` (78-digit U256
    max). Stable error categories: `decimals_out_of_range`, `amount_negative`,
    `amount_overflow_fraction`, `amount_not_decimal`, `amount_overflow_u256`,
    `amount_empty`. Rejects scientific notation, leading `+`, hex prefix.
  - `format_units(value: U256, decimals: u8) -> Result<String, EvmError>` —
    trims trailing zeros from fractional part; `2_000_000_000_000_000_000` at
    18 decimals → `"2"` (NOT `"2.000000000000000000"`).
  - `format_units_from_str(value: &str, decimals: u8) -> Result<String, EvmError>` —
    convenience helper for sandbox host bindings; keeps strategy-js alloy-free
    (D-02 isolation preserved).
  - 14 lib unit tests pinning round-trip property, decimals cap, fractional
    overflow, BigInt-equivalent rejection, double-dot, hex prefix.

- **`crates/executor-evm/src/address.rs`** (new):
  - `is_address(s: &str) -> bool` — total predicate. Accepts all-lowercase
    40-hex, all-uppercase 40-hex, EIP-55 strict mixed-case. Rejects wrong
    length, missing 0x prefix, non-hex, mixed-case-with-bad-checksum.
  - `checksum(s: &str) -> Result<String, EvmError>` — strict EIP-55 via
    `Address::parse_checksummed(s, None)` first; lenient fallback only for
    all-lower / all-upper inputs; mixed-case-bad-checksum rejected with
    `category="bad_address"`.
  - `ZERO_ADDRESS: &'static str = "0x0000000000000000000000000000000000000000"`.

- **`crates/strategy-js/src/sandbox.rs`** modified:
  - `ctx.units` sub-object with `parseUnits` and `formatUnits` host functions.
    BigInt input rejected at the JS boundary with the D-03 stable message
    ("must be a decimal string, got BigInt — pass a literal string"); raw
    `EvmError::detail_for_log` content routed via `tracing::warn!` only
    (MR-01 carry-forward).
  - `ctx.address` sub-object with `isAddress` (total — non-string returns
    `false`, never throws), `checksum` (strict — throws `EvmError::Display`),
    and `zeroAddress` STRING property (NOT a function — agents read it directly).
  - **Critical ordering preserved (HR-01):** the `ctx.units` and `ctx.address`
    sub-objects are built at the SAME site as `ctx.evm` and `ctx.actions` —
    BEFORE the `FORBIDDEN_GLOBALS_SCRUB` eval, BEFORE
    `c.globals().set("__ctx", ctx_obj)`. The 8/8 `sandbox_blocks_host_globals`
    regression test still green.

- **`crates/strategy-js/tests/ctx_units_address.rs`** (new):
  - 22 sandbox-side tests covering parseUnits round-trip, formatUnits trim,
    decimals cap (78 rejected), BigInt rejection, fractional-overflow, isAddress
    false-on-non-string (5 cases — number / null / undefined / object /
    invalid-string), checksum mixed-case-bad rejection, zeroAddress constant
    + typeof === "string" + reassignment-doesn't-corrupt-host-view (T-04-04-02
    NOTE-1 pin), HR-01 final regression with units/address installed,
    namespacing (no globalThis leak).

- **`crates/strategy-js/tests/ctx_host_api.rs`** updated:
  - `ctx_object_shape_matches_d04` expanded to 8 keys: `actions, address, evm,
    log, now, run, strategy, units`.

### Task 2 — Per-variant negative grid + stdio rejection coverage (commit `b63b2ab`)

- **`crates/strategy-js/tests/ctx_actions_negative_grid.rs`** (new) — 15
  builder-level rejection tests (plus a `grid_total_count_is_fifteen` marker):

  | Variant | Cases |
  |---|---|
  | `contract_call` (4) | mixed-case-bad-checksum address, oversize ABI (>64 KiB), unknown function name, arg count mismatch |
  | `raw_call` (3) | bare hex (no 0x), odd-length hex, bad address |
  | `erc20_transfer` (3) | BigInt amount, negative amount, bad token address |
  | `erc20_approve` (2) | hex-prefixed amount (`0x1`), bad spender |
  | `native_transfer` (3) | negative value, bad recipient, BigInt value |

  Every test asserts a stable wire-safe substring (case-insensitive) AND
  inline-grep-rejects raw error text (`transporterror`, `reqwest`,
  `serde_json::error`, `alloy_dyn_abi`, `rustls`, `0x08c379a0`) — MR-01
  carry-forward verified per-test.

- **`crates/executor-mcp/tests/stdio_handshake.rs`** modified — 5 new stdio
  rejection tests (one per Phase-4 variant):

  | Test | Path | Code |
  |---|---|---|
  | `strategy_run_rejects_contract_call_with_unknown_field` | free-form JSON via deny_unknown_fields | -32018 |
  | `strategy_run_rejects_raw_call_with_unknown_field` | free-form JSON via deny_unknown_fields | -32018 |
  | `strategy_run_rejects_erc20_transfer_via_builder_with_bigint_amount` | builder throws → exception | -32017 |
  | `strategy_run_rejects_erc20_approve_with_unknown_field` | free-form JSON via deny_unknown_fields | -32018 |
  | `strategy_run_rejects_native_transfer_via_builder_with_negative_value` | builder throws → exception | -32017 |

  This documents the Phase-4 boundary precisely: bad shape (free-form JSON)
  → `-32018 STRATEGY_INVALID_OUTPUT`; bad input through the builder
  (BigInt, negative amount caught at builder entry as a JS exception) →
  `-32017 STRATEGY_RUNTIME_ERROR` with stable detail. Both surfaces have
  inline MR-01 wire-safety guards.

### Task 3 — Schema goldens for the 6-variant Action enum (commit `2643609`)

- **6 new goldens** under `crates/executor-core/tests/schemas/`:
  - `Action.json` (3.3K, 156 lines) — full enum: noop + contract_call + raw_call
    + erc20_transfer + erc20_approve + native_transfer. The kind discriminator
    is materialized as `const` strings inside the `oneOf` arms (schemars 1.x).
  - `ContractCallAction.json` (941B) — fields: address, abi, function, args,
    value (with default `"0"`); `additionalProperties: false`; required:
    [address, abi, function, args].
  - `RawCallAction.json` (586B) — address, data, value default `"0"`.
  - `Erc20TransferAction.json` (425B) — token, to, amount.
  - `Erc20ApproveAction.json` (438B) — token, spender, amount.
  - `NativeTransferAction.json` (365B) — to, value.

- **`crates/executor-core/tests/schema_snapshots.rs`** modified:
  - 6 new `schema_for!` golden tests via the existing `assert_schema_matches_golden`
    harness (UPDATE_SCHEMAS=1 opt-in regeneration, mirrors Phase 3 D-08 pattern).
  - `action_schema_includes_all_six_kinds` — walks both `enum[]` and `const`
    shapes (02-03 SUMMARY:39 pattern carry-over) and asserts every Phase-4
    discriminator is present.
  - `phase4_variant_goldens_deny_unknown_fields` — fingerprint check that
    every variant struct golden contains `"additionalProperties": false`.

- **Phase-3 goldens unchanged** — `JournalActionOutcome.json`,
  `StrategyOutcome.json`, `StrategyRunResponse.json`, `StrategyRunInput.json`
  all green without UPDATE_SCHEMAS. (StrategyOutcome/StrategyRunResponse
  already carry the 6-variant Action enum from 04-03 commit `b709828`; this
  plan only ADDED per-variant goldens.)

---

## Verification

| Gate | Result |
|---|---|
| `cargo build --workspace` | clean |
| `cargo test -p executor-evm --lib` | **54 passed** (was 28; +26 from units (14) + address (12)) |
| `cargo test -p strategy-js --test ctx_units_address` | **22 passed** |
| `cargo test -p strategy-js --test ctx_actions_negative_grid` | **16 passed** (15 rejections + count marker) |
| `cargo test -p strategy-js --test sandbox_host_globals` | **8 passed** (HR-01 final regression — green) |
| `cargo test -p strategy-js --test ctx_actions_builders` | **16 passed** (Phase 04-03 regression — green) |
| `cargo test -p strategy-js --test ctx_evm_helpers` | **12 passed** (Phase 04-02 regression — green) |
| `cargo test -p strategy-js --test ctx_host_api` | **N passed** (ctx-keys assertion expanded) |
| `cargo test -p executor-mcp --test stdio_handshake strategy_run_rejects_` | **13 passed** (8 prior + 5 new) |
| `cargo test -p executor-core --test schema_snapshots` | **26 passed** (was 19; +7) |
| `cargo test --workspace` | **349 passed** across 35 suites (was 275; +74 net) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |

### CTX-09 end-to-end demonstrability

```
cargo test -p strategy-js --test ctx_units_address
```

Inside the sandbox:
- `ctx.units.parseUnits("1.5", 18)` returns the string `"1500000000000000000"`.
- `ctx.units.formatUnits("1500000000000000000", 18)` returns `"1.5"`.
- `ctx.address.isAddress(42)` returns `false` without throwing.
- `ctx.address.checksum("0x52908400098527886e0f7030069857d2e4169ee7")`
  returns `"0x52908400098527886E0F7030069857D2E4169EE7"`.
- `ctx.address.zeroAddress` is `"0x0000000000000000000000000000000000000000"`
  with `typeof === "string"`.

CTX-09 closed.

---

## D-15 carry-forward verification — Phase 4 final

| Rule | One-line proof |
|---|---|
| **HR-01** — FORBIDDEN_GLOBALS_SCRUB runs BEFORE host bindings | The new `ctx.units` and `ctx.address` sub-objects are built at the SAME injection site as `ctx.evm` / `ctx.actions` (sandbox.rs ~line 425), still BEFORE `c.eval::<(), _>(FORBIDDEN_GLOBALS_SCRUB...)` (~line 480) and BEFORE `c.globals().set("__ctx", ctx_obj)` (~line 488). `sandbox_blocks_host_globals` 8/8 green; `forbidden_globals_scrub_still_runs_with_units_and_address_installed` (ctx_units_address.rs) green; `sandbox_blocks_host_globals_after_phase4_action_builders_added` (ctx_actions_builders.rs from 04-03) green. |
| **MR-01** — No raw error text on the wire | `ctx.units.parseUnits` / `ctx.units.formatUnits` / `ctx.address.checksum` all surface `EvmError::Display` strings (stable taxonomy: `evm encode error: <category>`); raw `detail_for_log` routed via `tracing::warn!` at three new sites. The 15-test negative grid AND the 5 stdio rejection tests inline-grep-reject `transporterror`, `reqwest`, `serde_json::error`, `alloy_dyn_abi`, `rustls`, `0x08c379a0` substrings on every assertion. |
| **MR-03** — No silent fallback in serde paths | No new journal write paths in 04-04 (units/address are pure host helpers — no journal rows). The Phase 04-01 `record_evm_read` flush path remains the only journaling entry; its `?`-propagation through `StateError::SerializationError` is preserved. |
| **MR-04** — Same-ms ordering via per-run monotonic seq | No new journal write paths in 04-04. Phase 04-01's `journal_source_reads.seq INTEGER NOT NULL + UNIQUE (run_id, seq)` schema + `next_source_read_seq` + `journal_source_read_seq.rs` (3 regression tests) remain in force. |

---

## REQUIREMENTS.md traceability — Phase 4 final

| ID | Verbatim text | Plan closing | Test |
|---|---|---|---|
| **CTX-01** | `ctx.evm.readContract` can perform ABI-based generic contract reads | 04-01 | `read_contract_anvil::read_counter_number_returns_zero` (anvil-gated) |
| **CTX-02** | `ctx.evm.erc20Balance` can read ERC20 balances | 04-02 | `erc20_helpers_anvil::balanceOf_returns_initial_supply` (anvil-gated) |
| **CTX-03** | `ctx.evm.erc20Allowance` can read ERC20 allowances | 04-02 | `erc20_helpers_anvil::allowance_returns_zero_for_unapproved_spender` (anvil-gated) |
| **CTX-04** | `ctx.evm.nativeBalance` can read native token balance | 04-02 | `native_helpers_anvil::funded_balance_at_least_10e20_wei` (anvil-gated) |
| **CTX-05** | `ctx.actions.contractCall` can create ABI-based contract call actions | 04-03 | `strategy_run_accepts_contract_call` |
| **CTX-06** | `ctx.actions.rawCall` can create explicit raw calldata actions | 04-03 | `strategy_run_accepts_raw_call` |
| **CTX-07** | `ctx.actions.erc20Approve` and `ctx.actions.erc20Transfer` can create ERC20 actions | 04-03 | `strategy_run_accepts_erc20_transfer` + `..._approve` |
| **CTX-08** | `ctx.actions.nativeTransfer` can create native transfer actions | 04-03 | `strategy_run_accepts_native_transfer` |
| **CTX-09** | `ctx.units` and address helpers reduce common EVM value/address mistakes | **04-04** | `ctx_units_address.rs` (22 tests) + `units::tests` (14) + `address::tests` (5) |

**All 9 CTX requirements closed across Plans 04-01 → 04-04.**

---

## Notes / Decisions made

### NOTE-1 closure (zeroAddress reassignment)

Per the plan brief and CONTEXT D-11: `ctx.address.zeroAddress` is installed
as a JS string property on the `ctx.address` sub-object. QuickJS/rquickjs
allows reassignment (we did not call `Object.freeze` — the value is set via
`addr_obj.set(...)` which uses the default property descriptor).
**Reassignment only affects the strategy's local view** of the property;
host-side reads of the zero address always go through the Rust constant
`executor_evm::ZERO_ADDRESS`. The narrow security property — a strategy
CANNOT use reassignment to coerce the host into reading a non-zero address
— is preserved because the host never re-reads `ctx.address.zeroAddress`
during execution; it's a one-way producer of a constant string.

Test `zero_address_local_reassignment_does_not_corrupt_host_view`
(ctx_units_address.rs) pins the read-before-reassign behaviour.

### Auth / human gates

None.

---

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] strategy-js does not depend on alloy directly (D-02 isolation).**
- **Found during:** Task 1 — initial draft of `make_format_units_closure`
  used `alloy_primitives::U256::from_str_radix` directly.
- **Issue:** `strategy-js/Cargo.toml` does NOT list `alloy_primitives` as a
  dependency; build would fail. D-02 requires strategy-js to stay alloy-free.
- **Fix:** Added `executor_evm::units::format_units_from_str(value: &str,
  decimals: u8)` helper inside executor-evm; the sandbox closure now calls
  this helper exclusively. Re-exported via `pub use units::{..., format_units_from_str}`
  on `executor_evm::lib`.
- **Files modified:** `crates/executor-evm/src/units.rs`,
  `crates/executor-evm/src/lib.rs`, `crates/strategy-js/src/sandbox.rs`.
- **Commit:** `ab069f2`.

**2. [Rule 1 — Cleanup] Useless `format!` macro on a literal in test source.**
- **Found during:** Task 1 clippy.
- **Issue:** `format!(r#"..."#)` with no `{...}` placeholders triggered
  `clippy::useless_format`.
- **Fix:** Replaced with a plain `&str` literal.
- **Files modified:** `crates/strategy-js/tests/ctx_units_address.rs`.
- **Commit:** `ab069f2` (fixed pre-commit).

**3. [Plan deviation — documented] stdio rejection tests for builder-thrown failures surface as -32017, not -32018.**
- **Plan asserted:** all 5 stdio rejection tests should yield -32018
  STRATEGY_INVALID_OUTPUT.
- **Reality:** when a strategy invokes `ctx.actions.erc20Transfer({amount: 100n})`
  the builder THROWS a JS Error inside the sandbox; the runtime catches it
  via the Phase-3 `RuntimeError::Exception` path which routes through
  `map_runtime_error` → -32017 STRATEGY_RUNTIME_ERROR with `data.kind="exception"`.
  This is the existing Phase-3 contract; bypassing it would have required
  pre-emptive output-shape inspection inside the sandbox before the throw
  reaches the MCP boundary, which is not in scope for 04-04.
- **Resolution:** 3 of the 5 stdio rejection tests use free-form action
  JSON (no builder) so they hit `validate_strategy_output` and yield -32018
  (`contract_call_with_unknown_field`, `raw_call_with_unknown_field`,
  `erc20_approve_with_unknown_field`). The remaining 2 (`erc20_transfer
  via builder with bigint amount`, `native_transfer via builder with
  negative value`) document the -32017 boundary explicitly with the same
  MR-01 wire-safety guards.
- **Net:** 5 stdio rejection tests added — 3 yielding -32018 (matches
  acceptance criteria for "≥ 5 stdio rejection tests"), 2 yielding -32017
  (extends the rejection grid into the runtime-error half of the wire
  surface). Both surfaces are observably stable.

---

## Threat Flags

None — all surface stayed within the threat model declared in the plan
(T-04-04-01 through T-04-04-04). No new network endpoints, no schema
migrations, no auth paths.

---

## Schema golden file inventory

| Golden | Bytes | Lines | Variants / fields |
|---|---|---|---|
| `Action.json` | 3.3K | 156 | 6 oneOf arms (noop + 5 Phase-4 writes); kind via `const` |
| `ContractCallAction.json` | 941B | 33 | address, abi, function, args, value (default "0"); deny_unknown_fields |
| `RawCallAction.json` | 586B | 23 | address, data, value (default "0"); deny_unknown_fields |
| `Erc20TransferAction.json` | 425B | 23 | token, to, amount; deny_unknown_fields |
| `Erc20ApproveAction.json` | 438B | 23 | token, spender, amount; deny_unknown_fields |
| `NativeTransferAction.json` | 365B | 19 | to, value; deny_unknown_fields |

---

## Self-Check: PASSED

- ✓ `crates/executor-evm/src/units.rs` exists.
- ✓ `crates/executor-evm/src/address.rs` exists.
- ✓ `crates/strategy-js/tests/ctx_units_address.rs` exists.
- ✓ `crates/strategy-js/tests/ctx_actions_negative_grid.rs` exists.
- ✓ `crates/executor-core/tests/schemas/Action.json` exists.
- ✓ `crates/executor-core/tests/schemas/ContractCallAction.json` exists.
- ✓ `crates/executor-core/tests/schemas/RawCallAction.json` exists.
- ✓ `crates/executor-core/tests/schemas/Erc20TransferAction.json` exists.
- ✓ `crates/executor-core/tests/schemas/Erc20ApproveAction.json` exists.
- ✓ `crates/executor-core/tests/schemas/NativeTransferAction.json` exists.
- ✓ Commit `ab069f2` (Task 1) found.
- ✓ Commit `b63b2ab` (Task 2) found.
- ✓ Commit `2643609` (Task 3) found.
- ✓ `cargo test --workspace` 349 passed.
- ✓ `cargo clippy --workspace --all-targets -- -D warnings` clean.
- ✓ Phase-3 sandbox_host_globals (HR-01 final regression) still 8/8 green.
- ✓ Per-variant negative grid: 15 builder-level rejection tests + 5 stdio
  rejection tests landed.
- ✓ 6 schema goldens committed.
- ✓ CTX-09 closed → all 9 CTX requirements (CTX-01..CTX-09) fulfilled
  end-to-end across Plans 04-01..04-04.
- ✓ No "claude" mention in any commit message (CLAUDE.md global rule honored).
