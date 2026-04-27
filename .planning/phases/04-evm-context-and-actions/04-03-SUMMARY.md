---
phase: 04-evm-context-and-actions
plan: 04-03
name: contract-call-raw-call-erc20-and-native-action-builders
status: complete
completed_at: 2026-04-27
requirements_closed:
  - CTX-05
  - CTX-06
  - CTX-07
  - CTX-08
decisions:
  - D-08
  - D-09
  - D-15
  - D-16
commits:
  - b709828: feat(04-03) Action enum 5 new variants + phase4_emittable + 64 KiB ABI cap + builder validators
  - e05a16c: feat(04-03) ctx.actions.{contractCall,rawCall,erc20Transfer,erc20Approve,nativeTransfer} builders + validate_strategy_output widening
  - 9d7878f: test(04-03) flip strategy_run_rejects_phase4_action_kind -> accepts_contract_call + per-variant accept tests (D-16)
key_files:
  created:
    - crates/executor-evm/src/action.rs
    - crates/strategy-js/tests/ctx_actions_builders.rs
  modified:
    - crates/executor-core/src/schema/action.rs
    - crates/executor-core/tests/schemas/StrategyOutcome.json
    - crates/executor-core/tests/schemas/StrategyRunResponse.json
    - crates/executor-evm/src/lib.rs
    - crates/executor-mcp/src/tools.rs
    - crates/executor-mcp/src/validation.rs
    - crates/strategy-js/src/sandbox.rs
    - crates/executor-mcp/tests/stdio_handshake.rs
verification:
  cargo_test_workspace:
    before: 232
    after: 275
    delta: +43
  cargo_clippy: clean (-D warnings)
  d16_flip_complete: true
  legacy_test_name_grep: 0 matches for `strategy_run_rejects_phase4_action_kind`
---

## Plan 04-03 — Contract call, raw call, ERC20, and native action builders

Closes the Phase-4 write side: 5 new Action variants on the wire, JS-side
builders for each, validator widening so the strategy_run handler accepts
the new kinds, and the D-16 flip of the Phase-3 reject placeholder.

### Tasks delivered

**Task 1 — Action enum + per-variant validators (`b709828`)**

Extended `executor-core::schema::action::Action` with 5 new variants:
`ContractCall`, `RawCall`, `Erc20Transfer`, `Erc20Approve`, `NativeTransfer`.
All carry `serde(deny_unknown_fields)` and a `phase4_emittable()` future-lock
gate (Phase-3's `phase3_emittable` boundary preserved — Phase-3 strategies
still see only `Noop`). `MAX_ABI_BYTES = 65_536` (D-08 cap). Per-variant
validators in `executor-evm::action`:

- `validate_address` — lenient EIP-55: accept all-lowercase or correct
  EIP-55; reject mixed-case-with-bad-checksum.
- `validate_calldata` — `^0x[0-9a-fA-F]*$` plus `Bytes::from_str` check.
- `validate_decimal_amount` — U256 `from_str_radix(s, 10)`; rejects
  negatives, hex prefix, scientific notation, leading `+`.
- `validate_abi_size` — 64 KiB cap.
- `dry_run_abi_encode` — D-09 sanity: parses ABI + encodes args via dyn-abi
  to surface bad shapes early; encoded bytes are discarded (Phase 5 owns
  canonical encoding).

`StrategyOutcome.json` and `StrategyRunResponse.json` schema goldens
regenerated to include the 5 new variants. Per-variant goldens land in 04-04.

**Task 2 — JS-side builders + validator widening (`e05a16c`)**

5 new `ctx.actions.*` host bindings injected into `Sandbox::execute` at
the SAME site as the existing `ctx.actions.noop` binding — i.e. AFTER the
`FORBIDDEN_GLOBALS_SCRUB` runs (D-15 HR-01 carry-forward preserved).

Each builder validates inputs via `executor_evm::action::*` and returns
the wire-shape JSON object the runtime captures into the action queue.
BigInt amount/value inputs are rejected at builder entry with the stable
D-03 decimal-string message (RESEARCH pitfall 2) — not a confused JSON
conversion error. `abi` accepts JSON-string OR JS-array form (mirrors
readContract D-05).

`executor-mcp::validation::validate_action_kind_allowlisted` exposed;
allowlist = `{noop, contract_call, raw_call, erc20_transfer, erc20_approve,
native_transfer}`. Non-allowlisted kinds get -32018 INVALID_OUTPUT with
stable detail (MR-01 carry-forward — no raw error text on wire).

`validate_strategy_output` in tools.rs now pre-checks the kind allowlist
per element BEFORE serde, then `?`-propagates serde errors (MR-03
carry-forward — no silent fallback).

15 new integration tests in `ctx_actions_builders.rs`: 5 positive
round-trips + per-builder rejections + HR-01 regression. validation
unit tests +2 (allowlist accept / reject).

**Task 3 — D-16 stdio flip (`9d7878f`)**

The Phase-3 placeholder `strategy_run_rejects_phase4_action_kind` test is
replaced by per-variant accept tests:

- `strategy_run_accepts_contract_call`
- `strategy_run_accepts_raw_call`
- `strategy_run_accepts_erc20_transfer`
- `strategy_run_accepts_erc20_approve`
- `strategy_run_accepts_native_transfer`

Phase-3 spirit (rejecting unknown kinds) preserved by:
- `strategy_run_rejects_unknown_action_kind` — `kind: "multi_call"` still
  rejected (not in Phase-4 allowlist).
- `strategy_run_rejects_contract_call_with_bad_address` — `deny_unknown_fields`
  + lenient-EIP-55 still rejects malformed inputs.

Verified `! grep -q strategy_run_rejects_phase4_action_kind` against
stdio_handshake.rs (0 matches).

### Verification

```
cargo test --workspace        : 275 passed (was 232; +43 net)
cargo clippy --workspace ...  : clean (-D warnings)
D-16 grep guard               : 0 matches (legacy name fully removed)
```

### D-15 carry-forward status

| Anti-pattern | Enforcement in this plan |
|---|---|
| HR-01 (scrub before bindings) | New ctx.actions.* bindings installed at same site as ctx.actions.noop, AFTER scrub. Regression test `sandbox_blocks_host_globals` still 8/8 green. |
| MR-01 (no raw error on wire) | All builder/validator errors return stable taxonomy strings; alloy/serde raw text never reaches `data.detail`. |
| MR-03 (no silent serde fallback) | `validate_strategy_output` propagates serde errors via `?` after allowlist pre-check. |
| MR-04 (monotonic seq on journal_source_reads) | Already in place from 04-01; this plan adds no new journal write paths. |

### Requirements closed

- **CTX-05** — `ctx.actions.contractCall` available; round-trips via
  `strategy_run_accepts_contract_call`.
- **CTX-06** — `ctx.actions.rawCall` available; round-trips via
  `strategy_run_accepts_raw_call`.
- **CTX-07** — `ctx.actions.erc20Approve` AND `ctx.actions.erc20Transfer`
  available; round-trips via the two corresponding accept tests.
- **CTX-08** — `ctx.actions.nativeTransfer` available; round-trips via
  `strategy_run_accepts_native_transfer`.

### Out of scope (this plan)

- Per-variant Action schema goldens (lands in 04-04 T3)
- Comprehensive negative-grid stdio tests (lands in 04-04 T2)
- `ctx.units.*` and `ctx.address.*` host bindings (lands in 04-04 T1)
- Action broadcast / signer / simulation (Phase 5)

### Notes for downstream

- Phase 5 inherits the `dry_run_abi_encode` discard pattern; canonical
  encoding lives there. Phase 4 only proves shape correctness.
- The lenient EIP-55 policy (lowercase OR correct checksum, but not
  mixed-case-with-bad-checksum) is intentional. If Phase 5 wants stricter
  policy, gate it at the simulation layer, not at builder entry.
