---
phase: 01-mcp-runtime-surface
plan: 02
subsystem: mcp-server
tags: [rust, rmcp, tool-router, stdio, tracing, json-schema, golden-tests]

requires:
  - 01-mcp-runtime-surface/01 (workspace + schemas + harness)
provides:
  - executor-mcp `ExecutorServer` with rmcp 1.5 `#[tool_handler]` serving 8 tools over stdio
  - `#[tool_router(vis = "pub(crate)")]` impl in `tools.rs` (4 write-capable return unimplemented, 4 read-only return placeholders)
  - `executor_mcp::errors::unimplemented_err(tool_name, phase) -> McpError` (wire code -32010, structured data)
  - `executor_mcp::config::{Config, LoggingConfig, load}` (`--config` > `EXECUTOR_CONFIG` > `./config.toml` > default)
  - `executor_mcp::logging::init` (stderr-only tracing subscriber, D-05)
  - `config.example.toml` at project root
  - 3 stdio integration tests (`tools_list_emits_full_surface`, `unimplemented_tools_return_phase_hint`, `readonly_tools_return_placeholder`)
  - 7 JSON-schema golden files under `crates/executor-core/tests/schemas/`
affects:
  - 01-03-PLAN.md (Plan 03 adds `#[prompt_handler(router = self.prompt_router)]` to the SAME `impl ServerHandler` block already carrying `#[tool_handler]`; prompt_router field already declared, PromptRouter::new() is public so constructor swap is a 1-line change once a `#[prompt_router]` impl block appears)
  - Phase 2 (strategy tools move from `unimplemented_err` to real impls in Phase 2)
  - Phase 5 (policy_update moves from unimplemented to real; policy_get returns real policy instead of placeholder)
  - Phase 6 (strategy_run_once + execution_get become real)

tech-stack:
  added: []    # rmcp/schemars/serde/tokio/tracing already in workspace from 01-01
  patterns:
    - "#[tool_router(vis = \"pub(crate)\")] on an impl block in a separate module so `server.rs` can call `Self::tool_router()` across module boundary."
    - "#[tool_handler(router = self.tool_router)] — consume the router from the field instead of the default `Self::tool_router()` fn, so the stored router instance is actually reachable via `self` (makes Plan 03's `#[prompt_handler(router = self.prompt_router)]` symmetrical)."
    - "unimplemented_err helper returns McpError with wire code -32010 + data.{code, tool, phase, hint} — agents can parse without string matching."
    - "Config loader priority: --config CLI > EXECUTOR_CONFIG env > ./config.toml > default. `#[serde(deny_unknown_fields)]` at both struct levels so Phase 2+ drift is caught at parse time."

key-files:
  created:
    - crates/executor-mcp/src/config.rs
    - crates/executor-mcp/src/logging.rs
    - crates/executor-mcp/src/errors.rs
    - crates/executor-mcp/src/server.rs
    - crates/executor-mcp/src/tools.rs
    - config.example.toml
    - crates/executor-core/tests/schemas/StrategyRegisterInput.json
    - crates/executor-core/tests/schemas/StrategyIdInput.json
    - crates/executor-core/tests/schemas/StrategyRunOnceInput.json
    - crates/executor-core/tests/schemas/ExecutionIdInput.json
    - crates/executor-core/tests/schemas/PolicyUpdateInput.json
    - crates/executor-core/tests/schemas/WriteEvmStrategyArgs.json
    - crates/executor-core/tests/schemas/ReviewEvmStrategyArgs.json
  modified:
    - crates/executor-mcp/src/lib.rs
    - crates/executor-mcp/src/main.rs
    - crates/executor-mcp/tests/stdio_handshake.rs

key-decisions:
  - "Unimplemented wire code adopted: -32010 (primary path). Verified against rmcp 1.5 source: `model::ErrorCode(pub i32)` tuple struct constructor is public, so `ErrorCode(-32010)` compiles without fallback. No need for the `McpError::internal_error` fallback (-32603) discussed in PLAN."
  - "PromptRouter initialisation adopted: `PromptRouter::new()` (primary path). Verified against rmcp 1.5 source at handler/server/router/prompt.rs:143 — the constructor is public. Plan 03 only needs to add a `#[prompt_router] impl ExecutorServer { ... }` block with prompt methods and swap the `PromptRouter::new()` line to `Self::prompt_router()`."
  - "Used `#[tool_router(vis = \"pub(crate)\")]` — without this the generated `Self::tool_router()` associated fn is private and `server.rs` (separate module) can't call it across the module boundary."
  - "Used `#[tool_handler(router = self.tool_router)]` (not the default `Self::tool_router()`) so the stored `ExecutorServer.tool_router` field is actually read at dispatch time. Makes Plan 03's prompt wiring symmetrical: `#[prompt_handler(router = self.prompt_router)]`."
  - "ServerInfo is `#[non_exhaustive]` (aliased to InitializeResult in rmcp 1.5). Used the `ServerInfo::new(caps).with_instructions(...)` builder chain instead of a struct literal."
  - "config.example.toml kept intentionally minimal — only [logging].level. Phase 2+ will add [state]/[evm]/[policy]/[signer] sections; `deny_unknown_fields` will make the drift explicit at load time."

requirements-completed: [MCP-01, MCP-02]

duration: ~6min
completed: 2026-04-24
---

# Phase 1 Plan 02: rmcp ExecutorServer + tool surface Summary

**rmcp 1.5 `ExecutorServer` serves 8 tools over stdio (4 write-capable return structured `unimplemented` errors, 4 read-only return placeholders), config loader + stderr-only tracing wired, and 3 integration tests + 7 schema goldens prove the contract.**

## Performance

- **Duration:** ~6 min
- **Started:** 2026-04-24T09:15:00Z
- **Completed:** 2026-04-24T09:21:16Z
- **Tasks:** 3
- **Files created:** 13
- **Files modified:** 3
- **Commits:** 3

## Accomplishments

- `ExecutorServer` exposes 8 tools with inputSchema + description via `#[tool_router]` macro, confirmed by the `tools_list_emits_full_surface` integration test.
- Unimplemented write tools (`strategy_register`, `strategy_delete`, `strategy_run_once`, `policy_update`) return a uniform structured error (wire code -32010, `data.code="unimplemented"`, `data.tool`, `data.phase`, `data.hint`) so agents can plan follow-ups without string-matching the message. Phase mapping locked: register=2, delete=2, run_once=6, policy_update=5.
- Read-only tools (`strategy_list`, `policy_get`) return placeholder content; `strategy_get`/`execution_get` return `McpError::resource_not_found` with `data.phase` so agents can detect which future phase fills them in.
- `config::load()` honours `--config <path>` → `EXECUTOR_CONFIG` env → `./config.toml` → built-in default in that order; 4 unit tests cover default, explicit override, and `deny_unknown_fields` at both struct levels.
- `logging::init` sets up `tracing_subscriber` with `with_writer(std::io::stderr)` so stdout stays pure JSON-RPC. Workspace clippy denylist from 01-01 provides an independent tripwire against `println!`/`eprintln!`/`dbg!`.
- `main.rs` wires the full bootstrap: `config::load()? → logging::init(&cfg)? → ExecutorServer::new().serve(stdio()).await?.waiting().await?`.
- 7 schema goldens (`tests/schemas/*.json`) committed; `cargo test -p executor-core --test schema_snapshots` passes without `UPDATE_SCHEMAS` set.
- `cargo build -p executor-mcp`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` all green at head.

## Task Commits

1. **Task 1: Config loader + stderr-only tracing + main() wiring** — `6f630c2` (feat)
2. **Task 2: ExecutorServer + 8 tool handlers + unimplemented_err helper** — `37c940e` (feat)
3. **Task 3: stdio_handshake integration tests + schema goldens** — `72a05d4` (test)

## Files Created

- `crates/executor-mcp/src/config.rs` — `Config`/`LoggingConfig` with `deny_unknown_fields`, `load()` honouring CLI/env/file/default priority. 4 unit tests.
- `crates/executor-mcp/src/logging.rs` — `init(&Config) -> Result<()>` using `tracing_subscriber::fmt::layer().with_writer(std::io::stderr)` (load-bearing).
- `crates/executor-mcp/src/errors.rs` — `unimplemented_err(tool, phase)` returning `McpError::new(ErrorCode(-32010), ..., data)` with structured `{code, tool, phase, hint}` payload. 1 unit test.
- `crates/executor-mcp/src/server.rs` — `ExecutorServer { tool_router, prompt_router }` struct, `#[tool_handler(router = self.tool_router)] impl ServerHandler`. `get_info` declares `enable_tools + enable_prompts + enable_resources` (Pitfall 5) and sets descriptive `instructions`.
- `crates/executor-mcp/src/tools.rs` — `#[tool_router(vis = "pub(crate)")] impl ExecutorServer` with 8 `#[tool(name = "...", description = "...")]` methods.
- `config.example.toml` — `[logging] level = "info"` sample at project root, with a comment about Phase 2+ extensions.
- `crates/executor-core/tests/schemas/{StrategyRegisterInput,StrategyIdInput,StrategyRunOnceInput,ExecutionIdInput,PolicyUpdateInput,WriteEvmStrategyArgs,ReviewEvmStrategyArgs}.json` — 7 schema goldens (5 in plan scope + 2 prompt-args ahead of Plan 03; see Deviations).

## Files Modified

- `crates/executor-mcp/src/lib.rs` — declare `config/errors/logging/server/tools` modules; re-export `ExecutorServer`.
- `crates/executor-mcp/src/main.rs` — replace no-op `fn main()` with `#[tokio::main] async fn main() -> Result<()>` that loads config, inits logging, and calls `ExecutorServer::new().serve(stdio()).await?.waiting().await?`.
- `crates/executor-mcp/tests/stdio_handshake.rs` — keep `harness_compiles`; add `tools_list_emits_full_surface`, `unimplemented_tools_return_phase_hint`, `readonly_tools_return_placeholder`.

## Decisions Made

- **Unimplemented wire code = -32010 (primary).** rmcp 1.5's `model::ErrorCode(pub i32)` tuple constructor is public, so the primary `ErrorCode(-32010)` path compiles directly. No need for the `McpError::internal_error` fallback (-32603) discussed in the plan. Integration test's `EXPECTED_UNIMPL_CODE = -32010` locks this in.
- **PromptRouter init = `PromptRouter::new()` (primary).** Verified at `rmcp-1.5.0/src/handler/server/router/prompt.rs:143`: the fn is `pub`. Plan 03 adds a `#[prompt_router] impl ExecutorServer { ... }` block with prompt methods and swaps the `PromptRouter::new()` line to `Self::prompt_router()` — a 1-line change plus the 2 prompt methods.
- **`vis = "pub(crate)"` on the tool_router attribute.** `#[tool_router]` defaults to the private visibility of its impl's methods, but `server.rs` (separate module) calls `Self::tool_router()` across the module boundary. `vis = "pub(crate)"` makes the generated fn visible crate-wide without exposing it to downstream crates.
- **`router = self.tool_router` on `#[tool_handler]`.** The macro default (`Self::tool_router()`) reconstructs the router each call — which would leave the stored field unread (dead_code warning). Routing through `self.tool_router` keeps the field hot and mirrors Plan 03's forthcoming `#[prompt_handler(router = self.prompt_router)]`.
- **`ServerInfo::new(caps).with_instructions(...)` instead of struct literal.** `ServerInfo = InitializeResult` is `#[non_exhaustive]` in rmcp 1.5, so struct literals fail to compile; the builder API is the sanctioned surface.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Manual `Default for Config` triggered `clippy::derivable_impls`**
- **Found during:** Task 1 `cargo clippy --workspace --all-targets -- -D warnings`.
- **Issue:** The RESEARCH.md pattern wrote `impl Default for Config` manually delegating to `LoggingConfig::default()`. Clippy flagged it as derivable because every field has a `Default` impl in scope.
- **Fix:** Removed the manual impl and added `Default` to the `#[derive(...)]` on `Config`. `LoggingConfig` kept its manual impl because it uses the `default_log_level()` fn (not a plain `Default` call).
- **Files modified:** `crates/executor-mcp/src/config.rs`.
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Committed in:** `6f630c2` (Task 1).

**2. [Rule 3 - Blocking] `#[tool_router]`-generated fn was private across module boundary**
- **Found during:** Task 2 `cargo build`.
- **Issue:** `#[tool_router]` lives on `impl ExecutorServer` in `tools.rs`, whose methods are private. The generated `Self::tool_router()` inherited that private visibility, so `server.rs::ExecutorServer::new()` got E0624 "associated function is private".
- **Fix:** Changed to `#[tool_router(vis = "pub(crate)")]`. The macro supports a `vis` attribute specifically for this case; verified in `rmcp-macros-1.5.0/src/lib.rs` examples.
- **Files modified:** `crates/executor-mcp/src/tools.rs`.
- **Verification:** `cargo build -p executor-mcp` succeeds.
- **Committed in:** `37c940e` (Task 2).

**3. [Rule 3 - Blocking] `ServerInfo` is `#[non_exhaustive]`, struct literal fails**
- **Found during:** Task 2 `cargo build`.
- **Issue:** Initial `get_info` used a `ServerInfo { capabilities, server_info, instructions, ..Default::default() }` struct literal; rmcp 1.5's `ServerInfo` (aliased to `InitializeResult`) is `#[non_exhaustive]` so E0639 fires.
- **Fix:** Switched to `ServerInfo::new(caps).with_instructions(...)` builder chain, dropping the unused `Implementation` import.
- **Files modified:** `crates/executor-mcp/src/server.rs`.
- **Verification:** Build succeeds; `get_info` still declares all three capabilities + the instruction string required by Pitfall 5.
- **Committed in:** `37c940e` (Task 2).

**4. [Rule 1 - Bug] `prompt_router` field dead_code until Plan 03**
- **Found during:** Task 2 `cargo build`.
- **Issue:** After switching `#[tool_handler]` to `router = self.tool_router`, the `tool_router` field became hot but `prompt_router` stayed unread because there's no `#[prompt_handler]` yet. Under `-D warnings` this breaks CI.
- **Fix:** Scoped `#[allow(dead_code)]` on the `prompt_router` field only, with an inline comment pointing at Plan 03. Plan 03 will replace the attribute with a real consumer (`#[prompt_handler(router = self.prompt_router)]`) and can drop the allow.
- **Files modified:** `crates/executor-mcp/src/server.rs`.
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Committed in:** `37c940e` (Task 2).

**5. [Rule 2 - Missing Critical] Committed 2 prompt-args schema goldens ahead of Plan 03**
- **Found during:** Task 3 — running `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots`.
- **Issue:** Plan 01-01's `schema_snapshots.rs` already declares `#[test]` fns for `WriteEvmStrategyArgs` and `ReviewEvmStrategyArgs` (Plan 03 scope), so `UPDATE_SCHEMAS=1` writes all 7 goldens in one go. If Plan 02 committed only the 5 tool-input goldens listed in `files_modified`, the next `cargo test --workspace` would fail because the 2 prompt goldens would be missing while the tests remain.
- **Fix:** Committed all 7 goldens. The 2 prompt ones are read-only until Plan 03 may need to refresh them after adding prompt methods (but field names are already fixed on the structs, so the JSON shouldn't drift).
- **Files created:** `crates/executor-core/tests/schemas/{WriteEvmStrategyArgs,ReviewEvmStrategyArgs}.json`.
- **Verification:** `cargo test -p executor-core --test schema_snapshots` passes with 7 tests.
- **Committed in:** `72a05d4` (Task 3).

---

**Total deviations:** 5 auto-fixed (1 clippy lint, 2 blocking compiler errors, 1 dead-code trigger, 1 missing-critical artifact).
**Impact on plan:** All fixes were mechanical and required for the plan's verify gates (`-D warnings` + cross-module visibility + `cargo test --workspace`) to pass. No scope creep — no new modules, tools, or behaviours beyond what the plan specifies. Two decisions the plan flagged as "executor determines at Wave 2 start" are now concrete: wire code = -32010 (primary), PromptRouter init = `PromptRouter::new()` (primary).

## Issues Encountered

- None blocking. The 5 deviations above were all resolved inline during their owning task. No checkpoint returned, no follow-up required.

## User Setup Required

- None. Plan works with the existing workspace; no new environment variables, credentials, or external services.
- To sanity-check manually against a real client, build with `cargo build -p executor-mcp --release` and point Claude Desktop / MCP Inspector at the binary (see VALIDATION.md Manual-Only Verifications table). The 3 automated stdio tests exercise the same `initialize` / `tools/list` / `tools/call` surface a real client would hit.

## Next Phase Readiness

Plan 03 can:

1. Add a `prompts.rs` module with a `#[prompt_router] impl ExecutorServer { ... }` block containing the `write_evm_strategy` and `review_evm_strategy` placeholder methods.
2. Change `server.rs::ExecutorServer::new()` `prompt_router: PromptRouter::new()` line to `prompt_router: Self::prompt_router()`.
3. Add `#[prompt_handler(router = self.prompt_router)]` to the existing `impl ServerHandler for ExecutorServer` block in `server.rs`. Per Pitfall 6 it must share the block with the existing `#[tool_handler]`.
4. Remove the `#[allow(dead_code)]` on `ExecutorServer.prompt_router` — the handler now reads it.
5. Add `list_resources`, `list_resource_templates`, `read_resource` methods to the same `impl ServerHandler` block; add 4 integration tests (`resources_surface_matches_contract`, `prompts_surface_matches_contract`, `stdout_is_strict_jsonrpc`, `schema_contract_round_trip`) to `crates/executor-mcp/tests/stdio_handshake.rs`. The `common` helpers are already wired.
6. Prompt-args goldens (`WriteEvmStrategyArgs.json`, `ReviewEvmStrategyArgs.json`) already exist under `crates/executor-core/tests/schemas/`; Plan 03 only needs to re-run `UPDATE_SCHEMAS=1` if the structs change.

No blockers. No deferred items for this plan.

## Threat Flags

None beyond what PLAN's `<threat_model>` already captured (T-01-02-01..05). The chosen wire code (-32010) lands squarely in the JSON-RPC "server-defined" range so T-01-02-02 mitigation is stronger than the fallback path would have been (distinct code separates "unimplemented" from generic "internal_error"). T-01-02-01 (stdout leakage) now has three layers: workspace clippy denylist + crate `#![deny(...)]` + integration-test `common::recv` asserting every stdout line is JSON-RPC 2.0 parseable.

## Self-Check: PASSED

- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/config.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/logging.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/errors.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/server.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/tools.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/lib.rs` — FOUND (modified)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/main.rs` — FOUND (modified)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/tests/stdio_handshake.rs` — FOUND (modified)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/config.example.toml` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/StrategyRegisterInput.json` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/StrategyIdInput.json` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/StrategyRunOnceInput.json` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/ExecutionIdInput.json` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/PolicyUpdateInput.json` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/WriteEvmStrategyArgs.json` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/ReviewEvmStrategyArgs.json` — FOUND
- Commit `6f630c2` — FOUND
- Commit `37c940e` — FOUND
- Commit `72a05d4` — FOUND

---
*Phase: 01-mcp-runtime-surface*
*Completed: 2026-04-24*
