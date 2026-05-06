---
phase: 1
slug: mcp-runtime-surface
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-24
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust 2024 edition, workspace tests) + `cargo clippy` |
| **Config file** | Workspace `Cargo.toml` + per-crate `tests/` directories |
| **Quick run command** | `cargo test --workspace --lib` |
| **Full suite command** | `cargo test --workspace && cargo clippy --workspace -- -D warnings` |
| **Estimated runtime** | ~30–90 seconds (cold), ~10–20 seconds (warm) |

---

## Sampling Rate

- **After every task commit:** Run `cargo check --workspace` (≤10s) plus the test command for the touched crate (e.g., `cargo test -p executor-mcp`).
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`.
- **Before `/gsd-verify-work`:** Full suite green AND integration test (`cargo test -p executor-mcp --test stdio_handshake`) green.
- **Max feedback latency:** 90 seconds for full sweep, 20 seconds for per-task.

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 1-01-01 | 01 | 1 | MCP-01 | — | Workspace builds with 4 crates and 2024 edition | build | `cargo check --workspace` | ❌ W0 | ⬜ pending |
| 1-01-02 | 01 | 1 | MCP-01 | — | Clippy lint denies stdout/stderr/dbg in `executor-mcp` | lint | `cargo clippy -p executor-mcp -- -D warnings` | ❌ W0 | ⬜ pending |
| 1-02-01 | 02 | 2 | MCP-02 | — | All declared tools appear in `tools/list` with JSON Schema | integration | `cargo test -p executor-mcp --test stdio_handshake tools_list_emits_full_surface` | ❌ W0 | ⬜ pending |
| 1-02-02 | 02 | 2 | MCP-02 | — | Write tools return structured `unimplemented` error | integration | `cargo test -p executor-mcp --test stdio_handshake unimplemented_tools_return_phase_hint` | ❌ W0 | ⬜ pending |
| 1-02-03 | 02 | 2 | MCP-02 | — | Read-only tools (`strategy_list`, `policy_get`, `execution_get`, `strategy_get`) return placeholder shapes | unit/integration | `cargo test -p executor-mcp --test stdio_handshake readonly_tools_return_placeholder` | ❌ W0 | ⬜ pending |
| 1-03-01 | 03 | 3 | MCP-03 | — | `resources/list` empty, `resources/templates/list` declares 3 URI shapes, `resources/read` returns not-found for unknown URI | integration | `cargo test -p executor-mcp --test stdio_handshake resources_surface_matches_contract` | ❌ W0 | ⬜ pending |
| 1-03-02 | 03 | 3 | MCP-04 | — | `prompts/list` exposes `write_evm_strategy` and `review_evm_strategy` with placeholder body | integration | `cargo test -p executor-mcp --test stdio_handshake prompts_surface_matches_contract` | ❌ W0 | ⬜ pending |
| 1-03-03 | 03 | 3 | MCP-01 | — | Every stdout line is JSON-RPC 2.0 parseable; no logging leaks | integration | `cargo test -p executor-mcp --test stdio_handshake stdout_is_strict_jsonrpc` | ❌ W0 | ⬜ pending |
| 1-03-04 | 03 | 3 | MCP-02 | — | Schema contract round-trip: every tool input struct serializes → JsonSchema → validates against sample inputs | unit | `cargo test -p executor-mcp schema_contract_round_trip` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `Cargo.toml` (workspace root) — declares 4 member crates and shared dependency versions
- [ ] `rust-toolchain.toml` — pins 2024 edition / stable channel
- [ ] `crates/executor-mcp/tests/stdio_handshake.rs` — integration harness file (test bodies filled per task)
- [ ] `crates/executor-mcp/tests/common/mod.rs` — shared `spawn_server()` + `JsonRpcLine` helpers (per RESEARCH.md §"Integration Test Harness")
- [ ] `crates/executor-mcp/Cargo.toml` `[dev-dependencies]` — `tokio-test`, `serde_json`, `assert_matches` (per RESEARCH.md)
- [ ] `clippy.toml` (workspace root, optional) — shared clippy thresholds
- [ ] CI scaffold (deferred to Plan 03 if time allows): `.github/workflows/ci.yml` running `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Claude Desktop / MCP Inspector smoke test | MCP-01..04 | Confirms wire compatibility with a real client across the stdio boundary; cannot be fully automated within `cargo test` because real clients negotiate `protocolVersion` and surface UI quirks | 1) `cargo build -p executor-mcp --release`. 2) Configure Claude Desktop `mcpServers` entry pointing to the binary. 3) Verify all tools/resources/prompts appear in the UI. 4) Invoke `strategy_register` → must surface structured `unimplemented` error. |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 90s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
