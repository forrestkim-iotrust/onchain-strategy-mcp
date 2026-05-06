---
phase: 02
slug: strategy-state-and-journal
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-24
updated: 2026-04-27
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Details to be filled in by planner using 02-RESEARCH.md §"Validation Architecture" and 02-CONTEXT.md D-08 testing plan.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust built-in, tokio `#[tokio::test]` for async) |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` |
| **Quick run command** | `cargo test -p executor-state` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5-15 seconds (phase 1 baseline: 0.22s for 20 tests; phase 2 adds ~12 tests + repository-level) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p executor-state -p executor-mcp` (targeted)
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** Full suite + `cargo clippy --workspace --all-targets -- -D warnings` must be green
- **Max feedback latency:** ~30 seconds (clippy + tests)

---

## Per-Task Verification Map

*Populated from each PLAN.md `<verify><automated>` block. File-Exists column reflects intent: `✅` if the test file already exists in repo (Phase 1 inheritance); `❌ W0` if Wave 0 of the same plan creates it.*

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-T1 | 02-01 | 1 | STJ-01 | T-02-01 (DB integrity) | Schema + StateStore::open + error taxonomy initialised with WAL/FK pragmas + partial unique index enforced | unit (rust) | `cargo build -p executor-state && cargo test -p executor-state --test partial_index_behaviour -- --nocapture` | ❌ W0 (test file created by Task 1) | ⬜ pending |
| 02-01-T2 | 02-01 | 1 | STR-01, STJ-01, STJ-02 | T-02-02 (content addressing) | Strategy + Run repo: SHA-256 content addressing, soft-delete-then-reuse, full RunStatus enum, idempotent register | unit (rust) | `cargo test -p executor-state && cargo test -p executor-core --test schema_snapshots` | ❌ W0 (state repo tests new) / ✅ schema_snapshots (Phase 1) | ⬜ pending |
| 02-01-T3 | 02-01 | 1 | STJ-01 | — | Config `[state]` section + `:memory:` support + Phase-1 `--config=PATH` parser bug fix | unit (rust) | `cargo test -p executor-mcp --lib config::tests` | ✅ (Phase 1 module, tests extended) | ⬜ pending |
| 02-02-T1 | 02-02 | 2 | STR-01, STR-02 | T-02-03 (input validation) | StateError → MCP error code mapping (`-32014/-32015/-32016`) + payload validation (D-09 size/length limits) | unit (rust) | `cargo test -p executor-mcp --lib errors::tests && cargo test -p executor-mcp --lib validation::tests` | ❌ W0 (validation.rs new) / ✅ errors (Phase 1) | ⬜ pending |
| 02-02-T2 | 02-02 | 2 | STR-01, STR-02, STJ-01 | T-02-04 (transition placeholder→real) | 5 tool bodies use `Arc<Mutex<StateStore>>` + `spawn_blocking`; resources branch on `strategy://{id}` | build/lint (rust) | `cargo build -p executor-mcp && cargo clippy -p executor-mcp --all-targets -- -D warnings` | ✅ (extends Phase 1 tools/resources) | ⬜ pending |
| 02-02-T3 | 02-02 | 2 | STR-01, STR-02, STJ-01 | T-02-05 (e2e MCP contract) | 14 new MCP integration tests covering D-08a strategy lifecycle (idempotency, conflict, soft-delete-reuse, list-excludes-source, get-by-id/name) + 3 supporting + 4 Phase-1 regression updates | integration (rust + stdio) | `cargo test -p executor-mcp --test stdio_handshake` | ✅ (extends Phase 1 stdio_handshake.rs) | ⬜ pending |
| 02-03-T1 | 02-03 | 3 | STJ-02 | T-02-06 (run lifecycle integrity) | RunRepo: ULID shape, started_at/finished_at semantics, list ordering, missing-id, reserved-variant rejection on update | unit (rust) | `cargo test -p executor-state --test run_base_model` | ❌ W0 (test file created by Task 1) | ⬜ pending |
| 02-03-T2 | 02-03 | 3 | STJ-02 | T-02-07 (e2e run roundtrip) | E2E `execution_get` against inserted run + RunStatus JSON Schema includes all 7 future variants | integration (rust + stdio) | `cargo test -p executor-mcp --test stdio_handshake -- --test run_roundtrip_insert_get_update_status --test run_status_schema_includes_future_variants` | ✅ (extends Phase 1 stdio_handshake.rs) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

**Sampling continuity:** Every task has an `<automated>` cargo command. No 3 consecutive tasks lack automated verify. Wave 1 covers 3 tasks (state crate + config), Wave 2 covers 3 tasks (MCP wiring + e2e), Wave 3 covers 2 tasks (run polish + e2e). Total feedback latency for full suite stays under ~30s on Phase 1 baseline.

---

## Wave 0 Requirements

Wave 0 fixture/test files are created **as part of the same plan that needs them** (no separate Wave 0 plan — Phase 2 scope is small enough that Wave 0 work is folded into each plan's first task that requires the fixture):

- [ ] `crates/executor-state/tests/common/mod.rs` — shared test fixture (open `:memory:` StateStore, seed helpers). Created by Plan **02-01 Task 2**.
- [ ] `crates/executor-state/tests/partial_index_behaviour.rs` — index + pragma sanity. Created by Plan **02-01 Task 1**.
- [ ] `crates/executor-state/tests/strategies_repo.rs` — content-address + soft-delete + idempotency. Created by Plan **02-01 Task 2**.
- [ ] `crates/executor-state/tests/run_base_model.rs` — run repo lifecycle. Created by Plan **02-03 Task 1**.
- [ ] `crates/executor-mcp/tests/common/mod.rs` extension — state-aware spawn helper with in-tempfile DB. Extended by Plan **02-02 Task 3**.
- [ ] `crates/executor-mcp/src/validation.rs` — D-09 input validation module + tests. Created by Plan **02-02 Task 1**.

`wave_0_complete: true` is set by executor when all six items exist on disk and their owning task's `<automated>` command exits 0.

---

## Manual-Only Verifications

*Expected: none. All Phase 2 behaviors should have automated coverage because this is a pure persistence layer with a well-defined API surface. Planner confirms.*

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| _none expected_ | | | |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (8/8 tasks mapped)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (6 fixture/test files mapped to owning tasks)
- [x] No watch-mode flags (no `-w`, no `--watch`, no `cargo watch`)
- [x] Feedback latency < 30s (Phase 1 baseline 0.22s for 20 tests; Phase 2 adds ~14 strategy tests + repo-level)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-27 (per-task map populated from plan-checker pass; `wave_0_complete: true` will be set by executor once Wave 0 fixtures land in their owning plan tasks)
