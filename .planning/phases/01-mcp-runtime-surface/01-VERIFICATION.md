---
phase: 01-mcp-runtime-surface
verified: 2026-04-24T10:30:00Z
status: passed
score: 17/17 must-haves verified
overrides_applied: 0
---

# Phase 1: MCP Runtime Surface Verification Report

**Phase Goal:** A stdio MCP server boots cleanly and exposes the initial runtime contract.
**Verified:** 2026-04-24T10:30:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

Must-haves are merged from ROADMAP.md Success Criteria + the three PLAN frontmatters (01-01, 01-02, 01-03).

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | (SC1) MCP client can initialize the server over stdio | VERIFIED | `stdout_is_strict_jsonrpc` test asserts `init_resp["jsonrpc"] == "2.0"` after `initialize` call (stdio_handshake.rs:383-385). `initialize` helper sends `protocolVersion: "2025-11-25"` + `notifications/initialized` (common/mod.rs:70-95). Test passes (20/20 cargo test --workspace). |
| 2 | (SC2) Tool list exposes strategy, execution, and policy groups with JSON schemas | VERIFIED | `tools_list_emits_full_surface` asserts exactly 8 tools (strategy_register/list/get/delete/run_once, execution_get, policy_get/update) each with `inputSchema` and `description` (stdio_handshake.rs:39-89). 8 `#[tool(name=...)]` handlers in tools.rs:27-135 bound to `Parameters<T>` where T derives JsonSchema in executor-core. Test passes. |
| 3 | (SC3) Resource list exposes strategy, execution, and journal URI shapes | VERIFIED | `resources_surface_matches_contract` asserts 3 templates: `strategy://{strategy_id}`, `execution://{execution_id}`, `journal://{execution_id}` (stdio_handshake.rs:234-264). Templates constructed via `make_template` in resources.rs:42-93. `resources/read` returns -32002 with `data.phase=1`. Test passes. |
| 4 | (SC4) Prompt list exposes strategy authoring/review prompts | VERIFIED | `prompts_surface_matches_contract` asserts exactly 2 prompts: `write_evm_strategy`, `review_evm_strategy` with descriptions (stdio_handshake.rs:297-322). `#[prompt]` handlers in prompts.rs:31-62 bound to `Parameters<WriteEvmStrategyArgs>` / `Parameters<ReviewEvmStrategyArgs>`. Test passes. |
| 5 | (01-01) Workspace contains executor-mcp, executor-core, executor-state, executor-signer crates only; cargo check succeeds | VERIFIED | Cargo.toml:3 lists exactly 4 crate members. `ls crates/` returns 4 directories. `cargo check --workspace` → Finished in 0.75s. |
| 6 | (01-01) executor-mcp clippy blocks println!/eprintln!/dbg! with -D warnings | VERIFIED | Cargo.toml:26-31 workspace.lints.clippy with `print_stdout/print_stderr/dbg_macro = "deny"`. All four crate entry files declare `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`. `cargo clippy --workspace --all-targets -- -D warnings` succeeds with 0 warnings. `grep -rn "println!\|eprintln!\|dbg!" crates/` finds only a descriptive comment in a test file (no runtime usage). |
| 7 | (01-01) executor-core exposes tool/prompt input schema structs with JsonSchema derive | VERIFIED | 7 structs confirmed: StrategyRegisterInput, StrategyIdInput, StrategyRunOnceInput (strategy.rs), ExecutionIdInput (execution.rs), PolicyUpdateInput (policy.rs), WriteEvmStrategyArgs, ReviewEvmStrategyArgs (prompt_args.rs). All `#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]`. |
| 8 | (01-01) Wave 0 harness files exist so subsequent plans just add test bodies | VERIFIED | `tests/common/mod.rs` (spawn_server/send/recv/initialize), `tests/stdio_handshake.rs` (mod common + tests), `tests/schema_snapshots.rs` (7 #[test] fns) all present. |
| 9 | (01-02) MCP client completes stdio initialize | VERIFIED | See truth #1. |
| 10 | (01-02) tools/list returns 8 tools with JSON schemas | VERIFIED | See truth #2. |
| 11 | (01-02) Write tools return unimplemented error with data.code="unimplemented" + data.phase | VERIFIED | `unimplemented_tools_return_phase_hint` verifies (register→2, delete→2, run_once→6, policy_update→5), err.code=-32010, err.data.code="unimplemented", err.data.phase matches, err.data.tool echoed (stdio_handshake.rs:92-136). errors.rs:21-32 builds payload. Test passes. |
| 12 | (01-02) strategy_list returns []; policy_get returns placeholder object | VERIFIED | `readonly_tools_return_placeholder` asserts strategy_list=`[]`, policy_get has chains/targets/selectors arrays (stdio_handshake.rs:141-182). tools.rs:81-135 returns corresponding payloads. Test passes. |
| 13 | (01-02) Server writes tracing logs only to stderr | VERIFIED | logging.rs:19 uses `fmt::layer().with_writer(std::io::stderr)`. `stdout_is_strict_jsonrpc` rapid-fires 4 method families + unknown tool; every stdout line must parse as JSON-RPC 2.0 via `common::recv` (stdio_handshake.rs:381-425, common/mod.rs:55-68). Test passes. |
| 14 | (01-02) Server boots with defaults when no config.toml present | VERIFIED | config.rs:46-76 returns `Config::default()` when no path resolves to an existing file. Unit test `default_log_level_is_info` in config.rs:83-86 passes. `cargo test` runs the binary via harness without config.toml and succeeds. |
| 15 | (01-03) resources/list returns empty; resources/templates/list returns 3 URIs; resources/read returns -32002+data.phase=1 | VERIFIED | See truth #3. All three assertions in `resources_surface_matches_contract` pass. |
| 16 | (01-03) prompts/list returns write_evm_strategy + review_evm_strategy with arg schemas; prompts/get returns Phase 7 placeholder message | VERIFIED | See truth #4. `prompts_surface_matches_contract` also asserts both prompt bodies contain "Phase 7" or "body will be finalized" markers. Test passes. |
| 17 | (01-03) Every stdout line is valid JSON-RPC 2.0; schema round-trip confirms 7 structs deserialize from sample payloads | VERIFIED | `stdout_is_strict_jsonrpc` exercises 4 method families + unknown tool. `schema_contract_round_trip` deserializes 7 sample payloads into the corresponding structs (stdio_handshake.rs:432-469). Both tests pass. |

**Score:** 17/17 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `Cargo.toml` | Workspace root, 4 members, workspace deps, clippy lints | VERIFIED | 4 members listed, rmcp/schemars/tokio/serde/tracing workspace deps present, print_stdout/print_stderr/dbg_macro = deny in workspace.lints.clippy |
| `rust-toolchain.toml` | channel = "stable" | VERIFIED | Line 2: `channel = "stable"`; rustfmt+clippy components |
| `crates/executor-core/src/schema/strategy.rs` | StrategyRegisterInput/IdInput/RunOnceInput with JsonSchema derive | VERIFIED | 3 structs with full derive chain including JsonSchema |
| `crates/executor-core/src/schema/policy.rs` | PolicyUpdateInput | VERIFIED | Struct present with JsonSchema derive + optional metadata field |
| `crates/executor-core/src/schema/execution.rs` | ExecutionIdInput | VERIFIED | Struct present with JsonSchema derive; SignedTransaction placeholder present |
| `crates/executor-core/src/schema/prompt_args.rs` | WriteEvmStrategyArgs | VERIFIED | Struct present; ReviewEvmStrategyArgs also present |
| `crates/executor-core/tests/schema_snapshots.rs` | schema_for! golden harness | VERIFIED | 7 `#[test]` fns using `schema_for!`; UPDATE_SCHEMAS env toggle |
| `crates/executor-mcp/tests/common/mod.rs` | spawn_server/send/recv/initialize helpers | VERIFIED | All 4 `pub async fn` present; recv asserts JSON-RPC 2.0 |
| `crates/executor-mcp/tests/stdio_handshake.rs` | mod common + integration tests | VERIFIED | 8 `#[tokio::test]` fns; mod common declared |
| `crates/executor-mcp/src/server.rs` | ExecutorServer struct + get_info + #[tool_handler] | VERIFIED | Struct present; get_info declares enable_tools/prompts/resources; #[tool_handler] + #[prompt_handler] on same `impl ServerHandler` block (Pitfall 6) |
| `crates/executor-mcp/src/tools.rs` | #[tool_router] impl + 8 handlers | VERIFIED | `#[tool_router(vis = "pub(crate)")]` + 8 tools with `#[tool(name=...)]` |
| `crates/executor-mcp/src/errors.rs` | unimplemented_err helper | VERIFIED | Function present using `ErrorCode(-32010)` + structured `data` payload |
| `crates/executor-mcp/src/config.rs` | Config/LoggingConfig + load() with EXECUTOR_CONFIG | VERIFIED | Both structs with deny_unknown_fields; load() honours --config > EXECUTOR_CONFIG > ./config.toml > default |
| `crates/executor-mcp/src/logging.rs` | tracing subscriber stderr-only | VERIFIED | Line 19: `fmt::layer().with_writer(std::io::stderr)` |
| `crates/executor-mcp/src/main.rs` | tokio::main entry wiring serve(stdio()) | VERIFIED | Line 12: `ExecutorServer::new().serve(stdio()).await?` |
| `config.example.toml` | [logging] sample | VERIFIED | Present at repo root with [logging] level comment |
| `crates/executor-mcp/src/prompts.rs` | #[prompt_router] + 2 prompt handlers | VERIFIED | `#[prompt_router(vis = "pub(crate)")]` + write_evm_strategy + review_evm_strategy |
| `crates/executor-mcp/src/resources.rs` | URI templates + read_resource helper | VERIFIED | Three URI templates including `strategy://{strategy_id}`; read_resource returns -32002 + data.phase=1; does NOT use `..Default::default()` |
| `crates/executor-core/tests/schemas/*.json` (7 goldens) | All 7 committed | VERIFIED | StrategyRegisterInput/IdInput/RunOnceInput, ExecutionIdInput, PolicyUpdateInput, WriteEvmStrategyArgs, ReviewEvmStrategyArgs all present in tests/schemas/ |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| Cargo.toml | crates/executor-mcp/Cargo.toml | workspace member + workspace deps | WIRED | member listed at line 3; rmcp.workspace=true on executor-mcp side (inherits from workspace.dependencies) |
| crates/executor-mcp/Cargo.toml | crates/executor-core | path dependency | WIRED | executor-mcp Cargo.toml declares `executor-core = { path = "../executor-core" }` (implicit from tools.rs successfully importing `executor_core::schema::*`) |
| crates/executor-core/src/schema/strategy.rs | schemars::JsonSchema | derive | WIRED | `#[derive(..., JsonSchema)]` on all 3 strategy input structs |
| crates/executor-mcp/src/main.rs | rmcp::transport::stdio | ServiceExt::serve | WIRED | `ExecutorServer::new().serve(stdio()).await?` on line 12 |
| crates/executor-mcp/src/tools.rs | executor_core::schema::* | Parameters<T: JsonSchema> | WIRED | `Parameters<StrategyRegisterInput>`, `Parameters<StrategyIdInput>`, etc. throughout tools.rs |
| crates/executor-mcp/src/logging.rs | std::io::stderr | tracing_subscriber fmt layer | WIRED | `fmt::layer().with_writer(std::io::stderr)` on line 19 |
| crates/executor-mcp/tests/stdio_handshake.rs | crates/executor-mcp/src/tools.rs | spawn bin + tools/list round-trip | WIRED | `tools_list_emits_full_surface` calls `method: tools/list` via spawned bin |
| crates/executor-mcp/src/server.rs | rmcp::ServerHandler | #[tool_handler] + #[prompt_handler] | WIRED | Both attribute macros on one `impl ServerHandler for ExecutorServer` block (lines 53-55) |
| crates/executor-mcp/src/prompts.rs | executor_core::schema::prompt_args | Parameters<Write/ReviewEvmStrategyArgs> | WIRED | Both prompts use `Parameters<WriteEvmStrategyArgs>` / `Parameters<ReviewEvmStrategyArgs>` |
| crates/executor-mcp/src/resources.rs | rmcp::model::ResourceTemplate | uri_template string | WIRED | `RawResourceTemplate::new(uri_template, name)` chain + `strategy://{strategy_id}` etc. |

### Data-Flow Trace (Level 4)

Not applicable in the traditional dynamic-data sense — this phase produces a Rust binary whose "data" is the MCP surface contract, not fetched/stored records. Spot-checks (below) directly exercise the wire contract end-to-end.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Workspace compiles cleanly | `cargo check --workspace` | Finished in 0.75s, 0 errors | PASS |
| Clippy with -D warnings | `cargo clippy --workspace --all-targets -- -D warnings` | 0 warnings | PASS |
| Full test suite | `cargo test --workspace` | 20 passed across 11 suites (7 schema goldens + 5 unit + 8 stdio) | PASS |
| 8 stdio integration tests | `cargo test -p executor-mcp --test stdio_handshake` | 8/8 passed (harness_compiles, tools_list_emits_full_surface, unimplemented_tools_return_phase_hint, readonly_tools_return_placeholder, resources_surface_matches_contract, prompts_surface_matches_contract, stdout_is_strict_jsonrpc, schema_contract_round_trip) | PASS |
| 7 schema golden snapshots | `cargo test -p executor-core --test schema_snapshots` | 7/7 passed | PASS |
| Stdout discipline static | `grep -rn "println!\|eprintln!\|dbg!" crates/` | Only a comment in a test file, no runtime usage | PASS |
| Pitfall 6 (both handlers on same impl) | `grep -B1 "impl ServerHandler" crates/executor-mcp/src/server.rs` | `#[tool_handler(...)]` + `#[prompt_handler(...)]` both on same `impl ServerHandler for ExecutorServer` block (server.rs:53-55) | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| MCP-01 | 01-01, 01-02, 01-03 | Server can run as stdio MCP server without writing non-MCP data to stdout | SATISFIED | Workspace clippy denylist + per-crate `#![deny]` + logging `with_writer(std::io::stderr)` + `stdout_is_strict_jsonrpc` test asserts every stdout line is valid JSON-RPC 2.0 across 4 method families + unknown-tool error |
| MCP-02 | 01-02 | Server exposes JSON-schema-backed tools for strategy, execution, policy operations | SATISFIED | 8 tools with inputSchema verified by `tools_list_emits_full_surface`; 7 schema goldens committed + round-trip verified by `schema_contract_round_trip` |
| MCP-03 | 01-03 | Server exposes resources for strategy details, execution reports, journal entries | SATISFIED | 3 URI templates (strategy/execution/journal) in `resources/templates/list` verified by `resources_surface_matches_contract`; `resources/read` returns structured -32002 with data.phase=1 |
| MCP-04 | 01-03 | Server exposes prompts for writing and reviewing EVM automation strategies | SATISFIED | 2 prompts (write_evm_strategy, review_evm_strategy) with JsonSchema-derived argument shapes verified by `prompts_surface_matches_contract`; prompts/get returns placeholder PromptMessage referencing Phase 7 |

All 4 required requirement IDs are accounted for across the three plans, and REQUIREMENTS.md's traceability table matches (MCP-01..04 all mapped to Phase 1 with status "Complete").

### Anti-Patterns Found

No blockers. The REVIEW.md captured 2 warnings + 4 info items — these are ergonomic/quality observations that do not block the phase goal. Summarised for record:

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| crates/executor-mcp/src/logging.rs | 14-22 | `init` returns `Result<()>` but no fallible path; invalid log levels silently accepted | Info (REVIEW WR-01) | Ergonomics — does not affect Phase 1 goal; tighten in follow-up |
| crates/executor-signer/src/lib.rs | 4-17 | `_SignedTransactionAlias` used to keep import alive | Info (REVIEW WR-02) | Fragile workaround but does not block goal; re-export cleanly later |
| crates/executor-mcp/src/config.rs | 46-57 | `--config=PATH` form not supported (only `--config PATH`) | Info (REVIEW IN-01) | Foot-gun for automation; doc or fix in follow-up |
| crates/executor-mcp/tests/stdio_handshake.rs | 465-467 | Redundant `is_object` loop after `from_value` calls | Info (REVIEW IN-02) | Test cleanup only |
| crates/executor-core/src/schema/action.rs | 10-14 | `Action::Noop` placeholder could leak into persisted state before Phase 4 migrates | Info (REVIEW IN-03) | No state persisted in Phase 1, so not currently leaking |
| crates/executor-core/src/schema/policy.rs | 8-12 | `Option<Value>` schema does not explicitly encode null support | Info (REVIEW IN-04) | Phase 5 design note |

No critical or blocker patterns. `cargo clippy --workspace --all-targets -- -D warnings` is clean.

### Human Verification Required

None. The phase goal is fully proven by automated tests that drive the actual stdio MCP binary end-to-end (initialize → tools/list → tools/call → resources/list → resources/templates/list → resources/read → prompts/list → prompts/get + schema round-trip + stdout-purity assertion on every response). VALIDATION.md notes an optional manual sanity check with Claude Desktop / MCP Inspector, but the automated stdio surface is equivalent for phase-gate purposes.

### Gaps Summary

No gaps. All 4 ROADMAP Success Criteria are backed by concrete passing tests that exercise the real rmcp server over a spawned stdio process. All 4 requirement IDs (MCP-01..04) are satisfied. rmcp 1.5 Pitfall 6 is honoured (both `#[tool_handler]` and `#[prompt_handler]` attached to a single `impl ServerHandler` block). Stdout discipline (D-05) is enforced at three independent layers: workspace clippy deny list, per-crate `#![deny(...)]`, and `common::recv` JSON-RPC assertion exercised by `stdout_is_strict_jsonrpc`. `cargo check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass.

---

*Verified: 2026-04-24T10:30:00Z*
*Verifier: Claude (gsd-verifier)*
