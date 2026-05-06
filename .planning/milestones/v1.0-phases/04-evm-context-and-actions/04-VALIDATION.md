---
phase: 04
slug: evm-context-and-actions
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-27
updated: 2026-04-27
---

# Phase 04 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Mirrors `03-VALIDATION.md` shape; populated from each PLAN.md `<verification><automated>` block + acceptance criteria.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust built-in) + `tokio::test` for stdio integration; `--features anvil-tests` for alloy-spawned anvil integration |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` + `executor-evm/Cargo.toml [features]` |
| **Quick run command** | `cargo test -p executor-evm --lib && cargo test -p strategy-js && cargo test -p executor-core --test schema_snapshots && cargo test -p executor-mcp --test stdio_handshake strategy_run_` |
| **Anvil-gated command** | `cargo test --workspace --features anvil-tests` (CI / dev with foundry installed) |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15–45 seconds (default; no anvil). With `--features anvil-tests` add ~10–30s for anvil spawn + ERC20/Counter deploy/read tests. Phase-3 baseline ~175 tests in ~5s; Phase-4 net additions: ~7 dyn_abi + ~6 read_contract anvil + ~10 erc20 anvil + ~3 native anvil + ~13 ctx_evm_helpers + ~12 ctx_actions_builders + ~15 ctx_actions_negative_grid + ~6 schema goldens + ~10 stdio rejections + ~6 strategy_run_accepts_* + ~14 ctx_units_address + ~4 journal_source_read_seq ≈ 106 new. Workspace target ~280 tests after Phase 4. |

---

## Sampling Rate

- **After every task commit:** `cargo test -p <crate-touched>` (latency ≤ 10s for default; ≤ 45s with anvil if relevant).
- **After every plan wave:** `cargo test --workspace`. With anvil gates, also `--features anvil-tests` once per wave on machines with foundry.
- **Before `/gsd-verify-work`:** Full suite + `cargo clippy --workspace --all-targets -- -D warnings` must be green.
- **Max feedback latency:** ~45 seconds (clippy + tests + anvil-gated reads).

---

## Per-Task Verification Map

*Populated from each PLAN.md `<verification><automated>` block. File-Exists column reflects intent: `✅` if the test file already exists in the repo; `❌ W0` if the same plan's first task creates it.*

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 04-01-T1 | 04-01 | 1 | CTX-01 | T-04-01-04 (no Provider in JS), T-04-01-06 (MR-04 ordering) | executor-evm crate scaffold + alloy 2.0 + dyn-abi BigInt convention + journal_source_reads.seq column | unit (rust) + cargo tree | `cargo build -p executor-evm && cargo test -p executor-evm --lib && cargo test -p executor-evm --test dyn_abi_roundtrip && cargo test -p executor-state --test journal_source_read_seq && cargo build -p executor-evm --features anvil-tests && cargo clippy -p executor-evm -p executor-state --all-targets -- -D warnings && cargo tree -p executor-evm \| grep '^alloy v' \| grep -E '\\b2\\.0\\.'` | ❌ W0 (whole crate is new + new test files) | ⬜ pending |
| 04-01-T2 | 04-01 | 1 | CTX-01 | T-04-01-01..03 (DoS / decode / wire), T-04-01-08 (mutex discipline) | read_contract eth_call lifecycle + ExecutorServer lazy provider + [evm] config + -32017 evm_* taxonomy | integration (anvil) + unit | `cargo test -p executor-evm --features anvil-tests --test read_contract_anvil && cargo test -p executor-evm --lib && cargo test -p executor-mcp --lib config:: errors::map_runtime_error_classifies_evm_kinds && cargo build -p executor-mcp && cargo clippy -p executor-evm -p executor-mcp --all-targets -- -D warnings` | ❌ W0 (anvil-test file new; counter.hex new) | ⬜ pending |
| 04-01-T3 | 04-01 | 1 | CTX-01 | T-04-01-05 (HR-01 ordering), T-04-01-09 (anvil skip) | ctx.evm.readContract host binding + HR-01 scrub-before-binding regression | unit (rust) | `cargo test -p strategy-js --test ctx_evm_read_contract && cargo test -p strategy-js --test sandbox_host_globals && cargo test -p strategy-js && cargo clippy --workspace --all-targets -- -D warnings` | ❌ W0 (test file new) | ⬜ pending |
| 04-02-T1 | 04-02 | 2 | CTX-02, CTX-03, CTX-04 | T-04-02-02 (public read), T-04-02-04 (HR-01 future) | erc20 + native helper modules + anvil ERC20/native integration tests | unit + integration (anvil) | `cargo test -p executor-evm --lib erc20:: && cargo test -p executor-evm --features anvil-tests --test erc20_helpers_anvil --test native_helpers_anvil && cargo clippy -p executor-evm --all-targets -- -D warnings` | ❌ W0 (erc20.hex + 2 anvil test files new) | ⬜ pending |
| 04-02-T2 | 04-02 | 2 | CTX-02, CTX-03, CTX-04 | T-04-02-01 (alias divergence), T-04-02-03 (MR-04 multi-helper seq) | ctx.evm.readErc20.* + ctx.evm.readNative.* + 3 flat aliases + journal records | unit + integration (anvil) | `cargo test -p strategy-js --test ctx_evm_helpers && cargo test -p strategy-js --test sandbox_host_globals && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | ❌ W0 (ctx_evm_helpers.rs new) | ⬜ pending |
| 04-03-T1 | 04-03 | 3 | CTX-05, CTX-06, CTX-07, CTX-08 | T-04-03-02 (ABI cap), T-04-03-03 (phase4 gate) | Action enum 6 variants + per-variant structs + phase4_emittable + 64 KiB cap + builder validators | unit (rust) | `cargo test -p executor-core --lib schema::action:: && cargo test -p executor-evm --lib action:: && cargo clippy -p executor-core -p executor-evm --all-targets -- -D warnings` | ❌ W0 (action.rs in executor-evm new) | ⬜ pending |
| 04-03-T2 | 04-03 | 3 | CTX-05, CTX-06, CTX-07, CTX-08 | T-04-03-01 (allowlist), T-04-03-04 (lenient address), T-04-03-05 (MR-01 wire) | ctx.actions builders + validate_strategy_output widening + BigInt rejection | unit (rust) | `cargo test -p strategy-js --test ctx_actions_builders && cargo test -p strategy-js --test sandbox_host_globals && cargo test -p executor-mcp --lib validation:: && cargo clippy --workspace --all-targets -- -D warnings` | ❌ W0 (ctx_actions_builders.rs new) | ⬜ pending |
| 04-03-T3 | 04-03 | 3 | CTX-05 | T-04-03-06 (D-16 traceability) | Phase-3 reject-test FLIP + per-variant accept stdio tests | integration (rust + stdio) | `cargo test -p executor-mcp --test stdio_handshake strategy_run_accepts_ strategy_run_rejects_ && cargo test -p executor-mcp --test stdio_handshake -- --skip strategy_run_rejects_phase4_action_kind` | ✅ (extends Phase-3 stdio_handshake.rs) | ⬜ pending |
| 04-04-T1 | 04-04 | 4 | CTX-09 | T-04-04-01 (decimals cap), T-04-04-02 (ZERO_ADDRESS const) | ctx.units + ctx.address — pure-fn helpers + sandbox bindings | unit (rust) | `cargo test -p executor-evm --lib units:: address:: && cargo test -p strategy-js --test ctx_units_address && cargo test -p strategy-js --test sandbox_host_globals && cargo clippy --workspace --all-targets -- -D warnings` | ❌ W0 (units.rs / address.rs / ctx_units_address.rs new) | ⬜ pending |
| 04-04-T2 | 04-04 | 4 | CTX-05..CTX-08 (negative coverage) | T-04-04-03 (stable wire), MR-01 carry-forward | Per-variant rejection grid (15 builder + 5 stdio) | unit + integration (rust) | `cargo test -p strategy-js --test ctx_actions_negative_grid && cargo test -p executor-mcp --test stdio_handshake strategy_run_rejects_ && cargo test --workspace` | ❌ W0 (ctx_actions_negative_grid.rs new) | ⬜ pending |
| 04-04-T3 | 04-04 | 4 | CTX-05..CTX-08 (golden lock) | T-04-04-04 (golden opt-in) | Schema goldens — Action.json + 5 per-variant | unit + golden | `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots && cargo test -p executor-core --test schema_snapshots` | ❌ W0 (5 new + Action.json regen) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

**Sampling continuity:** Every task has an `<automated>` cargo command. No 3 consecutive tasks lack automated verify. Wave 1 covers 3 tasks, Wave 2 covers 2, Wave 3 covers 3, Wave 4 covers 3. Total feedback latency for full default suite stays ≤ ~15s; with anvil gates ≤ ~45s.

---

## Wave 0 Requirements

Wave 0 fixture/test files are created **as part of the same plan that needs them** (no separate Wave 0 plan — Phase 4 scope folds Wave 0 into each plan's first task). The full Wave 0 set:

- [ ] `crates/executor-evm/Cargo.toml` — new crate manifest. Created by Plan **04-01 Task 1**.
- [ ] `crates/executor-evm/src/{lib.rs, config.rs, error.rs, provider.rs, dyn_abi.rs, read.rs}` — crate scaffold. Created by Plan **04-01 Task 1+2**.
- [ ] `crates/executor-evm/src/{erc20.rs, native.rs}` — Plan **04-02 Task 1**.
- [ ] `crates/executor-evm/src/{action.rs}` — Plan **04-03 Task 1**.
- [ ] `crates/executor-evm/src/{units.rs, address.rs}` — Plan **04-04 Task 1**.
- [ ] `crates/executor-evm/tests/common/{mod.rs, anvil_fixture.rs}` — Plan **04-01 Task 1**.
- [ ] `crates/executor-evm/tests/fixtures/{counter.hex, erc20.hex}` — Plans **04-01 Task 2** + **04-02 Task 1** (committed bytecode).
- [ ] `crates/executor-evm/tests/{dyn_abi_roundtrip.rs, read_contract_anvil.rs, erc20_helpers_anvil.rs, native_helpers_anvil.rs}` — across Plans **04-01 / 04-02**.
- [ ] `crates/strategy-js/tests/{ctx_evm_read_contract.rs, ctx_evm_helpers.rs, ctx_actions_builders.rs, ctx_actions_negative_grid.rs, ctx_units_address.rs}` — across Plans **04-01 / 04-02 / 04-03 / 04-04**.
- [ ] `crates/executor-state/tests/journal_source_read_seq.rs` — Plan **04-01 Task 1**.
- [ ] `crates/executor-core/tests/schemas/{Action.json (regen), ContractCallAction.json, RawCallAction.json, Erc20TransferAction.json, Erc20ApproveAction.json, NativeTransferAction.json}` — Plan **04-04 Task 3** via `UPDATE_SCHEMAS=1`.
- [ ] `Cargo.toml` (root) — `members` array updated to include `crates/executor-evm`. Updated by Plan **04-01 Task 1**.

`wave_0_complete: true` is set by the executor when all items exist on disk and their owning task's `<automated>` command exits 0.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| _none expected_ | | | All Phase-4 surfaces are observable via `cargo test --workspace [--features anvil-tests]`. |

The `--features anvil-tests` integration tests require `anvil` (foundry) on PATH. Developers without foundry can:
1. Install: `curl -L https://foundry.paradigm.xyz | bash && foundryup`.
2. OR set `ANVIL_RPC_URL` env var pointing at an externally-managed devnet (Hardhat node, Reth dev mode, etc.).
3. OR skip anvil-gated tests in default `cargo test --workspace`; the AnvilFixture cleanly returns `None` when anvil missing (no panic, no failed test — eprintln + early return).

---

## Phase Requirements → Test Map

| Req ID | Behavior | Owning Plan / Task | Automated Command | File Exists |
|--------|----------|-------------------|-------------------|-------------|
| **CTX-01** | `ctx.evm.readContract` performs ABI-based generic contract reads | 04-01 T2 (read_contract anvil deploy + read) + 04-01 T3 (sandbox host binding + journal evm_read row) | `cargo test -p executor-evm --features anvil-tests --test read_contract_anvil read_counter_number_returns_zero && cargo test -p strategy-js --test ctx_evm_read_contract` | ❌ W0 |
| **CTX-02** | `ctx.evm.erc20Balance` reads ERC20 balances | 04-02 T1 (`erc20_balance_of_returns_initial_supply_for_deployer`) + 04-02 T2 (`flat_alias_erc20Balance_callable_and_matches_structured_form`) | `cargo test -p executor-evm --features anvil-tests --test erc20_helpers_anvil erc20_balance_of_ && cargo test -p strategy-js --test ctx_evm_helpers flat_alias_erc20Balance_` | ❌ W0 |
| **CTX-03** | `ctx.evm.erc20Allowance` reads ERC20 allowances | 04-02 T1 (`erc20_allowance_returns_zero_for_unapproved_spender`) + 04-02 T2 (`flat_alias_erc20Allowance_matches_structured_form`) | `cargo test -p executor-evm --features anvil-tests --test erc20_helpers_anvil erc20_allowance_ && cargo test -p strategy-js --test ctx_evm_helpers flat_alias_erc20Allowance_` | ❌ W0 |
| **CTX-04** | `ctx.evm.nativeBalance` reads native token balance | 04-02 T1 (`native_balance_returns_anvil_funded_balance`) + 04-02 T2 (`flat_alias_nativeBalance_matches_structured_form`) | `cargo test -p executor-evm --features anvil-tests --test native_helpers_anvil native_balance_ && cargo test -p strategy-js --test ctx_evm_helpers flat_alias_nativeBalance_` | ❌ W0 |
| **CTX-05** | `ctx.actions.contractCall` creates ABI-based contract call actions | 04-03 T1 (Action enum + ContractCallAction struct) + 04-03 T2 (`contract_call_builder_returns_valid_json`) + 04-03 T3 (`strategy_run_accepts_contract_call`) + 04-04 T2 (4 negative cases) | `cargo test -p strategy-js --test ctx_actions_builders contract_call_ && cargo test -p executor-mcp --test stdio_handshake strategy_run_accepts_contract_call && cargo test -p strategy-js --test ctx_actions_negative_grid contract_call_` | ❌ W0 |
| **CTX-06** | `ctx.actions.rawCall` creates raw calldata actions | 04-03 T1+T2 (`raw_call_builder_returns_valid_json`) + 04-03 T3 (`strategy_run_accepts_raw_call`) + 04-04 T2 (3 negative cases) | `cargo test -p strategy-js --test ctx_actions_builders raw_call_ && cargo test -p executor-mcp --test stdio_handshake strategy_run_accepts_raw_call && cargo test -p strategy-js --test ctx_actions_negative_grid raw_call_` | ❌ W0 |
| **CTX-07** | `ctx.actions.erc20Approve` and `ctx.actions.erc20Transfer` create ERC20 actions | 04-03 T1+T2 (`erc20_transfer_builder_validates_amount`, `erc20_approve_builder_returns_valid_json`) + 04-03 T3 (2 accept tests) + 04-04 T2 (5 negative cases) | `cargo test -p strategy-js --test ctx_actions_builders erc20_ && cargo test -p executor-mcp --test stdio_handshake strategy_run_accepts_erc20_ && cargo test -p strategy-js --test ctx_actions_negative_grid erc20_` | ❌ W0 |
| **CTX-08** | `ctx.actions.nativeTransfer` creates native transfer actions | 04-03 T1+T2 (`native_transfer_builder_returns_valid_json`, `native_transfer_builder_rejects_negative_value`) + 04-03 T3 (`strategy_run_accepts_native_transfer`) + 04-04 T2 (3 negative cases) | `cargo test -p strategy-js --test ctx_actions_builders native_transfer_ && cargo test -p executor-mcp --test stdio_handshake strategy_run_accepts_native_transfer && cargo test -p strategy-js --test ctx_actions_negative_grid native_transfer_` | ❌ W0 |
| **CTX-09** | `ctx.units` and address helpers reduce common EVM mistakes | 04-04 T1 (units round-trip + address total isAddress + checksum + zeroAddress) | `cargo test -p executor-evm --lib units:: address:: && cargo test -p strategy-js --test ctx_units_address` | ❌ W0 |

**Coverage:** every Phase-4 requirement (CTX-01..CTX-09) maps to ≥ 1 owning task with an automated test command. No requirement is unmapped.

**Carry-forward verification (D-15):** every plan's final task includes a regression assertion against `sandbox_blocks_host_globals` (HR-01). 04-01 errors.rs + 04-02 helpers + 04-03 builders + 04-04 negative-grid all assert no raw alloy substrings on the wire (MR-01). Journal write paths in 04-01 / 04-02 use `?`-propagation (MR-03). 04-01 `seq` column on journal_source_reads + 04-01 same-ms test (MR-04).

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (11/11 tasks mapped).
- [x] Sampling continuity: no 3 consecutive tasks without automated verify.
- [x] Wave 0 covers all MISSING references mapped to owning tasks.
- [x] No watch-mode flags.
- [x] Feedback latency ≤ 45s (workspace ~15s default; ~45s with anvil).
- [x] `nyquist_compliant: true` set in frontmatter.
- [x] Every Phase-4 requirement (CTX-01..09) has ≥ 1 task that exercises it via an automated command.
- [x] Phase-3 carry-forward rules (HR-01, MR-01, MR-03, MR-04) have explicit regression coverage in each plan.
- [x] Schema goldens for the 6-variant Action enum + 5 per-variant structs are tracked in Wave 0 and locked at first introduction (Phase-2 D-05 future-lock pattern carry-over).
- [x] D-16 Phase-3 reject-test flip is mapped to 04-03 Task 3 with explicit rename trail.

**Approval:** approved 2026-04-27 (per-task map populated by planner; `wave_0_complete: true` will be set by executor once all Wave-0 fixture/source files land in their owning plan tasks and their automated commands exit 0).

---

## Notes for the Executor

1. **Anvil binary detection** — Plan 04-01 Task 2's `read_counter_number_returns_zero` is the canary; if anvil isn't on PATH, the test must `eprintln!` skip + early return (NOT panic). If `cargo test --workspace --features anvil-tests` reports a panic from the fixture, fix the fixture first — this is a wave-blocker for 04-01 / 04-02.

2. **The `cargo tree` audit for alloy 2.0.x** (Plan 04-01 Task 1) is the durable guarantee that future dep churn doesn't accidentally pull alloy 1.x. Bake into pipeline; if `cargo update` regresses, this audit will fail loudly.

3. **The schema-golden walker** for Plan 04-04 Task 3's `action_schema_includes_six_kinds` test MUST collect strings from BOTH `enum[]` arrays AND `const` fields (Phase-2 02-03 SUMMARY:39 pattern). schemars 1.x can emit either shape.

4. **Provider concurrency test** (Plan 04-01 Task 3 Test 5 — `ctx_evm_readContract_drops_state_mutex_before_block_on`) may be flaky on slow machines. Mark `#[ignore]` with documented run command if observed flaky; do NOT delete — the property is critical (D-04 mutex discipline).

5. **rquickjs::Function::new closure must be `'static + Send`** (RESEARCH Pitfall 9 — sandbox parallel feature is forbidden but Send is still required for the closure capture). Verify at compile time when adding ctx.evm.* and ctx.actions.* host functions; if a `!Send` value sneaks in via `Arc<DynProvider>` (it shouldn't — DynProvider is Send + Sync + Clone), restructure.

6. **rquickjs Object.freeze for ctx.address.zeroAddress** — if rquickjs 0.11 supports declaring a property as `writable: false`, use it. If not, the value is constant per-injection; agents that try to reassign only mutate their JS-local view. Document whichever is chosen in Plan 04-04 Task 1 SUMMARY.

7. **Workspace test count target after Phase 4:** ≥ 280 total. Phase 3 left ~175. Phase 4 nets ~106 new tests across the four plans (estimate; final SUMMARY records actual).

8. **Documentation updates** — every task's commit lands the test code; the docs (REQUIREMENTS.md traceability table marking CTX-01..09 Complete, ROADMAP.md marking Phase 4 Complete) is the FINAL `docs(04)` commit after Plan 04-04 Task 3 lands. Mirror Phase-3 commit style.

9. **Strategy-js depends on executor-evm** (Plan 04-01 Task 3, Cargo.toml change). This adds a new crate-graph edge. Verify `cargo build --workspace` resolves cleanly; alloy must NOT leak into strategy-js's public types — only EvmError and EvmConfig may.
