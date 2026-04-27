---
phase: 03
slug: javascript-strategy-runner
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-27
updated: 2026-04-27
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Mirrors `02-VALIDATION.md` shape; populated from each PLAN.md `<verification><automated>` block + acceptance criteria.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust built-in) + `tokio::test` for stdio integration |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` |
| **Quick run command** | `cargo test -p strategy-js && cargo test -p executor-state --test journal_repo --test run_lifecycle_transition && cargo test -p executor-mcp --test stdio_handshake` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~10–25 seconds. Phase-2 baseline: 92 tests in ~0.30s. Phase 3 adds: ~38 strategy-js tests (some with 2-second wall-clock budgets — `wall_clock_interrupt_terminates_infinite_loop` alone takes ≤ 2.5s) + ~14 executor-state tests + ~3 executor-core schema tests + 19 stdio tests (each ~50–500 ms with binary spawn). The `_runtime_error_on_infinite_loop` and `_runtime_error_on_oom` tests dominate wall-time. |

---

## Sampling Rate

- **After every task commit:** Run the targeted command for the crate touched (`cargo test -p <crate>`). Latency ≤ 5s for most plans; up to 25s for stdio integration tests with timeout strategies.
- **After every plan wave:** Run `cargo test --workspace`.
- **Before `/gsd-verify-work`:** Full suite + `cargo clippy --workspace --all-targets -- -D warnings` must be green.
- **Max feedback latency:** ~30 seconds (clippy + tests + sandbox-budget tests).

---

## Per-Task Verification Map

*Populated from each PLAN.md `<verification><automated>` block. File-Exists column reflects intent: `✅` if the test file already exists in the repo (Phase-2 inheritance); `❌ W0` if the same plan's first task creates it.*

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 03-01-T1 | 03-01 | 1 | STR-04 | T-03-01-04 (no dyn-load) | strategy-js crate scaffold + RuntimeError taxonomy + D-03 limits constants + cargo-tree audit (no libloading / tokio via rquickjs) | unit (rust) + cargo tree | `cargo build -p strategy-js && cargo test -p strategy-js --lib && cargo clippy -p strategy-js --all-targets -- -D warnings && cargo tree -p strategy-js \| (! grep -E '(libloading\|^.*tokio v)')` | ❌ W0 (whole crate is new) | ⬜ pending |
| 03-01-T2 | 03-01 | 1 | STR-04 | T-03-01-01..03 (DoS limits), T-03-01-09 (no global leaks), T-03-01-10 (no promise) | Sandbox::execute with D-03 limits, D-05 Shape B, D-10 promise reject, error classification | unit (rust) | `cargo test -p strategy-js --test sandbox_limits && cargo test -p strategy-js --test sandbox_entry_shape && cargo clippy -p strategy-js --all-targets -- -D warnings` | ❌ W0 (test files created by Task 2) | ⬜ pending |
| 03-01-T3 | 03-01 | 1 | STR-04 | T-03-01-04..08 (no host caps) | D-11 forbidden-globals regression suite — proves STR-04 at runtime layer | unit (rust) | `cargo test -p strategy-js && cargo clippy -p strategy-js --all-targets -- -D warnings` | ❌ W0 (test file created by Task 3) | ⬜ pending |
| 03-02-T1 | 03-02 | 2 | STJ-03, STJ-04 | T-03-02-04 (transition guard), T-03-02-08 (schema future-lock) | journal_source_reads / journal_actions / journal_logs schema + repo + JournalActionOutcome with phase3_emittable + update_run_status_with_transition (closes 02-REVIEW MR-01) | unit (rust) + schema golden | `cargo test -p executor-state --test journal_repo --test run_lifecycle_transition && cargo test -p executor-core --test schema_snapshots && cargo clippy -p executor-state -p executor-core --all-targets -- -D warnings` | ❌ W0 (2 new test files; schema_snapshots existing) | ⬜ pending |
| 03-02-T2 | 03-02 | 2 | STR-03 | T-03-02-01 (ctx mutation safe), T-03-02-06 (ctx.log buffer integrity) | D-04 ctx surface (strategy/run/now/log/actions.noop) injected into sandbox + log buffering | unit (rust) | `cargo test -p strategy-js --test ctx_host_api && cargo test -p strategy-js && cargo clippy -p strategy-js --all-targets -- -D warnings` | ❌ W0 (test file created by Task 2) | ⬜ pending |
| 03-02-T3 | 03-02 | 2 | STR-03, STJ-03 | T-03-02-02 (log memory bounded), T-03-02-05 (per-run journal isolation) | RuntimeContext (StateStore-backed CtxHost) + flush() drains logs + source-read marker in single mutex acquisition | integration (rust + sqlite) | `cargo test -p strategy-js --test runtime_journal_flush && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | ❌ W0 (test file created by Task 3) | ⬜ pending |
| 03-03-T1 | 03-03 | 3 | STR-03, STR-05, STJ-04 | T-03-03-09 (error-code uniqueness audit) | 3 new MCP error codes (-32011/-32017/-32018) + map_runtime_error + StrategyRun(Input/Response/Outcome) types + 3 schema goldens | unit (rust) + schema golden | `cargo test -p executor-core --test schema_snapshots && cargo test -p executor-mcp --lib errors:: && cargo build -p executor-mcp && cargo clippy -p executor-mcp -p executor-core --all-targets -- -D warnings` | ❌ W0 (3 new schema goldens) | ⬜ pending |
| 03-03-T2 | 03-03 | 3 | STR-03, STR-05, STJ-04 | T-03-03-01 (param SQL), T-03-03-03 (no orphan run rows), T-03-03-04 (transition serialisation) | strategy_run handler + journal://{run_id} resource + ExecutorServer wiring + retire strategy_run_once placeholder | build + clippy (rust); integration coverage at T3 | `cargo build -p executor-mcp && cargo clippy -p executor-mcp --all-targets -- -D warnings && cargo test -p executor-mcp --test stdio_handshake -- --skip unimplemented_tools_return_phase_hint --skip strategy_run_once` | ✅ (extends Phase-2 src/tools/resources/server) | ⬜ pending |
| 03-03-T3 | 03-03 | 3 | STR-03, STR-05, STJ-04 | T-03-03-02..09 (full e2e MCP contract for strategy_run) | 19 D-08a stdio integration tests + retire `unimplemented_tools_return_phase_hint` strategy_run_once entry | integration (rust + stdio) | `cargo test -p executor-mcp --test stdio_handshake && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | ✅ (extends Phase-2 stdio_handshake.rs) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

**Sampling continuity:** Every task has an `<automated>` cargo command. No 3 consecutive tasks lack automated verify. Wave 1 covers 3 tasks (sandbox crate scaffold + execute body + D-11 regression), Wave 2 covers 3 tasks (schema + ctx + RuntimeContext), Wave 3 covers 3 tasks (error codes + handler + stdio). Total feedback latency for full suite stays ≤ ~30s.

---

## Wave 0 Requirements

Wave 0 fixture/test files are created **as part of the same plan that needs them** (no separate Wave 0 plan — Phase 3 scope is small enough that Wave 0 work folds into each plan's first task). The full Wave 0 set:

- [ ] `crates/strategy-js/Cargo.toml` — new crate manifest. Created by Plan **03-01 Task 1**.
- [ ] `crates/strategy-js/src/{lib.rs,error.rs,limits.rs,sandbox.rs,runtime.rs}` — crate scaffold + impl. Created by Plan **03-01 Task 1+2** (sandbox.rs body lands in Task 2; runtime.rs lands in Plan **03-02 Task 3**).
- [ ] `crates/strategy-js/tests/{sandbox_limits.rs,sandbox_entry_shape.rs,sandbox_host_globals.rs,ctx_host_api.rs,runtime_journal_flush.rs}` — five test files. Created by Plans **03-01 T2/T3** + **03-02 T2/T3**.
- [ ] `crates/executor-state/src/journal.rs` — new repo module. Created by Plan **03-02 Task 1**.
- [ ] `crates/executor-state/tests/{journal_repo.rs,run_lifecycle_transition.rs}` — repo CRUD + transition guard tests. Created by Plan **03-02 Task 1**.
- [ ] `crates/executor-core/tests/schemas/JournalActionOutcome.json` — new schema golden. Created by Plan **03-02 Task 1** via `UPDATE_SCHEMAS=1`.
- [ ] `crates/executor-core/tests/schemas/{StrategyRunInput.json, StrategyRunResponse.json, StrategyOutcome.json}` — three new schema goldens. Created by Plan **03-03 Task 1** via `UPDATE_SCHEMAS=1`. The Phase-1 `StrategyRunOnceInput.json` is DELETED in the same task.
- [ ] `Cargo.toml` (root) — `members` array updated to include `crates/strategy-js`. Updated by Plan **03-01 Task 1**.

`wave_0_complete: true` is set by the executor when all eight items exist on disk and their owning task's `<automated>` command exits 0.

---

## Manual-Only Verifications

*Expected: none. Phase 3 is a closed-loop runtime layer with no UI / external service / human-only checks. Every behaviour is observable via `cargo test --workspace`. Planner confirms.*

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| _none expected_ | | | |

---

## Phase Requirements → Test Map

| Req ID | Behavior | Owning Plan / Task | Automated Command | File Exists |
|--------|----------|-------------------|-------------------|-------------|
| STR-03 | Agent can run a registered strategy via MCP `strategy_run`; ctx surface (D-04) is observable from inside the sandbox; STJ-03 source-read row is recorded; output validation against `Action[]`/`noop` | 03-02 T2 (ctx surface) + 03-02 T3 (RuntimeContext flush) + 03-03 T2 (handler) + 03-03 T3 (`strategy_run_returns_noop_for_minimal_strategy`, `strategy_run_returns_actions_for_action_array_strategy`, `strategy_run_records_source_read_journal_row`) | `cargo test -p executor-mcp --test stdio_handshake strategy_run_` | ❌ W0 (Plan 03-03 Task 3) |
| STR-04 | Strategy code cannot access private keys / filesystem / process APIs / arbitrary network / direct RPC clients | 03-01 T2 (resource limits) + 03-01 T3 (D-11 forbidden globals) | `cargo test -p strategy-js --test sandbox_host_globals && cargo test -p strategy-js --test sandbox_limits` | ❌ W0 (Plan 03-01 Tasks 2+3) |
| STR-05 | Strategy returns `Action[]` or `"noop"`; runtime rejects unsupported return shapes | 03-01 T2 (D-05 Shape B + D-10 promise) + 03-03 T2 (validate_strategy_output) + 03-03 T3 (`strategy_run_rejects_number_return`, `_rejects_object_return`, `_rejects_null_return`, `_rejects_promise_return`, `_rejects_non_function_source`, `_rejects_phase4_action_kind`) | `cargo test -p executor-mcp --test stdio_handshake strategy_run_rejects_` | ❌ W0 (Plan 03-03 Task 3) |
| STJ-03 | Runtime records source reads performed during each run | 03-02 T1 (journal_source_reads schema + repo) + 03-02 T3 (RuntimeContext::flush writes the marker) + 03-03 T3 (`strategy_run_records_source_read_journal_row`) | `cargo test -p executor-state --test journal_repo record_source_read && cargo test -p executor-mcp --test stdio_handshake strategy_run_records_source_read_journal_row` | ❌ W0 (Plan 03-02 + 03-03) |
| STJ-04 | Runtime records returned actions and validation errors | 03-02 T1 (journal_actions schema + repo + JournalActionOutcome) + 03-03 T2 (handler records on every path) + 03-03 T3 (`strategy_run_run_row_status_transitions_to_failed_on_error`, `_records_log_messages`, plus implicit per-test journal assertions) | `cargo test -p executor-state --test journal_repo record_action_outcome && cargo test -p executor-mcp --test stdio_handshake strategy_run_` | ❌ W0 (Plan 03-02 + 03-03) |

**Coverage:** every Phase-3 requirement (STR-03, STR-04, STR-05, STJ-03, STJ-04) maps to ≥ 1 owning task with an automated test command. No requirement is unmapped.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (9/9 tasks mapped).
- [x] Sampling continuity: no 3 consecutive tasks without automated verify.
- [x] Wave 0 covers all MISSING references (8 fixture/test/schema files mapped to owning tasks).
- [x] No watch-mode flags (no `-w`, no `--watch`, no `cargo watch`).
- [x] Feedback latency ≤ 30s (workspace runs ~10–25s; Phase-2 baseline 0.30s, Phase-3 adds wall-clock-budget tests up to 2.5s each).
- [x] `nyquist_compliant: true` set in frontmatter (verifiable via grep on this file).
- [x] Every Phase-3 requirement (STR-03/04/05, STJ-03/04) has ≥ 1 task that exercises it via an automated command.
- [x] All 19 D-08a test names from CONTEXT D-08a appear verbatim in Plan 03-03 Task 3's `<behavior>` block.
- [x] Forbidden globals from D-11 are exhaustively enumerated in Plan 03-01 Task 3's test source.
- [x] Schema goldens (`StrategyRunInput.json`, `StrategyRunResponse.json`, `StrategyOutcome.json`, `JournalActionOutcome.json`) are tracked in Wave 0 and locked at first introduction (Phase-2 D-05 future-lock pattern carry-over).

**Approval:** approved 2026-04-27 (per-task map populated by planner; `wave_0_complete: true` will be set by executor once all 8 Wave-0 fixture/source files land in their owning plan tasks and their automated commands exit 0).

---

## Notes for the Executor

1. **The `_runtime_error_on_infinite_loop` and `_runtime_error_on_oom` tests dominate wall-time** (each takes up to `WALL_CLOCK_MS + spawn overhead` ≈ 2.5s). If `cargo test` parallelism causes flake under load, consider a serial `--test-threads=1` flag for these specific tests OR move them to a separate `#[ignore]`-by-default file with a documented run command. Default: leave parallel; flag if observed flaky.

2. **The error-code uniqueness audit** (Plan 03-03 Task 1):
   ```bash
   grep -r 'ErrorCode(-3201[178])' ~/.cargo/registry/src/index.crates.io-*/rmcp-1.5.0/ || echo "NO COLLISIONS"
   ```
   Plan 02-02 SUMMARY:213-215 ran the same audit for -32014/-32015/-32016 and got NO COLLISIONS. The expectation is the same here. If the audit fails, STOP and consult the user — `-32012`/`-32013`/`-32019` are reserved for renumbering.

3. **The schema golden walker** for Plan 03-02 Task 1's `journal_action_outcome_includes_future_variants` test MUST collect strings from BOTH `enum[]` arrays AND `const` fields (mirroring 02-03 SUMMARY:39 walker pattern). schemars 1.x can emit either shape depending on the enum.

4. **The `cargo tree` audit** (Plan 03-01 Task 1) is the durable guarantee that future dep churn doesn't accidentally enable `loader`/`dyn-load`/`futures`. Bake it into the verification pipeline; if a future `cargo update` adds `libloading`, this audit will fail loudly.

5. **`Sandbox::execute` is synchronous and `!Send` per rquickjs.** Every caller in Plan 03-03 wraps it in `tokio::task::spawn_blocking`. The closure captures `RuntimeContext` (which IS `Send` because it uses `Arc<Mutex<>>` and `Arc<dyn Fn() + Send + Sync>`). Verify these `Send` bounds at compile time — if rquickjs 0.11 adds a `!Send` field that propagates into `Sandbox::execute`'s body, restructure to keep the entire sandbox call inside `spawn_blocking`.

6. **Workspace test count target after Phase 3:** ≥ 130 total. Phase 2 left 92. Phase 3 adds:
   - 03-01: 18 strategy-js tests (3 limits + 8 entry-shape + 7 host-globals)
   - 03-02: 13 ctx_host_api + 5 runtime_journal_flush + 8 journal_repo + 6 run_lifecycle_transition + 1 schema golden = 33 new
   - 03-03: 4 errors-mod + 3 schema goldens + 19 stdio = 26 new
   Total Phase-3 net: ~77 new tests. Workspace target: 92 + 77 ≈ 169 tests. Final SUMMARY records the actual number.

7. **Documentation updates** — every task's commit lands the test code; the docs (REQUIREMENTS.md traceability table marking STR-03/04/05/STJ-03/04 Complete, ROADMAP.md marking Phase 3 Complete) is the FINAL `docs(03)` commit after Plan 03-03 Task 3 lands. Mirror Phase-2 02-02 commit `2831418` style.
