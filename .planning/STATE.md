---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
stopped_at: Plan 04-03 complete; Phase 04 in progress (3/4 plans)
last_updated: "2026-04-27T10:30:00.000Z"
last_activity: 2026-04-27
progress:
  total_phases: 7
  completed_phases: 3
  total_plans: 13
  completed_plans: 12
  percent: 92
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-04-24)

**Core value:** AI agent가 EVM 자동화 로직을 실제 온체인 실행으로 바꾸되, 모든 실행은 policy 검사를 거치고 기록으로 남아야 한다.  
**Current focus:** Phase 02 — strategy-state-and-journal

## Current Position

Phase: 04 (evm-context-and-actions) — IN PROGRESS
Plan: 3 of 4 complete
Status: Plan 04-03 closed (5 Action variants + builders + validator widening + D-16 flip — CTX-05/06/07/08 wired); next is 04-04 (units/address + negative grid + schema goldens)
Last activity: 2026-04-27

Progress: [████████▌░] ~85% across 13 planned plans (11/13)

## Performance Metrics

**Velocity:**

- Total plans completed: 6
- Average duration: ~5 min
- Total execution time: ~0.3 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 3 | - | - |
| 02 | 1/3 | ~6.5 min | ~6.5 min |

**Recent Trend:**

- Last 5 plans: 01-01 (6 min, 3 tasks, 23 files created), 01-02 (6 min, 3 tasks, 13 created + 3 modified, 5 auto-fixed deviations), 01-03 (4 min, 2 tasks, 2 created + 3 modified, 4 auto-fixed deviations), 02-01 (~6.5 min, 3 tasks, 16 created + 9 modified, 0 deviations — plan executed exactly)
- Trend: zero deviations on 02-01 (planning artifacts were dense enough to drive every decision); plan size grew slightly (storage layer + schemas + config in one wave) but velocity steady

| Phase 02 P02 | 480 | 3 tasks | 11 files |
| Phase 02 P03 | 5 | 2 tasks | 4 files |
| Phase 03 P01 | ~8 min | 3 tasks | 8 created + 2 modified |
| Phase 03 P02 | 25 | 3 tasks | 16 files |
| Phase 03 P03 | ~12 min | 3 tasks | 3 created + 9 modified + 1 deleted |
| Phase 04 P01 | ~30 min | 3 tasks | 14 created + 14 modified |
| Phase 04 P02 | ~10 min | 2 tasks | 6 created + 3 modified |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.

Recent decisions affecting current work:

- v1 is a local EVM automation programming runtime, not a dashboard or hosted product.
- Strategy language is plain JavaScript over a small `ctx` API.
- Strategy output is `Action[]`.
- v1 uses local signer managed execution; external signer/detached execution is deferred.
- Workspace lints require `[lints] workspace = true` in every crate's Cargo.toml to propagate — added in 01-01.
- `executor-core` stays pure-domain (no rmcp dep) so persistence/signer/EVM crates can reuse it freely — locked in by 01-01.
- Integration-test common module uses `#![allow(dead_code, unreachable_pub)]` so Plan 02/03 can adopt only the helpers they need.
- **Unimplemented wire code = -32010 (primary path).** rmcp 1.5's `ErrorCode(pub i32)` tuple constructor is public, so the fallback `McpError::internal_error` (-32603) is not needed. Locked in 01-02.
- **PromptRouter init = `PromptRouter::new()` (primary path).** Constructor is public in rmcp 1.5. Plan 03 swaps to `Self::prompt_router()` after adding a `#[prompt_router]` impl block.
- **`#[tool_router(vis = "pub(crate)")]`** required because `server.rs` calls the generated `Self::tool_router()` across the module boundary.
- **`#[tool_handler(router = self.tool_router)]`** (not the default `Self::tool_router()`) keeps the stored router field hot and mirrors Plan 03's `#[prompt_handler(router = self.prompt_router)]`.
- **`#[prompt_router(vis = "pub(crate)")]` + `#[prompt_handler(router = self.prompt_router)]`** applied symmetrically to tools in 01-03. Both handlers live on one `impl ServerHandler` block (Pitfall 6).
- **ResourceTemplate construction via `Annotated::new(RawResourceTemplate::new(...).with_description(...).with_mime_type(...), None)`.** PLAN RESOLVED #5 Fallback 2. Neither rmcp 1.5 type derives Default; `ResourceTemplate = Annotated<RawResourceTemplate>`. Phase 2+ reuses the `make_template` helper in `resources.rs`.
- Plan 02-02: Combined Tasks 1+2 into one commit per plan deviation note (server.rs state field forces tools/resources update in lockstep)
- Plan 02-02: ReadResourceResult is #[non_exhaustive] in rmcp 1.5 → use ::new(vec![..]) constructor; struct literal fails E0639
- Plan 02-02: Resource-boundary malformed strategy id surfaces as resource_not_found (-32002) with data.code=malformed_id, NOT as -32602 invalid_params (resources/read keeps its typed not_found contract)
- Plan 02-02: Default for ExecutorServer + no-arg new() removed; new(&StateConfig) is fallible because SQLite open can fail
- Plan 02-03: Adopted Option A test-only StateStore::__test_insert_run_with_time helper for deterministic ordering tests; Option B sleep-based was rejected (≥2s flake-prone)
- Plan 02-03: list_runs_for_strategy ORDER BY changed from DESC (Plan 02-01 vestigial) to ASC, id ASC per D-04b — id tie-breaker handles same-second now_rfc3339 collisions
- Plan 02-03: RunStatus future-variants walker collects BOTH enum[] strings and const strings — schemars 1.x emits oneOf:[{enum:[4]},const,const,const] not flat enum[7]
- Phase 02 complete: STJ-02 closed; STR-01/STR-02/STJ-01 still tracked (planning artifact lifecycle vs runtime emission distinction)
- Plan 03-01: `Context::base` cannot evaluate user JS (ships only base objects, no Eval intrinsic). Use `Context::builder().with::<intrinsic::All>()` — covers Eval/Promise/JSON/etc but still excludes module/import/require (those only ride on `Context::full`). D-11 invariant preserved.
- Plan 03-01: `intrinsic::Promise` exposes `queueMicrotask` on globalThis. A JS prelude scrub (`delete globalThis[name]` for the entire D-11 list) runs before user source — defensive against future intrinsic additions.
- Plan 03-01: Dynamic `import()` with no module loader returns a rejected Promise (does NOT throw). Test asserts via D-10 InvalidOutput(promise) path.
- Plan 03-02: STRICT D-12 — Succeeded → * disallowed at the transition guard (terminal state); idempotent re-finalize via COALESCE deferred.
- Plan 03-02: ctx.log buffer uses Rc<RefCell<Vec<String>>>; rquickjs Function::new requires only 'js (not Send + 'static), single-threaded inside ctx.with — no Arc/Mutex needed.
- Plan 03-02: tokio sync feature pinned per-crate in strategy-js (workspace tokio omits sync). Mirrors executor-mcp's per-crate feature additions.
- Plan 03-03: per-call Sandbox::execute construction (no pooling) — Plan 03-01 measurement of Runtime::new() stays well below the 50 ms threshold.
- Plan 03-03: EngineInit -> map_runtime_error("exception", ...) so agents see exactly four kinds (timeout/oom/stack_overflow/exception).
- Plan 03-03: kept unimplemented_tools_return_phase_hint with one case (policy_update) instead of deleting — preserves regression-detection lattice.
- Plan 03-03: log-message ordering test asserts HashSet membership (ULID monotonicity within the same millisecond is not guaranteed by Ulid::new()).
- Plan 03-03: journal:// resource serialises JournalActionOutcome via serde_json::to_value (NEVER format!("{:?}",..) which corrupts SimulationFailure -> "simulationfailure").
- Plan 04-01: alloy 2.0.1 verified via `cargo tree -p executor-evm`; per-crate dep pinning preserved (NOT promoted to workspace.dependencies until Phase 5 adds executor-mcp as second consumer).
- Plan 04-01: alloy 2.0 requires `Provider` trait in scope for `.erased()`; `JsonAbiExt + FunctionExt` for `abi_encode_input/abi_decode_output` — added explicit trait imports.
- Plan 04-01: rquickjs `Function::new` closure for `Object<'js> → Value<'js>` needs explicit `for<'js>` higher-rank lifetime via a helper `fn make_..._closure(...) -> impl for<'js> Fn(...) + 'static` — Value<'js> is invariant.
- Plan 04-01: NOTE-3 logged — empty-bytes "calling missing contract" surfaces as `evm_decode_error` (not `evm_revert`); custom-revert errors with non-`Error(string)` selector surface as `evm_revert{reason:"unknown"}`. Both shapes accepted by tests.
- Plan 04-01: ctx.evm.readContract uses `Handle::try_current() + block_in_place + handle.block_on()` inside spawn_blocking; falls back to a transient current-thread runtime for sync unit tests with no ambient runtime.
- Plan 04-01: `ctx_object_shape_matches_d04` Phase-3 test updated to include `"evm"` key — expected breaking change as Phase 4 adds the namespace.
- Plan 04-01: executor-evm re-exports `alloy::providers::DynProvider` so executor-mcp / strategy-js can name `Arc<DynProvider>` without direct alloy deps (D-02 isolation).
- Plan 04-02: ERC20 helpers are thin wrappers around `read_contract` with a bundled OZ-compatible `ERC20_ABI` static (selector-stable, balanceOf=0x70a08231 pinned by unit test) — strategies never supply their own ABI for ERC20 reads.
- Plan 04-02: Native helpers bypass dyn-abi entirely — `Provider::get_balance` + `Provider::get_block_number` direct calls, with U256→decimal-string per D-03 (78-digit max never fits JS Number).
- Plan 04-02: Flat aliases (`erc20Balance`, `erc20Allowance`, `nativeBalance`) and structured forms (`readErc20.*`, `readNative.*`) are SEPARATE JS Function objects but route to the SAME backing executor_evm fn — identical results AND identical journal payloads (T-04-02-01 mitigation).
- Plan 04-02: Default blockTag = "latest" when arg missing OR `undefined` (NOTE-2 plan-checker — pinned by `flat_alias_default_blockTag_is_latest` test).
- Plan 04-02: Helper positional shape is `(token, ...addresses, blockTag?)` — NOT options-object — to match REQUIREMENTS naming verbatim.
- Plan 04-02: `BlockTag::to_block_id` made `pub` so `executor_evm::native` can translate the agent-facing tag enum into alloy `BlockId` for `get_balance.block_id(...)`.
- Plan 04-02: Each helper records ONE `journal_source_reads` row with kind="evm_read", target="<lower_address>:<helper_function>", payload.helper = structured-form name (NOT alias name) — flat aliases produce identical journal target/payload for identical args.

### Pending Todos

- Phase 3 complete (3/3 plans). All 5 phase requirements (STR-03/04/05, STJ-03/04) closed.
- Phase 4 — Plans 04-01 and 04-02 complete. CTX-01/02/03/04 all wired (readContract + ERC20 helpers + native helpers + flat aliases).
- Workspace: 232 tests passing across 32 suites (was 213, +19 from 04-02); clippy `--workspace --all-targets -- -D warnings` clean; sandbox_host_globals (HR-01 regression) still green with Phase-4 surfaces installed.
- Next: Plan 04-03 (action builders — ctx.actions.{contractCall, rawCall, erc20Approve, erc20Transfer, nativeTransfer} + Action enum widening + per-Action validator).

### Blockers/Concerns

- GSD subagents may be unavailable or misconfigured in this environment; prefer local orchestration unless fixed.
- Local private-key signer must be treated as hot-wallet custody with strong defaults.

## Deferred Items

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| Execution | External signer / detached execution | Deferred | Initialization |
| Runtime | Scheduler / reconcile loops | Deferred | Initialization |
| DX | TypeScript compiler | Deferred | Initialization |
| Product | Dashboard / marketplace | Deferred | Initialization |

## Session Continuity

Last session: 2026-04-27T09:33:07.000Z
Stopped at: Plan 04-02 complete; Phase 04 in progress (2/4 plans). CTX-02/03/04 all closed. Workspace 232 tests / clippy clean.
Resume file: None

**Planned Phase:** 1 (mcp-runtime-surface) — 3 plans — 2026-04-24T09:01:09.909Z (COMPLETE)
