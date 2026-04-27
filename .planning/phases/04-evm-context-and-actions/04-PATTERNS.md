---
phase: 04-evm-context-and-actions
artifact: PATTERNS
status: complete
mapped: 2026-04-27
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md           # CTX-01..09
  - AGENTS.md                            # alloy + executor-evm crate target
  - crates/strategy-js/src/sandbox.rs    # Phase-3 host-injection pattern
  - crates/strategy-js/src/runtime.rs    # Phase-3 RuntimeContext + flush
  - crates/executor-mcp/src/tools.rs     # spawn_blocking + 8-step lifecycle
  - crates/executor-mcp/src/validation.rs
  - crates/executor-core/src/schema/{action,execution}.rs
  - crates/executor-core/tests/schema_snapshots.rs
  - crates/executor-state/src/{schema,journal,store}.rs
  - .planning/phases/03-javascript-strategy-runner/03-CONTEXT.md (D-04, D-06, D-08)
  - .planning/phases/03-javascript-strategy-runner/03-REVIEW-FIX.md (HR-01, MR-01..04)
files_classified: 14
analogs_found: 13
analogs_missing: 1
---

# Phase 4: EVM Context and Actions — Pattern Map

This document is the agent-facing analog catalogue for Phase 4 plans (anticipated 04-01 EVM client + read paths, 04-02 ctx surface extension + Action variants, 04-03 MCP wiring + journal extension + tests). Every plan's tasks MUST cite the analog file:line and the convention listed here, then extend rather than rewrite.

> Phase 4 introduces alloy (RPC + ABI) and a real `Action` enum surface. The crate boundary that AGENTS.md line 33 declares (`executor-evm/`) becomes a workspace member. The `ctx.evm.*` host injection extends `Sandbox::execute` exactly where Plan 03-02 wired `ctx.strategy/run/now/log/actions.noop`. No new MCP tools — the wire-shape change reaches the agent via `Action[]` returns from the existing `strategy_run` tool plus richer `journal_source_reads` / `journal_actions` rows that `journal://{run_id}` already exposes.

---

## Crate Layout Recommendation

**Recommendation: NEW `crates/executor-evm/` crate (NOT a strategy-js extension).**

Rationale traceable to existing precedent:

| Phase | Concern | Outcome |
|-------|---------|---------|
| Phase 2 | local SQLite persistence | `executor-state/` separate from `executor-mcp` (`crates/executor-state/Cargo.toml:10-13` "per-crate-only pinning until ≥2 crates consume the same dep") |
| Phase 3 | sandboxed JS runtime | `strategy-js/` separate from `executor-state` (`crates/strategy-js/Cargo.toml:10-14` repeats the same comment block) |
| Phase 4 | alloy provider + ABI + EVM read paths | **NEW `executor-evm/`** crate, separate from `strategy-js` |

AGENTS.md line 33 lists `executor-evm/` explicitly in the target architecture. Putting alloy inside `strategy-js` would (a) re-pull rquickjs into any future signer/policy crate that needs EVM reads, and (b) couple the sandbox crate to RPC concerns, breaking the "later we want detached execution / external signer" boundary called out in PROJECT.md §Constraints "Runtime boundary".

**Workspace `members` change:** root `Cargo.toml:3` extends to `["crates/executor-mcp", "crates/executor-core", "crates/executor-state", "crates/executor-signer", "crates/strategy-js", "crates/executor-evm"]` (alphabetical-by-second-word convention not strictly observed; keep insertion-order parallel to AGENTS.md target order).

`strategy-js` then takes a path-dep on `executor-evm` (mirrors `strategy-js`'s existing `executor-state = { path = "../executor-state" }` at `crates/strategy-js/Cargo.toml:18-19`). The `CtxHost` trait in `strategy-js/src/sandbox.rs:30-36` extends with EVM-host-call methods that take `EvmCall` types defined in `executor-evm`, not in `strategy-js`.

---

## File Classification

| New / Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---------------------|------|-----------|----------------|---------------|
| `crates/executor-evm/Cargo.toml` | crate-config | n/a | `crates/strategy-js/Cargo.toml:1-35` | exact |
| `crates/executor-evm/src/lib.rs` | crate-root | n/a | `crates/strategy-js/src/lib.rs:1-30` | exact |
| `crates/executor-evm/src/error.rs` | error-taxonomy | n/a | `crates/strategy-js/src/error.rs:1-90` | exact |
| `crates/executor-evm/src/provider.rs` | service | request-response (RPC) | none — see "No Analog Found" | role-only (signer crate is too thin to mirror) |
| `crates/executor-evm/src/abi.rs` | utility | transform | none — see "No Analog Found" | n/a |
| `crates/executor-evm/src/limits.rs` (optional) | constants | n/a | `crates/strategy-js/src/limits.rs:1-56` | exact |
| `crates/strategy-js/src/sandbox.rs` (modified) | sandbox | host-injection | self (Phase-3 ctx surface install at `sandbox.rs:142-234`) | exact |
| `crates/strategy-js/src/runtime.rs` (modified) | host-state | request-response | self (`runtime.rs:31-95`) | exact |
| `crates/executor-core/src/schema/action.rs` (modified) | wire-schema | n/a | self (Phase-3 `JournalActionOutcome` future-lock at `execution.rs:58-82`) | exact |
| `crates/executor-mcp/src/validation.rs` (modified) | validator | n/a | self (`validation.rs:14-67` `validate_register` shape) | role-only (this is widening an allowlist, not creating a new validator) |
| `crates/executor-mcp/src/tools.rs` (modified) | mcp-handler | request-response | self (`tools.rs:232-351` `strategy_run`) | exact |
| `crates/executor-mcp/src/config.rs` (modified) | config-parser | n/a | self (`config.rs:44-61` `StateConfig`) | exact |
| `crates/executor-state/src/journal.rs` (modified) | repo | CRUD | self (`journal.rs:80-95` `record_source_read`) | exact |
| `crates/executor-mcp/tests/stdio_handshake.rs` (extended) + new anvil-fixture file | integration-test | full-stack | self (`stdio_handshake.rs:1118-1147` `strategy_run_returns_noop_for_minimal_strategy`) | exact |

---

## Pattern Assignments

### `crates/executor-evm/Cargo.toml` (crate-config)

**Analog:** `crates/strategy-js/Cargo.toml`

**Per-crate dep pinning pattern** (analog lines 10-14, 33-35):

```toml
[package]
name = "executor-evm"
version.workspace = true
edition.workspace = true
license.workspace = true

[lints]
workspace = true

# New deps (alloy) are intentionally NOT promoted to workspace dependencies —
# only `executor-evm` consumes them today, mirroring the Phase 1 `[logging]`-only,
# Phase 2 `rusqlite`-only, and Phase 3 `rquickjs`-only precedent of letting each
# Phase land its own isolated stack before promotion to workspace scope is
# justified. (See `crates/executor-state/Cargo.toml:10-13` for the original
# justification block.)
[dependencies]
executor-core = { path = "../executor-core" }
alloy = { version = "<pin>", default-features = false, features = ["..."] }   # planner pins exact version + minimal feature list during 04-01
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }   # for Provider's async API + spawn_blocking bridge

[dev-dependencies]
tempfile = "3"
# anvil subprocess fixtures — no crate dep; binary discovery via `which("anvil")` at runtime.
```

**Conventions to copy verbatim:**
- `version.workspace = true / edition.workspace = true / license.workspace = true / [lints] workspace = true`
- `default-features = false` on the heavyweight third-party (alloy) — same posture as `rquickjs` at `strategy-js/Cargo.toml:21`.
- Per-crate dep comment block — copy the wording near-verbatim, change "rquickjs" to "alloy" and update the precedent list.

### `crates/executor-evm/src/lib.rs` (crate-root)

**Analog:** `crates/strategy-js/src/lib.rs:1-30`

**Excerpt to mirror:**

```rust
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! `executor-evm` — alloy-backed EVM read paths for Phase-4 strategy actions.
//!
//! - `error`: typed [`EvmError`] (mapped to `RuntimeError::Exception` at the
//!   strategy-js boundary, then to MCP error codes in `executor-mcp::errors`).
//! - `provider`: alloy Provider construction + `Arc<Provider>` sharing.
//! - `abi`: ABI encode/decode helpers used by `ctx.evm.readContract` and the
//!   `contract_call` Action variant.
//!
//! Phase-5 will extend with simulation; Phase-6 with broadcast/receipt.

pub mod abi;
pub mod error;
pub mod provider;

pub use error::EvmError;
pub use provider::EvmProvider;
```

**Conventions:** crate-level `#![deny(...)]` mirrors `strategy-js/src/lib.rs:1` exactly; doc comment lists each module's responsibility; flat `pub use` re-export of the most-used types at crate root (mirrors `strategy-js/src/lib.rs:27-29`).

### `crates/executor-evm/src/error.rs` (error-taxonomy)

**Analog:** `crates/strategy-js/src/error.rs:1-45`

**Excerpt to mirror:**

```rust
#[derive(Debug, thiserror::Error)]
pub enum EvmError {
    /// RPC transport failure — connection refused, timeout, malformed response.
    #[error("evm rpc transport: {0}")]
    Transport(String),

    /// Contract call reverted — carries the revert data if available.
    #[error("evm contract reverted: {0}")]
    Reverted(String),

    /// ABI encoding / decoding failure — malformed signature or input shape.
    #[error("evm abi: {0}")]
    Abi(String),

    /// Provider-construction failure (rare — typically host-side network/io).
    #[error("evm provider init: {0}")]
    ProviderInit(String),

    /// Input validation — bad address, bad selector, etc.
    #[error("evm invalid input: {0}")]
    InvalidInput(String),
}
```

**Conventions to copy:** flat enum (no nested `data.detail` struct — that's the `executor-mcp` boundary's job); `thiserror::Error`; one variant per failure-mode that the agent can dispatch on; carries `String` not borrowed lifetimes (so it crosses `spawn_blocking` and journal payload boundaries).

**Cross-boundary mapping:** at the `strategy-js` host-call site, every `EvmError` becomes either `RuntimeError::Exception(format!("evm: {e}"))` (for transport/abi/init/reverted) or `RuntimeError::InvalidOutput { detail: e.to_string() }` (for InvalidInput). This is the same shape Phase 3 used at `crates/strategy-js/src/sandbox.rs:326-388` `caught_to_runtime_error` + `classify_message`.

### `crates/executor-evm/src/provider.rs` (service, request-response)

**Closest analog:** none in-tree — see "No Analog Found". Use this as the **invent-but-conform** template:

- Hold `Arc<Provider>` (alloy's recommended sharing model).
- Constructor takes `&EvmConfig` (rpc_url + timeout) and returns `Result<Self, EvmError>` — identical shape to `executor_state::StateStore::open(path) -> Result<_, StateError>` at `crates/executor-state/src/store.rs:23-26`.
- Public methods are `pub fn eth_call(...) -> Result<Bytes, EvmError>` — synchronous-looking signatures that internally `Handle::current().block_on(provider.call(...).await)` so the call sites inside JS host functions (`Function::new(c.clone(), move |...| -> rquickjs::Result<...>`) at `sandbox.rs:185-193`) can treat them as plain blocking calls. **Rationale below in §Async-to-Sync Bridge Pattern.**

### `crates/strategy-js/src/sandbox.rs` (modified — extend `ctx.evm.*` and `ctx.actions.*`)

**Analog:** self — Phase-3 host-injection block at `sandbox.rs:142-234`.

**Imports pattern** (sandbox.rs:10-24) — keep verbatim, add only:
```rust
use executor_evm::EvmProvider;   // Plan 04-02 brings provider into the host-binding closure
```

**Host-binding install pattern** (sandbox.rs:142-211) — extend with `ctx.evm` and additional `ctx.actions.*` builders. The exact extension point is BETWEEN the existing `ctx.actions.noop` install (`sandbox.rs:199-210`) and the D-11 scrub eval (`sandbox.rs:225-229`). DO NOT move the scrub — see "Anti-patterns" below for HR-01.

**Concrete pattern excerpt to copy** (the Phase-3 `ctx.actions.noop()` shape at `sandbox.rs:199-210`):

```rust
let actions_obj = Object::new(c.clone())
    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
let noop_fn = Function::new(c.clone(), || -> rquickjs::Result<String> {
    Ok("noop".to_string())
})
.map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
actions_obj
    .set("noop", noop_fn)
    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
ctx_obj
    .set("actions", actions_obj)
    .map_err(|e| classify_qjs_error(&c, e, &timed_out))?;
```

**Phase-4 extension pattern** (apply the same shape — `Object::new -> Function::new -> set -> set -> set`):

- `ctx.evm = Object::new(...)` with `readContract`, `erc20Balance`, `erc20Allowance`, `nativeBalance` as `Function::new` closures that call `host.eth_call(...)` (a new method on `CtxHost` — see next section).
- `ctx.actions` extends with `contractCall`, `rawCall`, `erc20Approve`, `erc20Transfer`, `nativeTransfer` as `Function::new` closures that build a `serde_json::Value` shape matching `Action::ContractCall { ... }` and push it into a host-buffered Vec via `host.record_action_builder(...)` OR (simpler) just return the constructed JSON object and let the strategy aggregate them into the returned `Action[]`.
- `ctx.units` and `ctx.address` are pure-JS helpers — install as `Object::new` with `Function::new` children that perform string-level conversions (no host-call needed — keep them inside the JS sandbox so they don't cost a `spawn_blocking` round-trip).

**JS string coercion pattern carry-over** (sandbox.rs:185-193 `Coerced<String>`): use the same `Coerced<T>` adapter for all argument-coercion at the Phase-4 boundary so JS-spec coercion is uniform with `ctx.log`.

**Type conversion (qjs Value → serde_json::Value)** uses `qjs_value_to_json` at `sandbox.rs:394-462`. Phase 4's host functions return `rquickjs::Result<rquickjs::Value>` constructed via `serde_json::to_string(...) -> Object::from_str` round-trip (cleanest for nested structs); no walker change needed.

### `crates/strategy-js/src/runtime.rs` (modified — extend `RuntimeContext` + `CtxHost` for EVM)

**Analog:** self — `runtime.rs:31-113`.

**`CtxHost` trait extension** (current trait at `sandbox.rs:30-36`):

```rust
pub trait CtxHost {
    // Phase-3 (unchanged):
    fn strategy_id(&self) -> &str;
    fn strategy_name(&self) -> &str;
    fn run_id(&self) -> &str;
    fn now_millis(&self) -> i64;
    fn append_log(&mut self, message: String);

    // Phase-4 additions — DEFAULT impls returning EvmError::ProviderInit("not configured")
    // so CtxStub doesn't need to grow EVM concerns:
    fn eth_call(&mut self, req: EvmCallRequest) -> Result<Bytes, EvmError> {
        Err(EvmError::ProviderInit("CtxHost::eth_call not provided".into()))
    }
    fn record_source_read(&mut self, kind: &str, target: &str, payload: serde_json::Value) -> Result<(), StateError> {
        Err(StateError::InvalidInput("CtxHost::record_source_read not provided".into()))
    }
}
```

**Default-impl rationale:** mirrors how Phase-3 added `append_log(&mut self, String)` as a required method but avoided expanding `CtxStub`'s scope. `CtxStub` (sandbox.rs:40-66) is the test-only impl; default impls keep the test-only stub minimal while `RuntimeContext` (the production impl at `runtime.rs:97-113`) overrides each.

**`RuntimeContext::flush` extension** (runtime.rs:77-94) — Phase 4 adds journal payloads via the same `state.blocking_lock()` block. Pattern is identical:

```rust
pub fn flush(&mut self) -> Result<(), StateError> {
    let mut store = self.state.blocking_lock();
    // 1. Source-read marker (STJ-03) — UNCHANGED.
    if self.source_read_pending { ... }
    // 2. Logs — UNCHANGED.
    for msg in self.log_buffer.drain(..) { store.record_log(&self.run_id, &msg)?; }
    // 3. Phase-4: drain the buffered EVM-read marker rows.
    for entry in self.evm_read_buffer.drain(..) {
        store.record_source_read(&self.run_id, &entry.kind, &entry.target, Some(&entry.payload_json))?;
    }
    Ok(())
}
```

**Convention:** `record_*` calls happen in the `flush()` step on the same `MutexGuard`, so we never re-acquire mid-strategy. For `eth_call` itself the host call goes through the provider DURING execution (see Async-to-Sync bridge), but the JOURNAL row is buffered and recorded in `flush()` — same discipline as `ctx.log` (RESEARCH Pitfall 2 carry-over from Phase 3).

### `crates/executor-core/src/schema/action.rs` (modified — fill in real variants)

**Analog:** self — Phase-3 `JournalActionOutcome` future-lock pattern at `execution.rs:58-82`.

**Current placeholder** (action.rs:1-15):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Noop,
    // TODO(phase-4): ContractCall, RawCall, Erc20Approve, Erc20Transfer, NativeTransfer
}
```

**Phase-4 extension pattern** — mirror `JournalActionOutcome` exactly:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Noop,
    ContractCall { /* address, selector, calldata, chain_id, value */ },
    RawCall { /* address, calldata, chain_id, value */ },
    Erc20Approve { /* token, spender, amount, chain_id */ },
    Erc20Transfer { /* token, to, amount, chain_id */ },
    NativeTransfer { /* to, value, chain_id */ },
}
```

**Field type conventions (planner pins during 04-02):**
- All EVM addresses as `String` (lower-case 0x-prefixed 40-hex) on the wire — DO NOT use `alloy::primitives::Address` directly in the schema; that would couple `executor-core` (currently dep-light) to alloy and break the Phase-1 dep posture.
- All `value` / `amount` fields as decimal `String` — JSON `Number` cannot represent uint256.
- `chain_id` as `u64`.

**Schema-golden discipline:** `executor-core/tests/schema_snapshots.rs:140-202` is the existing walker for `JournalActionOutcome`. Phase 4 either updates the existing `Action.json` golden (if exists — currently the `Action` placeholder produces no golden because the only variant is the unit `Noop`) OR adds new tests `action_schema_stable` and `action_includes_phase4_kinds` mirroring lines 141-145 and 154-202 verbatim. The walker that collects from BOTH `enum[]` and `const` shapes (lines 160-182) is mandatory — schemars 1.x emits each non-unit variant as `oneOf:[{const:"contract_call", properties:{...}}, ...]`.

### `crates/executor-mcp/src/validation.rs` (modified — extend output validator allowlist)

**Analog:** self — `validation.rs` is currently the input-validation file. The output validator lives in `tools.rs:417-436` `validate_strategy_output`.

**Phase-3 rejection pattern** (`tools.rs:420-429`):

```rust
serde_json::Value::Array(items) => {
    let actions: Vec<Action> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            serde_json::from_value::<Action>(item.clone())
                .map_err(|e| format!("invalid action at index {i}: {e}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(StrategyOutcome::Actions { actions })
}
```

**Phase-4 widening:** the validator does NOT need code changes — it already calls `serde_json::from_value::<Action>(...)`, and once `Action` gains `ContractCall`/etc. variants in `executor-core/src/schema/action.rs`, the deserialization automatically accepts them. The Phase-3 D-08a regression test `strategy_run_rejects_phase4_action_kind` at `stdio_handshake.rs:1276-1291` MUST be **deleted** in Phase 4 (or repurposed to assert that `contract_call` IS now accepted with the proper field shape, and a NEW test `strategy_run_rejects_unknown_action_kind` covers `kind: "totally_made_up"` with -32018).

**Per-action input validation** (Phase 4 ADDS this — no Phase-3 analog):
- Address format check (lower-case 0x-prefixed 40-hex) — copy the regex shape from `validation.rs:70-84` `validate_strategy_id_format` (which uses `id.len() != 64 && all chars hex`); for addresses use `len != 42 && stripped 40-hex`.
- Decimal-string `value` / `amount` check — accept `^[0-9]+$` only.
- `chain_id` allowlist — defer to Phase-5 policy; Phase 4 just shape-validates.

These per-action validators live in a NEW free function `validate_action(&Action) -> Result<(), String>` in `executor-mcp/src/validation.rs`, called from `tools.rs::validate_strategy_output` AFTER the `serde_json::from_value::<Action>` step. Convention: same `Result<(), String>` signature as `validate_register` (validation.rs:14) so call sites stay uniform.

### `crates/executor-mcp/src/tools.rs` (modified — `strategy_run` handler grows host wiring)

**Analog:** self — `tools.rs:232-351` 8-step lifecycle.

**Pattern to preserve (do NOT rewrite):**
- STEP 5's `tokio::task::spawn_blocking(move || { let mut runtime_ctx = RuntimeContext::new(...); Sandbox::execute(&source, &mut runtime_ctx); runtime_ctx.flush(); ... })` (tools.rs:271-291) is the SINGLE place the JS sandbox runs. Phase 4 adds the alloy `Arc<Provider>` to the closure capture list and threads it into `RuntimeContext::new(...)` as a 6th constructor argument.
- STEP 6 `validate_strategy_output` and `record_action` paths (tools.rs:296-326) carry over verbatim — `Action` deserialization handles new variants automatically.
- STEP 7 `transition(Running -> Succeeded)` (tools.rs:329) unchanged.

**`record_action` payload-serialization pattern** (`tools.rs:471-498`) — MR-03 fix is critical for Phase 4: when `Action::ContractCall { calldata: SerializableHexString, .. }` etc. land, serialization can fail in ways the Phase-3 `Action::Noop` could not. Keep the `?`-propagation:

```rust
let payload = serde_json::to_string(actions).map_err(|e| {
    map_state_error(StateError::SerializationError(format!(
        "journal_actions.payload (Vec<Action>): {e}"
    )))
})?;
```

DO NOT regress to `unwrap_or_else(|_| "[]".into())` — that's the MR-03 anti-pattern.

**No new MCP tool surface.** Phase 4 does NOT add a `strategy_simulate` or `evm_call` tool — REQUIREMENTS.md CTX-01..09 all describe `ctx.*` (in-strategy) capabilities. The MCP layer's only change is the broader `Action` schema in tool responses.

### `crates/executor-mcp/src/config.rs` (modified — add `[evm]` section)

**Analog:** self — `config.rs:16-23` `Config` struct + `config.rs:44-61` `StateConfig` block.

**Phase-4 additions (mirror `StateConfig` line-for-line):**

```rust
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub state: StateConfig,
    #[serde(default)]
    pub evm: EvmConfig,   // Phase 4 — NEW
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvmConfig {
    #[serde(default = "default_rpc_url")]
    pub rpc_url: String,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
}

fn default_rpc_url() -> String {
    "http://127.0.0.1:8545".into()    // local anvil default
}
fn default_request_timeout_ms() -> u64 {
    5_000   // 5s — generous over D-03's 2s wall-clock since RPC is async-bridged
}

impl Default for EvmConfig {
    fn default() -> Self {
        Self {
            rpc_url: default_rpc_url(),
            request_timeout_ms: default_request_timeout_ms(),
        }
    }
}
```

**Test coverage to add** (mirror `config.rs:111-180` test block):
- `evm_section_defaults_to_localhost_anvil`
- `parses_evm_section`
- `absent_evm_section_yields_default`
- `rejects_unknown_evm_fields`
- Update existing `rejects_unknown_top_level_fields` (line 142-147) — it currently uses `[policy]` as the canary; keep `[policy]` (Phase 5 will replace it).

**Wiring point:** `executor-mcp/src/server.rs:44-55` `ExecutorServer::new(state_cfg)` becomes `ExecutorServer::new(state_cfg, evm_cfg)`. Constructor stays `anyhow::Result<Self>`. Provider initialization (`EvmProvider::new(evm_cfg).map_err(|e| anyhow::anyhow!(...))?`) follows the exact pattern of `StateStore::open(...)` at server.rs:45-46.

### `crates/executor-state/src/journal.rs` (modified — extend STJ-03 source-read kinds)

**Analog:** self — `journal.rs:80-95` `record_source_read` is already wired with the `kind: &str` and `target: &str` parameters. Phase-3 emits `kind="strategy_source"`; Phase 4 emits new kinds without schema change:

| Action | `kind` | `target` | `payload_json` |
|--------|--------|----------|----------------|
| `ctx.evm.readContract` | `"evm_call"` | `"{address}#{selector}"` | `{ "chain_id":, "args":[...] }` |
| `ctx.evm.erc20Balance` | `"erc20_balance"` | `"{token}@{owner}"` | `{ "chain_id": }` |
| `ctx.evm.erc20Allowance` | `"erc20_allowance"` | `"{token}@{owner}->{spender}"` | `{ "chain_id": }` |
| `ctx.evm.nativeBalance` | `"native_balance"` | `"{owner}"` | `{ "chain_id": }` |

**Schema is already sufficient** — `journal_source_reads` at `executor-state/src/schema.rs:36-44` declares `kind TEXT NOT NULL` and `payload_json TEXT NULL`. NO schema migration needed (Phase-2 D-03b idempotent CREATE TABLE precedent — no `schema_version` table; new `kind` strings are pure data, not DDL).

**`journal_actions` payload extension:** the JSON shape inside `payload_json` for `outcome="actions"` becomes the canonical `Vec<Action>` serialized by serde — already wired by `tools.rs:480-485`. No repository change needed.

**Anti-pattern carry-forward (MR-04 seq column):** `journal_actions` is one row per run from a single handler invocation, so no `seq` column is needed there (per 03-REVIEW-FIX MR-04 rationale). DO NOT add a `seq` column to `journal_source_reads` either — Phase-3 emits one row per run, Phase 4 emits 1..N rows per run but they are inserted from the same `RuntimeContext::flush()` `MutexGuard` so insertion order is preserved by `recorded_at ASC, id ASC` ordering at `journal.rs:170-174`. **Exception:** if Phase 4 chooses to record EVM reads as they happen during JS execution (not buffered), then a `seq` column IS needed — same logic that drove `journal_logs` MR-04. RECOMMENDATION: **buffer EVM reads inside `RuntimeContext` and flush them in `flush()`** to avoid this complexity.

### `crates/executor-mcp/tests/stdio_handshake.rs` (extended) + `crates/executor-mcp/tests/anvil_evm.rs` (NEW)

**Analog:** self — `stdio_handshake.rs:1118-1147` `strategy_run_returns_noop_for_minimal_strategy`.

**`seed_strategy` helper carry-over** (stdio_handshake.rs:1107-1116):

```rust
fn seed_strategy(db_path: &std::path::Path, name: &str, source: &str) -> Result<String> {
    let mut store = executor_state::StateStore::open(db_path)?;
    let outcome = store.register_strategy(name, source, None, None)?;
    let id = match outcome {
        executor_state::RegisterOutcome::Created(s)
        | executor_state::RegisterOutcome::AlreadyExists(s) => s.id,
    };
    Ok(id)
}
```

**Per-test scaffold** (lines 1119-1147):

```rust
#[tokio::test]
async fn <test_name>() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db_path = dir.path().join("state.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    let strategy_id = seed_strategy(&db_path, "<name>", "<JS source>")?;

    let mut proc = spawn_server_with_state(&db_path_str).await?;
    let _ = initialize(&mut proc).await?;
    let r = call_tool(&mut proc, 2, "strategy_run", json!({ "strategy_id": strategy_id })).await?;
    let body = extract_json_result(&r);
    // assertions ...
    proc.child.kill().await?;
    Ok(())
}
```

**Anvil subprocess fixture (NEW file `crates/executor-mcp/tests/anvil_evm.rs`):**

Recommend a separate test file (NOT inside `stdio_handshake.rs`) so the existing 43-test stdio file stays focused and tests that need anvil can be filtered with `cargo test --test anvil_evm`.

**Anvil binary discovery + skip pattern** (no in-tree analog — extrapolate from `executor-state/Cargo.toml:16` `rusqlite = { features = ["bundled"] }` which avoided the system-dep problem entirely):

```rust
fn anvil_available() -> bool {
    std::process::Command::new("anvil")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn <evm_test>() -> Result<()> {
    if !anvil_available() {
        eprintln!("skipping: anvil not on PATH (install foundry)");
        return Ok(());
    }
    // spawn anvil bound to an ephemeral port, capture it, spawn server, run tests, kill anvil
}
```

**Convention rationale:** **skip with stderr message, do not fail.** CI may or may not have foundry installed; failing locks the test to one runner config. The `eprintln!` is acceptable here because integration tests run as separate processes — the workspace `clippy::print_stderr = "deny"` lint at `Cargo.toml:30` applies to crate `src/` only (workspace lints don't propagate to `tests/` files unless explicitly applied); verify by running `cargo clippy --tests --workspace` after adding the first skip.

**Port management:** anvil supports `--port 0` to bind an ephemeral port; parse the port from stdout's first `Listening on` line. Pattern mirrors `executor-mcp/tests/common/mod.rs:96-104` which drains stderr in a background tokio::spawn — copy that pattern verbatim, but capture-and-parse one line of stdout before draining.

**Test naming convention** (Phase-3 D-08a verbatim style: `strategy_run_<scenario>_<expected>` snake_case from CONTEXT.md D-08a table):
- `evm_read_contract_returns_decoded_value_against_anvil`
- `evm_erc20_balance_returns_zero_for_fresh_account`
- `evm_native_balance_returns_anvil_default_funded_account`
- `actions_contract_call_records_action_journal_row`
- `actions_native_transfer_appears_in_strategy_run_response`
- `evm_read_records_source_read_journal_row` (parallels Phase-3 `strategy_run_records_source_read_journal_row`)

---

## Workspace Dependency Conventions

**Per-crate dep posture (mandatory for Phase 4):**

| Phase | Heavy dep | Promotion to `[workspace.dependencies]`? | Source comment |
|-------|-----------|------------------------------------------|----------------|
| 1 | `tracing-subscriber`, `rmcp` | Yes (workspace) | `Cargo.toml:11-21` |
| 2 | `rusqlite` | **NO** — per-crate | `crates/executor-state/Cargo.toml:10-13` |
| 3 | `rquickjs` | **NO** — per-crate | `crates/strategy-js/Cargo.toml:10-14` |
| 4 | `alloy` | **NO** — per-crate (`crates/executor-evm/Cargo.toml`) | new comment block; cite the precedent line refs |

The promotion threshold is "≥ 2 crates consume the dep". Phase 4 has only `executor-evm` consuming alloy initially; even when `strategy-js` takes a path-dep on `executor-evm`, alloy itself is re-exported through `executor-evm`'s public surface — `strategy-js` does NOT add `alloy` to its own Cargo.toml. This keeps the dep tree shallow and the workspace `[workspace.dependencies]` block honest.

**Default-features = false convention:** apply to alloy exactly as Phase 3 applied it to rquickjs at `crates/strategy-js/Cargo.toml:21`. Phase 4 planner explicitly enumerates the alloy feature subset needed (likely `["provider-http", "json-abi", "contract"]`-equivalent — pin during 04-01 research).

---

## Async-to-Sync Bridge Pattern (alloy inside spawn_blocking)

**Constraint stack (carries forward from Phase 3):**

1. `tools.rs::strategy_run` runs the sandbox inside `tokio::task::spawn_blocking { ... }` at lines 276-291.
2. Inside that closure, we are NOT on an async runtime — we are on a blocking thread.
3. `Sandbox::execute` calls `Function::new(c, move |...| -> rquickjs::Result<...>)` closures synchronously (rquickjs `Runtime` is `!Sync` without the `parallel` feature).
4. alloy's `Provider::call(...).await` is async.

**Pattern to use** (no in-tree analog — this is the bridge Phase 4 introduces):

```rust
// Inside the JS host function closure (sandbox.rs ctx.evm.readContract install):
let provider = provider.clone();      // Arc<Provider> captured from RuntimeContext
let req: EvmCallRequest = /* parse args */;

// Re-enter the async runtime that owned the spawn_blocking task:
let bytes = tokio::runtime::Handle::current()
    .block_on(provider.call(req))
    .map_err(|e| rquickjs::Error::new_from_js("evm", "transport"))?;
```

**Why `Handle::current().block_on(...)` is safe here:**
- `tokio::task::spawn_blocking` runs on a dedicated blocking-thread pool. Calling `Handle::current()` returns the same runtime handle the parent task lives on.
- `block_on` from inside `spawn_blocking` is documented-safe (NOT inside an async task — that would deadlock; we are NOT in an async task because `spawn_blocking` already moved us off the runtime threads).
- This is the EXACT inverse of `state.blocking_lock()` at `tools.rs:280` — Phase 3 used `blocking_lock` because it was inside `spawn_blocking`; Phase 4 uses `block_on` for the same reason.

**Anti-pattern (do NOT use):** wrapping the alloy call in `futures::executor::block_on(...)` — that creates a NEW executor that doesn't share the existing reactor, so HTTP IO will hang. Always use `tokio::runtime::Handle::current().block_on(...)`.

**Provider construction location:** `EvmProvider::new(evm_cfg)` runs in `ExecutorServer::new` (server.rs:44-55) — that IS on the async runtime, so the constructor can be `pub async fn new(...)` OR can use `Handle::current().block_on(...)` internally. Recommend **async constructor**: `pub async fn new(cfg: &EvmConfig) -> Result<Self, EvmError>`, called from `ExecutorServer::new(state_cfg, evm_cfg)` which itself becomes `pub async fn new(...)` in `server.rs`. Then `main.rs:1-30` already runs inside `#[tokio::main]` so `.await`-ing in `ExecutorServer::new` is straightforward.

---

## Schema-Golden Discipline (walker fix carry-forward)

**Mandatory walker** — copy verbatim from `crates/executor-core/tests/schema_snapshots.rs:154-202`:

```rust
fn walk(v: &serde_json::Value, found: &mut BTreeSet<String>) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(arr)) = map.get("enum") {
                for item in arr {
                    if let Some(s) = item.as_str() { found.insert(s.to_string()); }
                }
            }
            if let Some(serde_json::Value::String(s)) = map.get("const") {
                found.insert(s.clone());
            }
            for (_k, child) in map { walk(child, found); }
        }
        serde_json::Value::Array(arr) => { for child in arr { walk(child, found); } }
        _ => {}
    }
}
```

**Why:** schemars 1.x emits non-unit enum variants as `oneOf:[{const:"contract_call", properties:{...}}, {const:"raw_call", ...}, ...]`. A naive walker that only collects `enum[]` would miss every Phase-4 variant. The walker shipped in 03-03 already handles both shapes — Phase 4 reuses it for the Action golden.

**Phase-4 golden tests to add** (mirror `journal_action_outcome_includes_future_variants` at line 154):

```rust
#[test]
fn action_schema_stable() {
    assert_schema_matches_golden("Action", schema_for!(Action));
}

#[test]
fn action_includes_phase4_kinds() {
    let raw = std::fs::read_to_string("tests/schemas/Action.json").unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut found: BTreeSet<String> = BTreeSet::new();
    fn walk(...) { /* verbatim from schema_snapshots.rs:160-184 */ }
    walk(&v, &mut found);
    let expected: BTreeSet<String> = [
        "noop", "contract_call", "raw_call",
        "erc20_approve", "erc20_transfer", "native_transfer",
    ].iter().map(|s| s.to_string()).collect();
    assert_eq!(found, expected);
}
```

**`UPDATE_SCHEMAS=1` workflow** (lines 35-38 of schema_snapshots.rs): Phase 4 planner runs `UPDATE_SCHEMAS=1 cargo test -p executor-core --test schema_snapshots` to materialize the new `Action.json` after `executor-core/src/schema/action.rs` lands. The golden file becomes a checked-in contract artifact; subsequent edits without `UPDATE_SCHEMAS=1` fail loudly.

---

## Test Harness Conventions

**Three test tiers Phase 4 inherits:**

1. **In-crate unit tests** — `#[cfg(test)] mod tests { ... }` at the bottom of each `.rs` file. Convention from `crates/strategy-js/src/error.rs:47-90`, `crates/executor-mcp/src/validation.rs:87-185`. Phase 4 adds:
   - `executor-evm/src/error.rs` — round-trip for each `EvmError` variant's `Display` text.
   - `executor-evm/src/abi.rs` — encode/decode of a known function signature against a known calldata hex string (no network — pure offline ABI test).
   - `executor-mcp/src/validation.rs` — `validate_action` per-variant tests (address format, decimal-string amount).
   - `executor-mcp/src/config.rs` — `[evm]` section parsing + default tests (4 tests, mirror lines 117-138).

2. **In-crate integration tests** — `crates/<crate>/tests/<feature>.rs`. Convention from `crates/strategy-js/tests/{ctx_host_api,sandbox_host_globals,sandbox_entry_shape,sandbox_limits,runtime_journal_flush}.rs`. Phase 4 adds:
   - `crates/strategy-js/tests/ctx_evm_api.rs` — D-04 carry-over for `ctx.evm.*` shape (mirror `tests/ctx_host_api.rs:114-153` `Object.keys` shape assertions).
   - `crates/strategy-js/tests/ctx_actions_api.rs` — `ctx.actions.*` builder shape + return-value tests (mirror `tests/ctx_host_api.rs:107-110` `ctx.actions.noop` test pattern).
   - `crates/executor-evm/tests/abi_roundtrip.rs` — offline ABI tests (no network).
   - `crates/executor-state/tests/journal_evm_kinds.rs` — assert STJ-03 carries Phase-4 kinds (`evm_call`, `erc20_balance`, etc.) round-trip without schema change.

3. **MCP stdio integration** — `crates/executor-mcp/tests/stdio_handshake.rs` (offline) + `crates/executor-mcp/tests/anvil_evm.rs` (anvil-required). Convention from `stdio_handshake.rs:1118-1147` + the seed-strategy helper at lines 1107-1116. Phase 4 adds:
   - Offline tests in `stdio_handshake.rs` for each new Action variant's validation (mirror `strategy_run_rejects_phase4_action_kind` line 1276 — but flipped: now `contract_call` is ACCEPTED, and `unknown_kind` is rejected).
   - Online tests in `anvil_evm.rs` (skip-if-anvil-missing pattern above) for read-path correctness.

**Anvil binary discovery — recommended pattern** (no exact in-tree analog; closest is `Cargo.toml:11-21` `rmcp` workspace dep where the dep ships its own bundled binary, NOT applicable to anvil):

```rust
fn anvil_available() -> bool {
    std::process::Command::new("anvil")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
```

**Skip-vs-fail decision: SKIP** with one-line `eprintln!` to stderr. Rationale: foundry is a separate-install dependency; CI may or may not have it. A test failing on absent foundry is noise. Document in `crates/executor-evm/README.md` (or a new `crates/executor-evm/TESTING.md`) that `foundry-rs/foundry` is required for the `anvil_evm` test suite, with the install command (`curl -L https://foundry.paradigm.xyz | bash && foundryup`).

---

## Action Validator Extension Pattern

**Current rejection list at `tools.rs::validate_strategy_output` (lines 417-436):** rejects everything that isn't deserializable into the `Action` enum. Phase 3 only had `Action::Noop`, so any object with `kind` other than `"noop"` failed.

**Phase-4 widening — automatic via serde:** once `Action::ContractCall { .. }` etc. are added to the enum, `serde_json::from_value::<Action>` accepts them without validator code change. The Phase-3 D-08a regression test `strategy_run_rejects_phase4_action_kind` at `stdio_handshake.rs:1276-1291` becomes the inverse:

```rust
#[tokio::test]
async fn strategy_run_accepts_contract_call_action() -> Result<()> {
    let strategy_id = seed_strategy(
        &db_path,
        "p4_cc",
        r#"(ctx) => [{kind:"contract_call", chain_id: 1, address: "0x...", selector: "0xa9059cbb", calldata: "0x...", value: "0"}]"#,
    )?;
    // ... assert outcome.kind == "actions" and actions[0].kind == "contract_call"
}

#[tokio::test]
async fn strategy_run_rejects_unknown_action_kind() -> Result<()> {
    let strategy_id = seed_strategy(&db_path, "unk", r#"(ctx) => [{kind:"totally_made_up"}]"#)?;
    // ... assert error code -32018, data.code "strategy_invalid_output"
}
```

**Per-action shape validation** (NEW `validate_action` in `executor-mcp/src/validation.rs`):

Sequence inside `tools.rs::validate_strategy_output`:
1. `serde_json::from_value::<Action>(item.clone())` — enforces enum-variant + field-presence (existing).
2. `crate::validation::validate_action(&action)` — enforces address format / decimal-string / chain_id range (NEW).
3. On either failure: `format!("invalid action at index {i}: {detail}")` → wrapped into `STRATEGY_INVALID_OUTPUT` (-32018) at `tools.rs:431`.

**Convention:** `validate_action` returns `Result<(), String>` — same shape as `validate_register` at `validation.rs:14`. Per-variant pattern-match inside the function so the `Action` enum stays the source of truth for what fields exist.

---

## Anti-patterns to Avoid

Carried forward from Phase-3 `03-REVIEW-FIX.md`. Each is a class of bug Phase 4 will hit a new instance of unless the planner pre-instructs against it:

### MR-01: Never echo raw library text in `data.detail`

**What Phase 3 did wrong:** raw `rusqlite::Error::to_string()` (constraint names, table names, SQLite-internal phrasing) leaked onto the wire via `data.detail = msg`.

**Phase 4 risk:** alloy errors carry RPC-server text (e.g. `"execution reverted: ERC20: insufficient balance"`, gas-estimation internals, node-vendor-specific phrasing). Leaking that text breaks agent dispatch (every node-vendor's wording is different).

**Mandatory pattern** (mirror `errors.rs:180-193`):

```rust
EvmError::Transport(raw) => {
    tracing::warn!(detail = %raw, "evm transport error");
    McpError::new(STRATEGY_RUNTIME_ERROR, "evm transport error".to_string(),
        Some(json!({ "code": "strategy_runtime_error", "kind": "exception", "detail": "evm transport error", "run_id": run_id })))
}
EvmError::Reverted(raw) => {
    tracing::warn!(detail = %raw, "evm contract reverted");
    McpError::new(STRATEGY_RUNTIME_ERROR, "evm contract reverted".to_string(),
        Some(json!({ "code": "strategy_runtime_error", "kind": "exception", "detail": "evm contract reverted", "run_id": run_id })))
}
```

Stable taxonomy strings on the wire; raw text to `tracing::warn!` for operator forensics.

### MR-03: Never silently fall back to default values on serde failure

**What Phase 3 did wrong:** `serde_json::to_string(actions).unwrap_or_else(|_| "[]".into())` — a swallowed serde failure was indistinguishable from a legitimate empty `Action[]`.

**Phase 4 risk:** new `Action::ContractCall { calldata: ... }` variants serialize fallibly (calldata may be invalid hex, value may overflow). The Phase-4 fix is **already in place** at `tools.rs:480-485` — DO NOT regress it. If new fallible serialization sites are added (e.g. journal_source_reads payload for `evm_call`), use the same `?`-propagation:

```rust
serde_json::to_string(&payload).map_err(|e| {
    map_state_error(StateError::SerializationError(format!(
        "journal_source_reads.payload (EvmCallRequest): {e}"
    )))
})?
```

### MR-04: Same-millisecond ordering needs a `seq` column or monotonic id

**What Phase 3 did wrong:** `ULID::new()` is not monotonic within a millisecond; `journal_logs` rows from the same `ctx.log` burst landed in non-deterministic order.

**Phase 4 risk:** if `ctx.evm.readContract` records a journal row PER CALL and a strategy makes multiple reads in tight succession, the same flaw recurs in `journal_source_reads`. **Mitigation = buffer-and-flush:** record EVM reads in `RuntimeContext.evm_read_buffer: Vec<EvmReadEntry>` during execution, drain in `flush()` from the same `MutexGuard`. Single-writer guarantee preserves insertion order via `recorded_at ASC, id ASC` ordering at `journal.rs:170-174`.

If buffer-and-flush is rejected and Phase 4 records during JS execution: ADD a per-run `seq` column to `journal_source_reads` mirroring `journal_logs` schema at `executor-state/src/schema.rs:64-71` exactly (including the `UNIQUE (run_id, seq)` constraint at line 70).

### HR-01: D-11 globals scrub MUST run BEFORE host bindings install

**What Phase 3 fixed:** the scrub at `sandbox.rs:225-229` was reordered to run BEFORE `c.globals().set("__ctx", ctx_obj)` at lines 232-234. A future intrinsic that surfaces a name overlapping a host binding would otherwise silently delete the host binding.

**Phase 4 risk:** when `ctx.evm` is added, IF a future rquickjs intrinsic surfaces a name like `evm` or `eth`, the scrub list (sandbox.rs:308-321) might need to either delete that name OR — if Phase 4 wants to keep `ctx.evm` reachable — stay AS-IS but with `ctx.evm` installed AFTER the scrub. **The scrub-before-bindings invariant is non-negotiable.** Phase 4 planner MUST cite `sandbox.rs:212-234` (the comment block at lines 212-224 documents the rationale) as a constraint on Plan 04-02's ordering.

### Promise-return rejection (D-10 carry-over)

**Phase-3 invariant** at `sandbox.rs:254-260`: any `rquickjs::Value::is_promise()` return triggers `RuntimeError::InvalidOutput { detail: "promise return values are not supported in v1" }`.

**Phase 4 risk:** alloy is async. If `ctx.evm.readContract` is wired to return a Promise that the strategy must `await`, Phase 4 effectively reintroduces async strategies — explicitly out of scope per 03-CONTEXT.md D-10 ("Async strategies / Promise-returning strategies — explicitly rejected for v1"). **Mandatory:** `ctx.evm.*` host functions are SYNCHRONOUS from the strategy's perspective — they return decoded values, not Promises. The async work happens via the `Handle::current().block_on(...)` bridge inside the host function. Strategies remain `(ctx) => Action[]` — no `async (ctx) => ...`.

### Error-code uniqueness audit carry-over

**Phase-3 acceptance** at 03-03-SUMMARY.md line 189: ran `grep -r 'ErrorCode(-3201[178])' ~/.cargo/registry/src/index.crates.io-*/rmcp-1.5.0/` — confirmed empty.

**Phase 4 may not need new MCP codes** (no new tools — the existing -32011/-32014/-32016/-32017/-32018/-32602 cover every failure path). If the planner concludes that an EVM-specific code is warranted (e.g. `-32019 EVM_RPC_UNREACHABLE` distinct from generic `-32017`), the same audit pattern is mandatory before reservation — do NOT skip it.

---

## Logging / Tracing Convention

Phase 3 used `tracing::warn!(detail = %msg, "<context>")` for every error swallowed at the wire boundary. Examples at `errors.rs:170, 187`. Phase 4 inherits — every `EvmError` mapping path uses `tracing::warn!(detail = %raw, "<context>")` before constructing the wire-stable `data.detail`. No new logging crates; `tracing` already in `executor-mcp/Cargo.toml:20` and `strategy-js/Cargo.toml:31` and will be added to `executor-evm/Cargo.toml`.

The workspace lints at `Cargo.toml:28-31` (`print_stdout = "deny"`, `print_stderr = "deny"`, `dbg_macro = "deny"`) propagate to `executor-evm` automatically via `[lints] workspace = true` (mirror `strategy-js/Cargo.toml:7-8`). Test files (`crates/*/tests/*.rs`) inherit the same lints — confirmed by Phase 3's clean clippy run on the 19 D-08a stdio tests.

---

## Commit / Branch / Tracking Conventions

Mirroring the Phase-3 commit log (`git log --oneline` excerpts from this conversation's gitStatus):

| Phase 3 example | Phase 4 form |
|-----------------|--------------|
| `feat(03-03): add STRATEGY_DELETED/RUNTIME_ERROR/INVALID_OUTPUT codes…` | `feat(04-NN): <subject>` |
| `feat(03-03): wire strategy_run MCP tool + journal://{run_id} resource…` | `feat(04-NN): <subject>` |
| `test(03-03): add 19 D-08a stdio integration tests for strategy_run…` | `test(04-NN): <subject>` |
| `docs(03): code review + verification reports…` | `docs(04): <subject>` |
| `fix(03): MR-01 stop echoing raw rusqlite text in storage_error data.detail` | `fix(04): <ID> <subject>` |

**Conventional-commits scope `(04-NN)` for plan-level commits, `(04)` for phase-level docs/fix.** No mention of "claude" in commit messages (user instruction in `~/.claude/CLAUDE.md`).

**Branch:** continue on `main` per repo policy (no feature branches in Phase 3 either — all commits land directly on `main` in 03-03-SUMMARY.md line 177-185).

**Plan-level acceptance gates** (mirror 03-03-SUMMARY.md line 81-91):
- [ ] `cargo build --workspace` clean
- [ ] `cargo test --workspace` passes — target around 200+ tests by phase end (Phase 3 closed at 175)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] All new D-08a-equivalent stdio tests use verbatim names from CONTEXT.md
- [ ] Schema goldens for `Action.json` (and any new request/response types) committed
- [ ] CTX-01..09 marked complete in REQUIREMENTS.md
- [ ] No mention of "claude" in any commit message
- [ ] No raw alloy/RPC error text in any `data.detail` (MR-01 audit)

---

## No Analog Found

Files where the closest in-tree match is too thin to mirror — planner consults RESEARCH.md (not yet present for Phase 4) and AGENTS.md instead:

| File | Role | Reason no analog |
|------|------|------------------|
| `crates/executor-evm/src/provider.rs` | service | `executor-signer` is the closest crate-shape sibling but currently a stub (`crates/executor-signer/src/lib.rs:1-21`). The provider pattern (`Arc<Provider>` + async constructor + sync method facade via `Handle::current().block_on`) is genuinely new in Phase 4. RECOMMEND: planner anchors on alloy's own crate examples + the `Arc<Mutex<StateStore>>` sharing pattern at `executor-mcp/src/server.rs:38-54` for the lifecycle (one Arc per `ExecutorServer`, cloned into closure captures). |
| `crates/executor-evm/src/abi.rs` | utility | No ABI-encoding code anywhere in tree today. Pure-alloy concern; planner cites alloy docs. The wrapping convention to follow is the same as `executor-state/src/strategies.rs` — free functions that take borrowed args + a `Result<T, EvmError>` return; expose the public surface via `lib.rs` re-export per `crates/strategy-js/src/lib.rs:27-29` precedent. |

---

## Metadata

- **Analog search scope:** `crates/strategy-js/`, `crates/executor-mcp/`, `crates/executor-state/`, `crates/executor-core/`, `crates/executor-signer/`, `Cargo.toml`, `.planning/phases/03-javascript-strategy-runner/`, `.planning/REQUIREMENTS.md`, `AGENTS.md`.
- **Files scanned:** ~30 source/test/planning files.
- **Pattern extraction date:** 2026-04-27.
- **Phase-3 acceptance state inherited:** 175 tests green, clippy clean, all Phase-3 D-08a tests verbatim per CONTEXT.md.
