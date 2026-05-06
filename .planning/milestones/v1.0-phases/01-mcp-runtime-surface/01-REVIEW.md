---
phase: 01-mcp-runtime-surface
reviewed: 2026-04-24T00:00:00Z
depth: standard
files_reviewed: 38
files_reviewed_list:
  - Cargo.toml
  - rust-toolchain.toml
  - .gitignore
  - config.example.toml
  - crates/executor-core/Cargo.toml
  - crates/executor-core/src/error.rs
  - crates/executor-core/src/lib.rs
  - crates/executor-core/src/schema/action.rs
  - crates/executor-core/src/schema/execution.rs
  - crates/executor-core/src/schema/mod.rs
  - crates/executor-core/src/schema/policy.rs
  - crates/executor-core/src/schema/prompt_args.rs
  - crates/executor-core/src/schema/strategy.rs
  - crates/executor-core/tests/schema_snapshots.rs
  - crates/executor-core/tests/schemas/ExecutionIdInput.json
  - crates/executor-core/tests/schemas/PolicyUpdateInput.json
  - crates/executor-core/tests/schemas/ReviewEvmStrategyArgs.json
  - crates/executor-core/tests/schemas/StrategyIdInput.json
  - crates/executor-core/tests/schemas/StrategyRegisterInput.json
  - crates/executor-core/tests/schemas/StrategyRunOnceInput.json
  - crates/executor-core/tests/schemas/WriteEvmStrategyArgs.json
  - crates/executor-mcp/Cargo.toml
  - crates/executor-mcp/src/config.rs
  - crates/executor-mcp/src/errors.rs
  - crates/executor-mcp/src/lib.rs
  - crates/executor-mcp/src/logging.rs
  - crates/executor-mcp/src/main.rs
  - crates/executor-mcp/src/prompts.rs
  - crates/executor-mcp/src/resources.rs
  - crates/executor-mcp/src/server.rs
  - crates/executor-mcp/src/tools.rs
  - crates/executor-mcp/tests/common/mod.rs
  - crates/executor-mcp/tests/stdio_handshake.rs
  - crates/executor-signer/Cargo.toml
  - crates/executor-signer/src/lib.rs
  - crates/executor-state/Cargo.toml
  - crates/executor-state/src/lib.rs
findings:
  critical: 0
  warning: 2
  info: 4
  total: 6
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-04-24
**Depth:** standard
**Files Reviewed:** 38
**Status:** issues_found

## Summary

Phase 01 delivers a clean, well-scoped MCP runtime scaffold. Stdout discipline (D-05)
is enforced with layered defences: workspace-wide clippy denylist for
`print_stdout`/`print_stderr`/`dbg_macro`, per-crate `#![deny(...)]` at every `lib.rs`
and `main.rs`, and the load-bearing `fmt::layer().with_writer(std::io::stderr)`
subscriber wiring. The integration test suite actively asserts JSON-RPC purity on
every stdout line (`common::recv`) and exercises every MCP method the server
exposes, which is a strong correctness backstop.

rmcp 1.5 handler wiring looks correct per the researched API (Pitfall 6 resolved:
`#[tool_handler]` and `#[prompt_handler]` share one `impl ServerHandler` block;
`vis = "pub(crate)"` on both `#[tool_router]` and `#[prompt_router]` correctly
exposes the macro-generated associated fns across module boundaries). Schema
goldens round-trip and the `unimplemented` error envelope is uniform and
machine-readable.

Findings are all non-blocking: two warnings around ergonomics/robustness of the
config loader and logging init, and a handful of info-level code-smell/dead-code
observations. No security issues, no stdout-discipline violations, no rmcp
signature mismatches.

## Warnings

### WR-01: `logging::init` treats its `Result` as a success-only return

**File:** `crates/executor-mcp/src/logging.rs:14-22`
**Issue:** The signature is `pub fn init(cfg: &crate::config::Config) -> Result<()>`
but the body has no fallible operation — `tracing_subscriber::registry()... .init()`
panics on double-init rather than returning an error, and `EnvFilter::new` is
lossy (silently drops invalid directives) rather than fallible. The `Result<()>`
return is therefore misleading: callers (and future maintainers) may assume
init failure is communicated through the `Err` branch when it never will be.

A second consequence: if a Phase 2+ operator writes `logging.level = "garbage"`
in `config.toml`, `EnvFilter::new("garbage")` produces an empty filter (effectively
silencing tracing) with no error surface. The config parser accepts any string
because `LoggingConfig::level` is `String` with no validation.

**Fix:** Either tighten the config validation or surface the parse error:

```rust
use tracing_subscriber::filter::EnvFilter;

pub fn init(cfg: &crate::config::Config) -> Result<()> {
    let env_filter = match EnvFilter::try_from_default_env() {
        Ok(f) => f,
        Err(_) => EnvFilter::try_new(&cfg.logging.level).with_context(|| {
            format!("invalid logging.level {:?}", cfg.logging.level)
        })?,
    };
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init()
        .context("tracing subscriber already initialised")?;
    Ok(())
}
```

`try_new` and `try_init` are both fallible variants that align with the stated
posture in `errors.rs`: "the error surface stays structured (never a raw panic
that could bleed into stdout — cf. D-05)."

### WR-02: `_SignedTransactionAlias` is a fragile workaround for an unused-import warning

**File:** `crates/executor-signer/src/lib.rs:4-17`
**Issue:** The module imports `executor_core::schema::execution::SignedTransaction`,
then re-exports it as `pub type _SignedTransactionAlias = SignedTransaction;`
with `#[doc(hidden)]`, solely to suppress a `dead_code`/`unused_imports`
warning while keeping the dependency edge for Phase 6. The leading underscore
is a C-style hack that signals "please ignore me" but is still part of the
crate's public API — any downstream crate could import `_SignedTransactionAlias`
by that name and create a bespoke coupling that Phase 6 cannot easily break.

**Fix:** Either (a) drop the import until Phase 6 actually needs it, since
the `Signer` trait body is empty anyway; (b) re-export cleanly with intent:

```rust
//! Signer boundary — Phase 6에서 local signer 구현.

// Re-exported so downstream crates consume SignedTransaction through the
// signer boundary rather than reaching into executor_core directly. Remove
// if Phase 6 decides SignedTransaction belongs elsewhere.
pub use executor_core::schema::execution::SignedTransaction;

pub trait Signer: Send + Sync {
    // Phase 6: add sign methods.
}
```

Option (b) keeps the dependency edge visible and intentional; the current
`_`-prefixed alias hides both the intent and the breakage risk.

## Info

### IN-01: CLI arg parser does not support `--config=PATH` form

**File:** `crates/executor-mcp/src/config.rs:46-57`
**Issue:** Only the space-separated `--config PATH` form is recognised. The
more common `--config=PATH` form is silently ignored (it neither matches the
`args[i] == "--config"` branch nor errors), so the loader will fall back to
env / default and the operator will have no feedback that the CLI flag was
typo'd. Not a bug for Phase 1, but a common foot-gun when the server is run
via automation (systemd unit files, Docker CMD arrays, etc.).

**Fix:** Either document the supported form in `config.example.toml` /
README, or handle both forms:

```rust
if let Some(rest) = args[i].strip_prefix("--config=") {
    path_from_cli = Some(rest.to_string());
    break;
} else if args[i] == "--config" && i + 1 < args.len() {
    path_from_cli = Some(args[i + 1].clone());
    break;
}
```

### IN-02: Redundant shape-sanity loop after explicit `from_value` calls

**File:** `crates/executor-mcp/tests/stdio_handshake.rs:465-467`
**Issue:** The seven `serde_json::from_value::<Type>(cases[N].1.clone())?`
calls above already prove each sample deserialises into its target struct —
which requires the sample to be a JSON object (since every struct is
`#[derive(Deserialize)]` without `#[serde(transparent)]` or a scalar shape).
The subsequent `for (name, sample) in &cases { assert!(sample.is_object(), ...) }`
loop re-checks the same invariant from a weaker angle. Low-impact but
slightly misleading about what the test is actually enforcing.

**Fix:** Drop the loop, or replace it with an assertion that actually adds
coverage (e.g., serialising the struct back and asserting idempotent
round-trip with `serde_json::to_value(...)?`).

### IN-03: `Action::Noop` placeholder will coerce into real variants in Phase 4

**File:** `crates/executor-core/src/schema/action.rs:10-14`
**Issue:** With `#[serde(tag = "kind", rename_all = "snake_case")]` on the
enum, `Noop` serialises to `{"kind":"noop"}`. Any Phase 2+ persisted strategy
payload or external integration test that happens to serialise an `Action`
today will pin `"noop"` into the wire format. Phase 4 adds real variants
(ContractCall, RawCall, Erc20Approve, Erc20Transfer, NativeTransfer) and
will have to decide whether to keep `Noop` as a no-op action or remove it —
either way the transition is easier if `Noop` never escapes to persisted
state.

Not a Phase 1 bug (nothing persists `Action` yet), but worth a note so
Phase 2 doesn't accidentally store `{"kind":"noop"}` blobs that Phase 4
then has to migrate.

**Fix:** Add a Phase-4 migration note to the TODO comment explicitly
calling out "do not persist Action values until Phase 4 decides Noop's
fate," or mark the enum `#[non_exhaustive]` so downstream matches are
forced to handle the variant explosion intentionally:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Action {
    Noop,
    // TODO(phase-4): ContractCall, RawCall, Erc20Approve, Erc20Transfer, NativeTransfer
}
```

### IN-04: `PolicyUpdateInput.metadata` schema omits explicit `null` support

**File:** `crates/executor-core/src/schema/policy.rs:8-12` and
`crates/executor-core/tests/schemas/PolicyUpdateInput.json:6-10`
**Issue:** `metadata: Option<serde_json::Value>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`
produces a golden schema where `metadata` has no `type` field at all, which
is technically valid JSON Schema (permits anything) but contrasts with
`WriteEvmStrategyArgs.chain_hint`, where `Option<String>` renders as
`"type": ["string", "null"]`. The asymmetry is schemars' natural behaviour
(untyped `Value` maps to no type constraint), but agents reading the schema
will not immediately see that `null` is accepted.

Not a bug — both forms are accepted by serde in Phase 1 — but if Phase 5
starts enforcing "metadata must be an object," the schema should be
tightened from `Option<Value>` to `Option<HashMap<String, Value>>` or a
concrete struct so `schema_for!` emits the `"object"` constraint.

**Fix:** No change needed in Phase 1; flag for Phase 5 design review.

---

_Reviewed: 2026-04-24_
_Reviewer: gsd-code-reviewer_
_Depth: standard_
