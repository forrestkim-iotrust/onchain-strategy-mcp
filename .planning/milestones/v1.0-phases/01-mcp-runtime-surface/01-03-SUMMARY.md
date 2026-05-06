---
phase: 01-mcp-runtime-surface
plan: 03
subsystem: mcp-server
tags: [rust, rmcp, prompt-handler, resource-templates, stdout-discipline, schema-contract, phase-gate]

requires:
  - 01-mcp-runtime-surface/01 (workspace + schemas + harness)
  - 01-mcp-runtime-surface/02 (ExecutorServer + tool_handler + 8 tools)
provides:
  - `executor_mcp::prompts` module — `#[prompt_router(vis = "pub(crate)")] impl ExecutorServer` with 2 placeholder prompts (write_evm_strategy, review_evm_strategy) bound to Write|ReviewEvmStrategyArgs via Parameters<T>
  - `executor_mcp::resources` module — `list_resources_impl` / `list_resource_templates_impl` / `read_resource_impl` helpers; 3 URI templates (strategy://, execution://, journal://) + always-`resource_not_found` read
  - `ExecutorServer` `impl ServerHandler` block now carries `#[tool_handler(router = self.tool_router)]` + `#[prompt_handler(router = self.prompt_router)]` on ONE block (Pitfall 6 satisfied); hand-written `list_resources` / `list_resource_templates` / `read_resource` overrides delegate to `resources` module
  - 4 new integration tests in `tests/stdio_handshake.rs`: `resources_surface_matches_contract`, `prompts_surface_matches_contract`, `stdout_is_strict_jsonrpc`, `schema_contract_round_trip`
affects:
  - Phase 2 (strategy_register / strategy_delete / strategy_get move from placeholder to real state repo; populate resources/list + resources/read via `strategy://` URIs)
  - Phase 5 (policy_update + policy_get wire to real engine)
  - Phase 6 (strategy_run_once + execution_get; populate `execution://` + `journal://` resources)
  - Phase 7 (replace placeholder prompt bodies once ctx API stabilizes)

tech-stack:
  added: []    # all deps inherited from 01-01 / 01-02
  patterns:
    - "#[prompt_router(vis = \"pub(crate)\")] lives in prompts.rs so server.rs can call Self::prompt_router() across module boundary — mirrors the tool_router setup from 01-02."
    - "#[prompt_handler(router = self.prompt_router)] on the same impl ServerHandler block as #[tool_handler] — Pitfall 6 satisfied."
    - "ResourceTemplate is `Annotated<RawResourceTemplate>` on rmcp 1.5; neither struct derives Default, so construction uses `Annotated::new(RawResourceTemplate::new(uri, name).with_description(..).with_mime_type(..), None)` — no `..Default::default()` shortcut (PLAN RESOLVED #5 Fallback 2)."
    - "Integration tests use common::recv which asserts every stdout line parses as JSON-RPC 2.0 with jsonrpc:\"2.0\" — stdout_is_strict_jsonrpc rapid-fires 4 methods + an unknown tool call to exercise the assertion across the full Phase 1 surface."
    - "schema_contract_round_trip complements schema_snapshots: goldens prove the JsonSchema output is stable, serde_json::from_value proves a representative payload still deserializes into the struct."

key-files:
  created:
    - crates/executor-mcp/src/prompts.rs
    - crates/executor-mcp/src/resources.rs
  modified:
    - crates/executor-mcp/src/lib.rs
    - crates/executor-mcp/src/server.rs
    - crates/executor-mcp/tests/stdio_handshake.rs

key-decisions:
  - "ResourceTemplate construction: `Annotated::new(RawResourceTemplate::new(...).with_description(...).with_mime_type(...), None)`. rmcp 1.5 defines `pub type ResourceTemplate = Annotated<RawResourceTemplate>;` where `Annotated<T> = { raw: T, annotations: Option<Annotations> }`. Neither side derives Default, so the PLAN's primary struct-literal + `..Default::default()` path was infeasible (PLAN RESOLVED #5 Fallback 2 adopted). Final RawResourceTemplate field set: uri_template: String, name: String, title: Option<String>, description: Option<String>, mime_type: Option<String>, icons: Option<Vec<Icon>> — NO `annotations` on the raw (those live on the Annotated wrapper)."
  - "`#[prompt_router(vis = \"pub(crate)\")]` — same reason as Plan 02's `#[tool_router(vis = \"pub(crate)\")]`: without it the generated `Self::prompt_router()` associated fn inherits the impl's private visibility and `server.rs` (separate module) cannot call it (E0624). Discovered during Task 1 `cargo build`."
  - "The `#[prompt_handler]` macro expands with unqualified references to `ListPromptsResult`, `GetPromptRequestParams`, `GetPromptResult` — they must be in scope at the `impl ServerHandler` call site even though the block itself never names them. Added them to the `use rmcp::model::{...}` import group in server.rs."
  - "`#[allow(dead_code)]` on `ExecutorServer.prompt_router` removed — the `#[prompt_handler(router = self.prompt_router)]` macro now reads the field at dispatch time, so the field is hot. `cargo clippy --workspace --all-targets -- -D warnings` stays clean."
  - "`#[doc(hidden)] pub type _SignedTransactionAlias = ...` pattern (from 01-01) and `#[allow(dead_code, unreachable_pub)]` on tests/common/mod.rs (from 01-01) both still carry their weight — Plan 03 didn't need to revisit them."

patterns-established:
  - "Two-module router layout: `tools.rs` hosts `#[tool_router(vis = \"pub(crate)\")] impl ExecutorServer` with tool methods; `prompts.rs` hosts `#[prompt_router(vis = \"pub(crate)\")] impl ExecutorServer` with prompt methods; `server.rs` holds the struct + one `impl ServerHandler` block with both handler macros + hand-written resource overrides. Phase 2+ that adds a new tool only touches `tools.rs`."
  - "Resource template construction helper (`make_template` in resources.rs) centralises the `Annotated::new(RawResourceTemplate::new(..).with_...(..), None)` ritual, so Phase 2+ adding a new URI template writes one line instead of six."
  - "stdout-purity enforcement layer cake: (1) workspace clippy denylist + per-crate `#![deny(clippy::print_stdout, print_stderr, dbg_macro)]`, (2) `common::recv` asserts every stdout line is JSON-RPC 2.0 at the test level, (3) `stdout_is_strict_jsonrpc` rapid-fires 4 methods + unknown-tool error to exercise (2) across the full surface."

requirements-completed: [MCP-01, MCP-03, MCP-04]

duration: ~4min
completed: 2026-04-24
---

# Phase 1 Plan 03: Resources + Prompts + Phase Gate Summary

**Added the `prompts` (`#[prompt_router]` + 2 placeholders) and `resources` (3 URI templates + always-not-found read) surfaces, attached `#[prompt_handler]` to the same `impl ServerHandler` block as `#[tool_handler]` (Pitfall 6), and sealed the phase gate with 4 new integration tests (resources/prompts contract, stdout-strict JSON-RPC, schema round-trip).**

## Performance

- **Duration:** ~4 min
- **Started:** 2026-04-24T09:27:25Z
- **Completed:** 2026-04-24T09:31:36Z
- **Tasks:** 2
- **Files created:** 2
- **Files modified:** 3
- **Commits:** 2

## Accomplishments

- `ExecutorServer` now serves **8 tools + 2 prompts + 3 resource templates** over stdio. `initialize` → `tools/list` / `prompts/list` / `resources/list` / `resources/templates/list` / `resources/read` / `prompts/get` all round-trip as valid JSON-RPC 2.0 with stable schemas.
- Pitfall 6 honored: `#[tool_handler(router = self.tool_router)]` + `#[prompt_handler(router = self.prompt_router)]` coexist on ONE `impl ServerHandler for ExecutorServer` block, letting the macros co-generate `list_tools` / `call_tool` / `list_prompts` / `get_prompt` alongside hand-written `list_resources` / `list_resource_templates` / `read_resource`.
- 2 placeholder prompts (`write_evm_strategy`, `review_evm_strategy`) declare their arg schemas via `Parameters<Write|ReviewEvmStrategyArgs>`, so `prompts/list` publishes the JsonSchema-derived argument shapes automatically. Bodies reference "Phase 7" so agents do not mistake them for finished authoring templates (T-01-03-04 accept).
- 3 URI templates (`strategy://{strategy_id}`, `execution://{execution_id}`, `journal://{execution_id}`) appear in `resources/templates/list`; `resources/list` returns an empty array (Phase 2+ will populate); `resources/read` always returns structured `resource_not_found` (-32002) with `data.phase=1` + `data.uri` echoed (T-01-03-01 mitigated: no URI parsing performed in Phase 1).
- 4 new integration tests lock the contract down:
  - `resources_surface_matches_contract` — exact URI set, template count, -32002 shape.
  - `prompts_surface_matches_contract` — exact name set, prompt count, Phase-7 placeholder marker.
  - `stdout_is_strict_jsonrpc` — rapid-fires 4 methods + unknown-tool `tools/call`; every stdout line parsed by `common::recv` which asserts JSON-RPC 2.0 (T-01-03-02 + T-01-03-03 mitigated).
  - `schema_contract_round_trip` — `serde_json::from_value` round-trip for all 7 input structs with representative payloads (T-01-03-05 mitigated beyond the schema_snapshots goldens).
- Full workspace sweep green: `cargo test --workspace` = **20 passed** (11 suites), `cargo clippy --workspace --all-targets -- -D warnings` = **0 warnings**. The 7 schema goldens from 01-01/02 still pass un-touched.
- Phase 1 validation contract (VALIDATION.md Per-Task Verification Map rows 1-01-01 … 1-03-04) is **fully green**; `nyquist_compliant: true` can be set.

## Adopted `ResourceTemplate` Field Set (PLAN RESOLVED #5)

`rmcp 1.5` defines:

```rust
pub type ResourceTemplate = Annotated<RawResourceTemplate>;

pub struct Annotated<T: AnnotateAble> {
    pub raw: T,
    pub annotations: Option<Annotations>,
}

pub struct RawResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub icons: Option<Vec<Icon>>,
}
```

- [x] `uri_template: String`
- [x] `name: String`
- [x] `description: Option<String>`
- [x] `mime_type: Option<String>`
- [x] `annotations: Option<Annotations>` — **on `Annotated<T>` wrapper, NOT on `RawResourceTemplate`**
- [x] Extra fields discovered: `title: Option<String>`, `icons: Option<Vec<Icon>>` (both left `None` in Phase 1).
- [x] **Adopted construction:** `Annotated::new(RawResourceTemplate::new(uri, name).with_description(desc).with_mime_type(mime), None)` — builder chain, no struct-literal, no `..Default::default()` (neither type derives `Default`).

Phase 2+ adding a new template just extends the `make_template` helper in `crates/executor-mcp/src/resources.rs`.

## Task Commits

1. **Task 1: Prompt router + resource handler + server impl integration** — `dabe05f` (feat)
2. **Task 2: resources/prompts + stdout + schema round-trip integration tests** — `7bc1f8d` (test)

## Files Created

- `crates/executor-mcp/src/prompts.rs` — `#[prompt_router(vis = "pub(crate)")] impl ExecutorServer` with `write_evm_strategy` + `review_evm_strategy` placeholder prompts. Both return `GetPromptResult` with one `PromptMessage` referencing "Phase 7" so agents can detect placeholder.
- `crates/executor-mcp/src/resources.rs` — `make_template` helper + `list_resources_impl` (empty) / `list_resource_templates_impl` (3 URIs) / `read_resource_impl` (always `resource_not_found` with `data.phase=1`). Documents the Annotated/RawResourceTemplate field set in-module for Phase 2+.

## Files Modified

- `crates/executor-mcp/src/lib.rs` — declare `prompts` + `resources` modules; updated module-doc to reflect the three-way router split.
- `crates/executor-mcp/src/server.rs` — swapped `PromptRouter::new()` → `Self::prompt_router()`; added `#[prompt_handler(router = self.prompt_router)]` to the existing `impl ServerHandler` block; added `list_resources` / `list_resource_templates` / `read_resource` overrides delegating to `crate::resources`; removed the Plan-02 `#[allow(dead_code)]` on the `prompt_router` field (now hot via macro); imported `GetPromptRequestParams` / `GetPromptResult` / `ListPromptsResult` so the macro expansion references resolve.
- `crates/executor-mcp/tests/stdio_handshake.rs` — 4 new `#[tokio::test]` fns and updated module docstring. Every test drives a freshly-spawned `executor-mcp` bin over stdio; `common::recv` JSON-RPC assertion guards stdout purity on every response.

## Decisions Made

- **`ResourceTemplate` construction via builder chain.** `ResourceTemplate` is `Annotated<RawResourceTemplate>` on rmcp 1.5. Neither type derives `Default`, so the PLAN's primary struct-literal + `..Default::default()` path does not compile. Adopted PLAN RESOLVED #5 **Fallback 2**: `Annotated::new(RawResourceTemplate::new(uri, name).with_description(...).with_mime_type(...), None)`. Documented the field set + rationale inline in `resources.rs`.
- **`#[prompt_router(vis = "pub(crate)")]` (not default private).** Mirrors 01-02's `#[tool_router(vis = "pub(crate)")]`. Without it the generated `Self::prompt_router()` associated fn is private and `server.rs` (separate module) gets E0624. Discovered during Task 1 `cargo build`.
- **Import macro-expansion types in server.rs.** `#[prompt_handler]` expands to code that names `ListPromptsResult`, `GetPromptRequestParams`, `GetPromptResult` in the enclosing scope. Added them to the `use rmcp::model::{...}` import group in `server.rs` even though the hand-written block does not reference them directly. Error surfaced as E0425/E0422 from the macro-generated `ServerHandler::list_prompts` / `get_prompt` impls.
- **Doc-comment indentation tidy.** Clippy's `doc_overindented_list_items` caught a multi-space aligned bullet in the `server.rs` crate doc. Collapsed to 2-space indent to satisfy `-D warnings`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `ResourceTemplate` is `Annotated<RawResourceTemplate>` — PLAN's struct-literal path doesn't compile**
- **Found during:** Task 1 initial build (would have hit E0063 / E0277 — inspected rmcp 1.5 source before writing code).
- **Issue:** PLAN primary snippet used `ResourceTemplate { uri_template: ..., name: ..., description: ..., mime_type: ..., annotations: None }` as a direct struct literal. rmcp 1.5 source (`src/model/resource.rs`, `src/model/annotated.rs`) shows `pub type ResourceTemplate = Annotated<RawResourceTemplate>;` where `Annotated<T>` has fields `raw` + `annotations`, and `RawResourceTemplate` has NO `annotations` field (it has `title`, `icons` instead). Neither type derives `Default`.
- **Fix:** Adopted PLAN RESOLVED #5 **Fallback 2** — builder chain `RawResourceTemplate::new(uri, name).with_description(...).with_mime_type(...)` wrapped in `Annotated::new(raw, None)`. Centralised in a `make_template` helper.
- **Files modified:** `crates/executor-mcp/src/resources.rs` (created with the correct pattern from the start).
- **Verification:** `cargo build -p executor-mcp` succeeds; `resources_surface_matches_contract` passes and confirms `uriTemplate`, `name`, `description`, `mimeType` appear in the wire response.
- **Committed in:** `dabe05f` (Task 1).

**2. [Rule 3 - Blocking] `#[prompt_router]`-generated fn was private across module boundary**
- **Found during:** Task 1 `cargo build` (E0624 on `Self::prompt_router()` in `server.rs::ExecutorServer::new()`).
- **Issue:** `#[prompt_router]` inherits the impl's method visibility (private by default). `server.rs` calls `Self::prompt_router()` from a separate module.
- **Fix:** Changed to `#[prompt_router(vis = "pub(crate)")]`. Macro source (`rmcp-macros-1.5.0/src/prompt_router.rs`) confirms `vis` attribute is supported, same syntax as `#[tool_router]`.
- **Files modified:** `crates/executor-mcp/src/prompts.rs`.
- **Verification:** `cargo build -p executor-mcp` succeeds.
- **Committed in:** `dabe05f` (Task 1).

**3. [Rule 3 - Blocking] `#[prompt_handler]` macro needs model types in scope**
- **Found during:** Task 1 `cargo build` (E0425 / E0422 on `ListPromptsResult`, `GetPromptRequestParams`, `GetPromptResult` from the `#[prompt_handler]` expansion site).
- **Issue:** The macro generates `ServerHandler::list_prompts` + `get_prompt` impls inline at the attribute site. Those generated methods reference the rmcp model types by bare name, so they must be in scope of the `impl ServerHandler` block.
- **Fix:** Extended the `use rmcp::model::{...}` import group in `server.rs` to include `GetPromptRequestParams`, `GetPromptResult`, `ListPromptsResult`.
- **Files modified:** `crates/executor-mcp/src/server.rs`.
- **Verification:** `cargo build` succeeds; `prompts_surface_matches_contract` confirms `prompts/list` + `prompts/get` wire correctly.
- **Committed in:** `dabe05f` (Task 1).

**4. [Rule 1 - Bug] `doc_overindented_list_items` clippy lint on server.rs doc**
- **Found during:** Task 1 `cargo clippy --all-targets -- -D warnings`.
- **Issue:** Bullet list in the `server.rs` crate-doc used wider indentation for continuation lines, tripping `clippy::doc-overindented-list-items` which is denied under `-D warnings`.
- **Fix:** Collapsed continuation-line indentation to 2 spaces.
- **Files modified:** `crates/executor-mcp/src/server.rs`.
- **Verification:** `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Committed in:** `dabe05f` (Task 1).

---

**Total deviations:** 4 auto-fixed (3 blocking compiler errors tied to rmcp 1.5 macro plumbing + 1 clippy lint). All were mechanical fixes flagged as fallbacks by the PLAN itself (RESOLVED #5 Fallback 2; Plan 02's `vis = "pub(crate)"` pattern applied symmetrically to `#[prompt_router]`).
**Impact on plan:** No scope creep. No new modules, types, or behaviours beyond what the plan specifies. The Fallback path for `ResourceTemplate` construction is documented inline in `resources.rs` so Phase 2+ can extend it without re-discovering the `Annotated` wrapper.

## Issues Encountered

- None blocking. All deviations resolved inline during Task 1. No checkpoint returned; full workspace sweep green at HEAD.

## User Setup Required

- None. Workspace stays a pure Rust project with no external services.
- Manual sanity check per VALIDATION.md Manual-Only Verifications: `cargo build -p executor-mcp --release`, point Claude Desktop / MCP Inspector at the binary, confirm 8 tools + 2 prompts + 3 resource templates surface in the UI. Automated tests cover the stdio contract exhaustively but cannot exercise real client UX.

## Next Phase Readiness

Phase 1 is complete. Phase 2 (Strategy State and Journal) can:

1. Add a `[state]` section to `config.toml` (SQLite path, migration dir) — the existing `Config` loader uses `#[serde(deny_unknown_fields)]` so the drift is forced to be conscious.
2. Implement `executor-state` repo traits (already crate-scaffolded in 01-01). The existing `executor_core::schema::strategy::*` types are stable; Phase 2 consumes them directly.
3. Swap `strategy_register` / `strategy_delete` / `strategy_get` / `strategy_list` bodies in `crates/executor-mcp/src/tools.rs` from placeholder / `unimplemented_err` / `resource_not_found` to real state-repo calls. Integration-test harness (`common` module) and the 8 existing stdio tests stay in place as regression coverage.
4. Populate `resources/list` + `resources/read` for `strategy://{strategy_id}` URIs by extending `crates/executor-mcp/src/resources.rs::list_resources_impl` and `read_resource_impl`. The `make_template` helper already handles the `Annotated` wrapper — Phase 2 adds an `uri` parser + state lookup instead.
5. Reuse the `schema_snapshots` golden harness: any new input struct added to `executor-core::schema::*` gets a `#[test]` in `crates/executor-core/tests/schema_snapshots.rs`; running `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots` writes the golden.

Stable Phase 2+ contracts (do not break without a conscious schema bump):
- `executor_core::schema::strategy::{StrategyRegisterInput, StrategyIdInput, StrategyRunOnceInput}`
- `executor_core::schema::execution::{ExecutionIdInput, SignedTransaction}`
- `executor_core::schema::policy::PolicyUpdateInput`
- `executor_core::schema::prompt_args::{WriteEvmStrategyArgs, ReviewEvmStrategyArgs}`
- `executor_mcp::errors::unimplemented_err(tool, phase) -> McpError` (wire code -32010)
- `executor_signer::Signer` trait boundary
- 8 integration tests in `crates/executor-mcp/tests/stdio_handshake.rs` (harness_compiles, tools_list_emits_full_surface, unimplemented_tools_return_phase_hint, readonly_tools_return_placeholder, resources_surface_matches_contract, prompts_surface_matches_contract, stdout_is_strict_jsonrpc, schema_contract_round_trip)

VALIDATION.md `nyquist_compliant: true` can now be set — every row 1-01-01 through 1-03-04 is green with automated verification.

## Threat Flags

None beyond the PLAN's `<threat_model>` (T-01-03-01..05). Strengthened mitigations:
- **T-01-03-01** (resources/read path traversal): Phase 1 never parses the URI — `read_resource_impl` passes the raw string straight into the `resource_not_found` `data.uri` echo. No filesystem / state lookup exists yet. Phase 2+ must add URI scheme validation + path sanitization as a consumer of this surface.
- **T-01-03-02** (stdout contamination): three layers now in place — workspace clippy denylist, crate-level `#![deny]`, and `stdout_is_strict_jsonrpc` exercising 4 method families + unknown-tool error in one test.
- **T-01-03-03** (unknown tool spoofing): `stdout_is_strict_jsonrpc` explicitly tests `tools/call` with `name: "nonexistent_tool"` and asserts a JSON-RPC error object back (not a panic or log line).
- **T-01-03-05** (schema drift): `schema_contract_round_trip` complements `schema_snapshots` by proving representative payloads still deserialize. Goldens catch shape drift; round-trip catches serde-only drift.

## Self-Check: PASSED

- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/prompts.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/resources.rs` — FOUND
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/lib.rs` — FOUND (modified, declares `prompts` + `resources`)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/src/server.rs` — FOUND (modified, `#[tool_handler]` + `#[prompt_handler]` on one block)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/tests/stdio_handshake.rs` — FOUND (modified, 4 new tests)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/WriteEvmStrategyArgs.json` — FOUND (committed by 01-02 ahead of plan)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-core/tests/schemas/ReviewEvmStrategyArgs.json` — FOUND (committed by 01-02 ahead of plan)
- Commit `dabe05f` — FOUND
- Commit `7bc1f8d` — FOUND

---
*Phase: 01-mcp-runtime-surface*
*Completed: 2026-04-24*
