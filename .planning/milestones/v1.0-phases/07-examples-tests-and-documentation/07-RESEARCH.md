# Phase 7: Examples, Tests, and Documentation - Research

**Researched:** 2026-04-29  
**Domain:** Rust MCP runtime verification, local Anvil EVM examples, safety regression tests, developer docs  
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
## Implementation Decisions

### Examples
- Add local anvil-based examples that demonstrate an ERC20 approve or transfer through the actual runtime flow.
- Add a generic ABI contract call example so v1 is not framed as ERC20-only.
- Prefer runnable repository examples/fixtures over prose-only examples.
- Keep examples local-first and deterministic; no hosted services, dashboards, or protocol-specific recipe catalog.

### Tests
- Add safety regression tests for policy denial, simulation failure before signing, and JavaScript sandbox forbidden host access.
- Reuse existing stdio/anvil test harnesses and fixtures where possible instead of creating a parallel test framework.
- Prioritize tests that prove externally observable runtime guarantees: no signing before policy/simulation approval, no host access from JS, and status/journal evidence is queryable.
- If live local-chain execution requires anvil, gate those tests consistently with existing `anvil-tests` patterns.

### Documentation
- Refresh README/AGENTS usage docs around the final v1 runtime loop: register strategy, run strategy, policy/simulation/signing, query execution and journal.
- Document local hot-wallet assumptions clearly, including env-var private key handling and local signer scope.
- Keep docs developer-facing and concise; avoid dashboard/product marketing language.
- Include enough commands/config examples for an agent or developer to reproduce the local flow.

### Claude's Discretion
- Exact fixture names, test grouping, and README section ordering are at Claude's discretion as long as the phase success criteria and VER-01..VER-05 are covered.

### Deferred Ideas (OUT OF SCOPE)
## Deferred Ideas

- Protocol-specific recipe catalog remains out of scope for v1.
- Dashboard/marketplace docs remain out of scope for v1.
- External signer and detached execution examples remain v2 work.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| VER-01 | Repository includes a local EVM/anvil example for ERC20 transfer or approve. [VERIFIED: .planning/REQUIREMENTS.md] | Use existing Anvil feature-gating and fixture patterns from `executor-evm` and `executor-mcp`; prefer executable example files plus a smoke/integration path that uses the public MCP flow. [VERIFIED: codebase Read/Bash] |
| VER-02 | Repository includes a generic contract call example using ABI. [VERIFIED: .planning/REQUIREMENTS.md] | Reuse `contract_call` / `ctx.actions.contractCall` shape and `counter.hex` fixture/ABI patterns rather than building a protocol recipe catalog. [VERIFIED: codebase Read/Bash] |
| VER-03 | Tests prove policy prevents disallowed chains/contracts/selectors. [VERIFIED: .planning/REQUIREMENTS.md] | Existing stdio policy negative grid already covers chain, contract, selector, native value, ERC20 spend, and raw calldata; Phase 7 should preserve/organize this as safety regression coverage and add signer-not-reached evidence where needed. [VERIFIED: crates/executor-mcp/tests/stdio_handshake.rs] |
| VER-04 | Tests prove failed simulation prevents signing. [VERIFIED: .planning/REQUIREMENTS.md] | Existing Anvil-backed simulation revert test proves `simulation_failure`; Phase 7 should strengthen it with no execution-report/signing side-effect assertions if Phase 6 added execution rows. [VERIFIED: crates/executor-mcp/tests/stdio_handshake.rs] |
| VER-05 | Tests prove strategy sandbox blocks forbidden host access. [VERIFIED: .planning/REQUIREMENTS.md] | Existing `strategy-js` forbidden globals suite blocks host globals, `require("fs")`, Deno, dynamic import, console, and process; Phase 7 should keep these as the canonical sandbox regression and optionally add an MCP-level smoke if needed. [VERIFIED: crates/strategy-js/tests/sandbox_host_globals.rs] |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- Use GitNexus to understand unfamiliar code and execution flows; `gitnexus_query` is preferred over grep for unfamiliar code exploration. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/CLAUDE.md]
- Run impact analysis before editing any function, class, or method; warn before proceeding if impact is HIGH or CRITICAL. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/CLAUDE.md]
- Run `gitnexus_detect_changes()` before committing to verify affected scope. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/CLAUDE.md]
- Never rename symbols with find-and-replace; use GitNexus rename tooling for symbol renames. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/CLAUDE.md]
- Do not add dashboard, landing page, marketplace, hosted platform, protocol recipe catalog, TypeScript compiler, custom DSL, opcode VM, or workflow DAG in v1. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/AGENTS.md]
- Strategy code must not access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients; strategy code returns `Action[]` and does not sign or broadcast. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/AGENTS.md]
- Simulation and policy must run before signing; stdio MCP servers must not write logs to stdout. [CITED: /Users/user/Documents/GitHub/onchain-strategy-mcp/AGENTS.md]
- Global user instruction forbids mentioning Claude in git commits and forbids working-tree reset/cleanup commands unless explicitly requested. [CITED: /Users/user/.claude/CLAUDE.md]

## Summary

Phase 7 should be planned as a verification-and-DX phase, not as a runtime feature phase. [VERIFIED: 07-CONTEXT.md] The repository already has the core harnesses needed for the phase: stdio MCP integration tests, Anvil-gated EVM fixtures, policy negative grids, signer configuration helpers, state/journal resource assertions, and sandbox forbidden-host regression tests. [VERIFIED: codebase Read/Bash] The planner should therefore allocate work to runnable examples, consolidation/strengthening of existing safety tests, and concise README/AGENTS docs that explain the final v1 loop. [VERIFIED: 07-CONTEXT.md]

The most important planning risk is duplicating harnesses instead of reusing existing ones. [VERIFIED: 07-CONTEXT.md] `crates/executor-mcp/tests/stdio_handshake.rs` already drives the public MCP server over stdio and has helper functions for config, policy files, signer env injection, strategy seeding, JSON-RPC calls, and journal resource reads. [VERIFIED: crates/executor-mcp/tests/stdio_handshake.rs] `crates/executor-evm/tests/common/anvil_fixture.rs` already encodes the Anvil skip-cleanly contract and `ANVIL_RPC_URL` behavior. [VERIFIED: crates/executor-evm/tests/common/anvil_fixture.rs]

**Primary recommendation:** Plan Phase 7 around `examples/` runnable assets plus a small number of high-signal integration tests that use the existing stdio + Anvil + journal/status harness, then refresh README/AGENTS with exact local commands and hot-wallet warnings. [VERIFIED: 07-CONTEXT.md; codebase Read/Bash]

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Runnable ERC20 approve/transfer example | Repository examples / MCP runtime | Local Anvil EVM | The example should enter through strategy registration/run and prove policy/simulation/signing/broadcast/receipt through the public runtime path. [VERIFIED: 07-CONTEXT.md; crates/executor-mcp/tests/stdio_handshake.rs] |
| Generic ABI contract call example | Repository examples / MCP runtime | Local Anvil EVM | The runtime already supports generic `contract_call`; the example should show ABI-based breadth without adding protocol-specific recipes. [VERIFIED: .planning/REQUIREMENTS.md; 07-CONTEXT.md] |
| Policy denial tests | MCP integration test layer | Policy crate unit tests | VER-03 is about externally observable runtime guarantees, so stdio tests should prove MCP-visible denial and journal/status evidence; lower-level policy tests remain supporting coverage. [VERIFIED: 07-CONTEXT.md; crates/executor-policy/tests/*] |
| Simulation-failure-before-signing tests | MCP integration test layer | EVM simulator / signer/state crates | The guarantee spans simulation, signer boundary, and persisted execution evidence, so the test belongs at the orchestration boundary. [VERIFIED: .planning/ROADMAP.md; crates/executor-mcp/tests/stdio_handshake.rs] |
| JS sandbox host-access tests | `strategy-js` unit/integration tests | MCP smoke test optional | Direct sandbox tests precisely prove forbidden globals/module access; MCP smoke is optional if planner wants end-to-end visibility. [VERIFIED: crates/strategy-js/tests/sandbox_host_globals.rs] |
| Usage documentation | Root docs | Example files | README/AGENTS should describe the final local runtime loop and point to runnable examples. [VERIFIED: 07-CONTEXT.md; README.md; AGENTS.md] |

## Standard Stack

### Core
| Library/Tool | Version | Purpose | Why Standard |
|--------------|---------|---------|--------------|
| Rust / Cargo | cargo 1.94.0 | Build, test, and run workspace crates. | Workspace is Rust 2024 and all tests are Cargo-driven. [VERIFIED: cargo --version; Cargo.toml] |
| `rmcp` | 1.5.0 | MCP server support and stdio protocol surface. | Existing `executor-mcp` crate depends on rmcp and stdio tests drive JSON-RPC tool/resource calls. [VERIFIED: Cargo.toml; cargo tree] |
| `tokio` | 1.52.1 | Async runtime for server and integration tests. | Existing stdio tests use `#[tokio::test]` and async process/IO helpers. [VERIFIED: cargo tree; crates/executor-mcp/tests/common/mod.rs] |
| `serde_json` | 1.0.149 | JSON-RPC request/response assertions and example payloads. | Existing harness sends/receives JSON-RPC with `serde_json::json!`. [VERIFIED: cargo tree; crates/executor-mcp/tests/common/mod.rs] |
| `alloy` | 2.0.1 | EVM provider, Anvil bindings, ABI/RPC primitives. | Existing EVM and Anvil-gated integration tests use Alloy provider/node bindings. [VERIFIED: cargo tree; crates/executor-mcp/Cargo.toml] |
| Foundry `anvil` | 1.5.1-stable | Local deterministic EVM chain for examples/tests. | Existing Anvil tests are gated and the environment has `anvil` installed. [VERIFIED: anvil --version; crates/executor-evm/tests/common/anvil_fixture.rs] |
| `rquickjs` | 0.11.0 | JavaScript sandbox runtime. | Existing `strategy-js` crate uses `rquickjs` and exposes `Sandbox::execute`. [VERIFIED: cargo tree; crates/strategy-js/src/sandbox.rs] |
| `rusqlite` | 0.39.0 | Local state/journal persistence. | Existing `executor-state` crate persists strategies, runs, journals, and executions. [VERIFIED: cargo tree; crates/executor-state/src/*] |

### Supporting
| Library/Tool | Version | Purpose | When to Use |
|--------------|---------|---------|-------------|
| `tempfile` | 3.27.0 | Per-test config, policy, and state DB isolation. | Use in stdio integration tests and example smoke tests rather than fixed repo-local mutable state. [VERIFIED: cargo tree; crates/executor-mcp/tests/common/mod.rs] |
| `toml` | 0.8.23 | Runtime and policy config parsing. | Use for generated config/policy fixture files. [VERIFIED: cargo tree; config.example.toml] |
| `schemars` | 1.2.1 | JSON schema snapshot contracts. | Use only if examples/docs require schema contract refresh; do not add new schema framework. [VERIFIED: cargo tree; crates/executor-core/tests/schema_snapshots.rs] |
| `hex` | 0.4.3 | Fixture bytecode decoding. | Use for local fixture deployment helpers. [VERIFIED: cargo tree; crates/executor-mcp/tests/stdio_handshake.rs] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Existing Rust stdio/Anvil harness | Shell scripts or a separate Node-based example runner | Adds another framework and weakens existing public-runtime assertions; only add shell wrappers if they call the existing binary/examples directly. [VERIFIED: 07-CONTEXT.md] |
| Existing checked-in bytecode fixtures | Runtime Solidity compilation with `solc` | `solc` was not available in the environment probe, while checked-in `.hex` fixtures already exist and avoid an extra local dependency. [VERIFIED: command probe; crates/executor-evm/tests/fixtures/*] |
| Unit-only policy tests | MCP-level policy safety tests | Unit tests are useful but VER-03 asks runtime safety, so public stdio tests should remain the phase gate. [VERIFIED: .planning/REQUIREMENTS.md; crates/executor-mcp/tests/stdio_handshake.rs] |

**Installation:** No new package installation is recommended for Phase 7; use the current Cargo workspace and existing Foundry Anvil dependency. [VERIFIED: cargo test --workspace --no-run; anvil --version]

**Version verification:** Direct versions were verified with `cargo tree -p executor-mcp --depth 1`, `cargo tree -p strategy-js --depth 1`, `cargo tree -p executor-state --depth 1`, `cargo --version`, and `anvil --version`. [VERIFIED: Bash]

## Architecture Patterns

### System Architecture Diagram

```text
Developer / Agent
  |
  | register strategy + config/policy/private-key env
  v
executor-mcp stdio server
  |
  | strategy_run
  v
strategy-js Sandbox::execute
  |        |
  |        +-- ctx.evm reads / ctx.actions builders
  v
Action[] validation
  |
  v
Action normalization / ABI encoding
  |
  v
Policy gate ---- deny -> MCP error + journal policy fail + simulation skipped
  |
  pass
  v
Simulation gate ---- fail -> MCP error + journal simulation fail + no signing
  |
  pass
  v
Local signer boundary -> broadcast -> receipt watcher
  |
  v
State DB: run, journal, execution report
  |
  v
MCP resources/tools: execution_get + journal://{run_id}
```
[VERIFIED: .planning/ROADMAP.md; crates/executor-mcp/tests/stdio_handshake.rs; AGENTS.md]

### Recommended Project Structure

```text
examples/
├── README.md                         # local example index and prerequisites [ASSUMED]
├── local-erc20-transfer-or-approve/  # VER-01 runnable assets [ASSUMED]
│   ├── strategy.js
│   ├── policy.toml
│   └── config.example.toml
└── generic-contract-call/            # VER-02 runnable assets [ASSUMED]
    ├── strategy.js
    ├── abi.json
    ├── policy.toml
    └── config.example.toml
crates/executor-mcp/tests/
└── stdio_handshake.rs                # existing public-runtime integration harness [VERIFIED: codebase]
crates/strategy-js/tests/
└── sandbox_host_globals.rs           # existing forbidden-host sandbox regression suite [VERIFIED: codebase]
README.md                             # root final loop docs [VERIFIED: codebase]
AGENTS.md                             # agent-facing runtime and safety docs [VERIFIED: codebase]
```

### Pattern 1: Public-runtime stdio tests
**What:** Spawn `executor-mcp`, initialize MCP, call tools/resources over JSON-RPC, then assert response bodies and persisted state/journal. [VERIFIED: crates/executor-mcp/tests/common/mod.rs]  
**When to use:** Use for VER-01 through VER-04 because these requirements are about externally observable runtime guarantees. [VERIFIED: .planning/REQUIREMENTS.md]

**Example:**
```rust
// Source: crates/executor-mcp/tests/common/mod.rs [VERIFIED]
let mut proc = spawn_server_with_config_text(config_text).await?;
let _ = initialize(&mut proc).await?;
let response = call_tool(&mut proc, 2, "strategy_run", json!({ "strategy_id": strategy_id })).await?;
```

### Pattern 2: Anvil-gated local EVM tests
**What:** Gate local-chain tests with `#[cfg(feature = "anvil-tests")]`, spawn or connect to Anvil, deploy checked-in bytecode fixtures, and early-return/skip when Anvil is unavailable. [VERIFIED: crates/executor-evm/tests/common/anvil_fixture.rs; crates/executor-mcp/tests/stdio_handshake.rs]  
**When to use:** Use for local execution examples and simulation-failure tests that require chain state. [VERIFIED: 07-CONTEXT.md]

**Example:**
```rust
// Source: crates/executor-evm/tests/common/anvil_fixture.rs [VERIFIED]
let Some(fixture) = AnvilFixture::try_spawn() else { return Ok(()); };
let rpc_url = fixture.rpc_url;
```

### Pattern 3: Policy fixture generation in tests
**What:** Generate temp policy TOML with explicit chains/contracts/selectors/native/erc20/raw-call rules. [VERIFIED: crates/executor-mcp/tests/stdio_handshake.rs]  
**When to use:** Use when proving policy denies specific unsafe cases or permits local example fixtures. [VERIFIED: crates/executor-mcp/tests/stdio_handshake.rs]

### Pattern 4: Sandbox forbidden-host assertions from inside JS
**What:** JS test strategy checks `globalThis` names are `undefined` and returns `noop`; Rust asserts `noop`. [VERIFIED: crates/strategy-js/tests/sandbox_host_globals.rs]  
**When to use:** Use for VER-05; keep direct sandbox tests as canonical precision tests. [VERIFIED: .planning/REQUIREMENTS.md]

### Anti-Patterns to Avoid
- **Creating a parallel test framework:** Existing Cargo/stdout/Anvil helpers already cover the public path. [VERIFIED: 07-CONTEXT.md; codebase]
- **Shell-only examples with no automated proof:** Phase success criteria require examples that execute, not just prose. [VERIFIED: 07-CONTEXT.md; .planning/ROADMAP.md]
- **Relying on hosted RPCs or protocol-specific app recipes:** Phase constraints require local-first deterministic examples and defer recipe catalogs. [VERIFIED: 07-CONTEXT.md]
- **Putting private keys in committed config/docs:** Signer docs should use env-var names and Anvil dev keys only for local examples. [VERIFIED: 07-CONTEXT.md; AGENTS.md]
- **Writing logs to stdout in example tooling:** Stdio MCP stdout must remain JSON-RPC only. [VERIFIED: AGENTS.md; crates/executor-mcp/tests/common/mod.rs]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP client harness | Custom parser/process framework | `crates/executor-mcp/tests/common/mod.rs` helpers | Existing helpers enforce JSON-RPC stdout discipline, initialize MCP, and parse tool results. [VERIFIED: codebase] |
| Local EVM fixture management | Ad hoc background chain scripts | Existing Anvil-gated patterns and checked-in bytecode fixtures | Existing tests already define skip semantics, chain ID 31337, fixture deployment, and `ANVIL_RPC_URL` behavior. [VERIFIED: crates/executor-evm/tests/common/anvil_fixture.rs] |
| ABI encoding/selector calculation in examples | Hand-computed calldata | `ctx.actions.contractCall`, ERC20 helpers, and runtime normalization | The runtime owns ABI encoding and policy-ready action normalization. [VERIFIED: .planning/ROADMAP.md; crates/executor-mcp/src/tools.rs] |
| Sandbox security assertions | Browser/Node mocks | `strategy-js::Sandbox::execute` tests | The actual QuickJS sandbox boundary is in `strategy-js`. [VERIFIED: crates/strategy-js/tests/sandbox_host_globals.rs] |
| Policy evaluation proof | One-off string checks | Existing policy TOML model plus stdio error/journal assertions | Existing tests assert stable `policy_violation` shape and journal decisions. [VERIFIED: crates/executor-mcp/tests/stdio_handshake.rs] |

**Key insight:** Phase 7 should package and prove the already-built runtime loop; custom wrappers would make tests less representative than the current public MCP path. [VERIFIED: 07-CONTEXT.md; codebase]

## Common Pitfalls

### Pitfall 1: Examples bypass the runtime they are meant to prove
**What goes wrong:** Example calls EVM helpers directly or broadcasts with Alloy instead of registering/running a strategy. [ASSUMED]  
**Why it happens:** Direct crate calls are simpler than stdio MCP orchestration. [ASSUMED]  
**How to avoid:** Plan examples to enter through strategy registration/run, policy config, signer env, execution_get, and journal resources. [VERIFIED: 07-CONTEXT.md]  
**Warning signs:** Example never calls `strategy_register`, `strategy_run`, `execution_get`, or `journal://...`. [VERIFIED: existing MCP surface in tests]

### Pitfall 2: Anvil tests become flaky or non-deterministic
**What goes wrong:** Tests assume a persistent port, account, nonce, or external chain state. [ASSUMED]  
**Why it happens:** Local devnet orchestration is easy to globalize accidentally. [ASSUMED]  
**How to avoid:** Use temp DBs, temp policy files, Anvil chain ID 31337, existing fixture bytecode, and skip-cleanly behavior. [VERIFIED: crates/executor-evm/tests/common/anvil_fixture.rs; crates/executor-mcp/tests/stdio_handshake.rs]  
**Warning signs:** Tests require a pre-running `anvil` without feature gating or mutate repo-local `state.db`. [VERIFIED: config.example.toml; existing test patterns]

### Pitfall 3: Safety tests assert only error codes, not no-signing/no-side-effects
**What goes wrong:** A simulation/policy error is returned, but the test does not prove signing/execution was not reached. [ASSUMED]  
**Why it happens:** Error-envelope assertions are easier than checking execution reports/state rows. [ASSUMED]  
**How to avoid:** Add assertions against execution report absence/status and journal decision rows after policy/simulation failures, using Phase 6 status surfaces. [VERIFIED: .planning/ROADMAP.md; existing journal resource helper]

### Pitfall 4: Docs imply production custody or hosted service semantics
**What goes wrong:** README suggests wallet-product guarantees or recommends storing real private keys casually. [ASSUMED]  
**Why it happens:** Examples need a private key for Anvil and can be misread as production guidance. [ASSUMED]  
**How to avoid:** Clearly label local signer as v1 local hot-wallet custody and use env-var private key handling. [VERIFIED: 07-CONTEXT.md; AGENTS.md]

### Pitfall 5: Updating docs with obsolete conceptual API names
**What goes wrong:** README examples use older conceptual names like `ctx.source.erc20Balance` or `ctx.action.*` instead of implemented `ctx.evm.*` / `ctx.actions.*` shapes. [VERIFIED: README.md vs current tests]  
**Why it happens:** Root README is more conceptual than current implementation. [VERIFIED: README.md]  
**How to avoid:** Base docs/examples on current test strategy sources and action builders. [VERIFIED: crates/strategy-js/tests/*; crates/executor-mcp/tests/stdio_handshake.rs]

## Code Examples

### Stdio tool call harness
```rust
// Source: crates/executor-mcp/tests/common/mod.rs [VERIFIED]
let r = call_tool(
    &mut proc,
    2,
    "strategy_run",
    json!({ "strategy_id": strategy_id }),
)
.await?;
```

### Policy-denial assertion shape
```rust
// Source: crates/executor-mcp/tests/stdio_handshake.rs [VERIFIED]
assert_eq!(err["code"].as_i64(), Some(-32017));
assert_eq!(err["data"]["code"].as_str(), Some("strategy_runtime_error"));
assert_eq!(err["data"]["kind"].as_str(), Some("policy_violation"));
assert_eq!(err["data"]["rule"].as_str(), Some("selector_not_allowed"));
```

### Simulation-failure assertion shape
```rust
// Source: crates/executor-mcp/tests/stdio_handshake.rs [VERIFIED]
assert_eq!(err["data"]["kind"].as_str(), Some("simulation_failure"));
assert_eq!(err["data"]["action_index"].as_i64(), Some(0));
assert_eq!(err["data"]["fail_reason"].as_str(), Some("revert"));
```

### Sandbox forbidden globals pattern
```rust
// Source: crates/strategy-js/tests/sandbox_host_globals.rs [VERIFIED]
const names = ["console", "fetch", "process", "child_process", "fs"];
for (const n of names) {
  if (typeof globalThis[n] !== "undefined") return "FOUND: " + n;
}
return "noop";
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Prose-only examples | Runnable local Anvil examples through the actual runtime flow | Phase 7 decision | Planner should create executable repo assets, not only README snippets. [VERIFIED: 07-CONTEXT.md] |
| Unit-only safety coverage | Public stdio tests plus journal/status evidence | Phase 5/7 carry-forward | Policy/simulation guarantees should be asserted at the MCP boundary. [VERIFIED: STATE.md; crates/executor-mcp/tests/stdio_handshake.rs] |
| Conceptual README API names | Current implemented `ctx.evm.*`, `ctx.actions.*`, `ctx.units.*`, `ctx.address.*` names | Phase 4 implementation | Docs need implementation-aligned examples. [VERIFIED: README.md; REQUIREMENTS.md; strategy-js tests] |
| External signer/detached examples | Local env-var private-key signer examples only | v1 scope decision | Do not document external signer flows in Phase 7. [VERIFIED: 07-CONTEXT.md; REQUIREMENTS.md] |

**Deprecated/outdated:**
- Root README currently presents conceptual `ctx.source.*` / `ctx.action.*` examples and should be refreshed to current implementation vocabulary. [VERIFIED: README.md; strategy-js tests]
- `config.example.toml` header says future `[policy]` and `[signer]` sections will be added even though tests already use those sections after later phases; refresh if Phase 6 config surface is final. [VERIFIED: config.example.toml; crates/executor-mcp/tests/stdio_handshake.rs]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Suggested `examples/` directory structure and file names. | Recommended Project Structure | Planner may choose different names; low risk if examples remain runnable. |
| A2 | Examples bypassing runtime is a likely pitfall. | Common Pitfalls | If planner already requires stdio path, this is redundant. |
| A3 | Anvil tests can become flaky through persistent ports/accounts/nonces. | Common Pitfalls | Could overemphasize isolation, but aligns with existing harness. |
| A4 | Safety tests can pass error checks without proving no signer side effects. | Common Pitfalls | If Phase 6 already has explicit no-side-effect tests, this becomes a verification reminder. |
| A5 | Docs may accidentally imply production custody semantics. | Common Pitfalls | Documentation review should still validate wording. |

## Open Questions

1. **What exact Phase 6 execution status/report API shape should Phase 7 assert?**
   - What we know: ROADMAP says Phase 6 adds execution status query and receipt-backed reports. [VERIFIED: .planning/ROADMAP.md]
   - What's unclear: This research environment has Phase 6 artifacts in progress and uncommitted review/pattern docs; exact final API should be re-read before implementation. [VERIFIED: git status context]
   - Recommendation: Planner should add a Wave 0 inspection task to identify final `execution_get` response shape and execution table helpers before writing VER-01/VER-04 assertions. [ASSUMED]

2. **Should examples be executable Rust integration tests, shell scripts, or documented manual commands?**
   - What we know: User decisions prefer runnable repository examples/fixtures over prose-only examples and existing tests are Cargo-based. [VERIFIED: 07-CONTEXT.md; codebase]
   - What's unclear: Preferred UX wrapper is not locked. [VERIFIED: 07-CONTEXT.md]
   - Recommendation: Use example files plus Cargo integration smoke tests as the verification source; optional shell snippets in docs should call the same binary/config. [ASSUMED]

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|-------------|-----------|---------|----------|
| Cargo/Rust | Build/test/examples | ✓ | cargo 1.94.0 | None needed. [VERIFIED: command probe] |
| Anvil | Local EVM examples/tests | ✓ | 1.5.1-stable | Existing tests skip when unavailable; `ANVIL_RPC_URL` can point to external devnet. [VERIFIED: command probe; anvil_fixture.rs] |
| Node.js | GitNexus/GSD tooling only | ✓ | v25.5.0 | Not needed for Rust tests/examples. [VERIFIED: command probe] |
| solc | Runtime contract compilation | ✗ | — | Use checked-in `.hex` fixtures; do not add runtime `solc` dependency. [VERIFIED: command probe; executor-evm fixtures] |

**Missing dependencies with no fallback:** None identified for Phase 7 if checked-in bytecode fixtures are reused. [VERIFIED: command probe]

**Missing dependencies with fallback:** `solc` is missing; fallback is existing `.hex` fixtures and `.sol-src.txt` audit files. [VERIFIED: command probe; crates/executor-evm/tests/fixtures/*]

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust Cargo test + Tokio integration tests. [VERIFIED: Cargo.toml; tests] |
| Config file | Cargo workspace `Cargo.toml`; runtime example config from `config.example.toml` and generated temp config files. [VERIFIED: Cargo.toml; config.example.toml; common/mod.rs] |
| Quick run command | `cargo test -p strategy-js --test sandbox_host_globals` for VER-05 and `cargo test -p executor-policy` for lower-level policy checks. [VERIFIED: test files; cargo test --workspace --no-run] |
| Full suite command | `cargo test --workspace` plus `cargo test -p executor-mcp --features anvil-tests` for local-chain integration gates. [VERIFIED: cargo test --workspace --no-run; cargo test -p executor-mcp --features anvil-tests --no-run] |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|--------------|
| VER-01 | Local Anvil example executes ERC20 approve or transfer through runtime. | integration/anvil | `cargo test -p executor-mcp --features anvil-tests local_erc20_example_executes -- --nocapture` | ❌ Wave 0/implementation |
| VER-02 | Generic ABI contract call example executes successfully. | integration/anvil | `cargo test -p executor-mcp --features anvil-tests generic_contract_call_example_executes -- --nocapture` | ❌ Wave 0/implementation |
| VER-03 | Policy blocks disallowed chains/contracts/selectors. | integration | Existing: `cargo test -p executor-mcp strategy_run_returns_policy_violation_for_disallowed_chain strategy_run_returns_policy_violation_for_disallowed_contract strategy_run_returns_policy_violation_for_disallowed_selector` is not valid as one Cargo invocation with multiple filters; run by substring or full test file. Recommended: `cargo test -p executor-mcp strategy_run_returns_policy_violation_for_disallowed -- --nocapture`. [VERIFIED: cargo CLI error; test names] | ✅ existing partial coverage |
| VER-04 | Failed simulation prevents signing. | integration/anvil | Existing proof: `cargo test -p executor-mcp --features anvil-tests strategy_run_returns_simulation_failed_when_revert -- --nocapture`; add no-signing/no-execution side-effect assertion if Phase 6 exposes it. [VERIFIED: stdio_handshake.rs] | ✅ existing partial coverage |
| VER-05 | JS sandbox blocks forbidden host access. | unit/integration | `cargo test -p strategy-js --test sandbox_host_globals -- --nocapture` | ✅ existing coverage |

### Sampling Rate
- **Per task commit:** `cargo test -p strategy-js --test sandbox_host_globals` and the narrow `executor-mcp` test substring touched by the task. [VERIFIED: test layout]
- **Per wave merge:** `cargo test --workspace` and, for Anvil-dependent waves, `cargo test -p executor-mcp --features anvil-tests`. [VERIFIED: existing feature gate]
- **Phase gate:** `cargo test --workspace`, `cargo test -p executor-mcp --features anvil-tests`, and `cargo clippy --workspace --all-targets -- -D warnings`. [VERIFIED: STATE.md]

### Wave 0 Gaps
- [ ] Create or identify `examples/` runnable assets for VER-01 and VER-02. [VERIFIED: no root examples directory found by file listing]
- [ ] Add MCP-level local ERC20 example smoke test using Phase 6 execution report/status assertions. [ASSUMED]
- [ ] Add MCP-level generic ABI contract call example smoke test using existing `counter.hex` or equivalent fixture. [ASSUMED]
- [ ] Re-read final Phase 6 `execution_get` response schema before writing no-signing/no-execution side-effect assertions. [ASSUMED]
- [ ] Refresh README/AGENTS examples from current `ctx.evm.*` and `ctx.actions.*` test patterns. [VERIFIED: README.md; strategy-js tests]

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|------------------|
| V2 Authentication | no | No user auth surface in local stdio runtime for this phase. [VERIFIED: ROADMAP/AGENTS local runtime scope] |
| V3 Session Management | no | No HTTP sessions or browser sessions in Phase 7. [VERIFIED: ROADMAP/AGENTS] |
| V4 Access Control | yes | Policy allowlists for chain, contract, selector, native value, ERC20 spend, and raw calldata. [VERIFIED: REQUIREMENTS.md; policy tests] |
| V5 Input Validation | yes | JSON schema-backed MCP inputs plus action validation and ABI encoding gates. [VERIFIED: REQUIREMENTS.md; schema snapshots; validation tests] |
| V6 Cryptography | yes | Local signer crate handles private-key signing; examples must use env-var private-key config and Anvil dev keys only. [VERIFIED: 07-CONTEXT.md; executor-signer tests] |
| V8 Data Protection | yes | Avoid logging/leaking private keys and keep strategy sandbox away from secrets/env/process APIs. [VERIFIED: AGENTS.md; sandbox_host_globals.rs; local_execution.rs] |

### Known Threat Patterns for Rust MCP + local EVM runtime

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Strategy accesses host filesystem/process/env/private keys | Information Disclosure / Elevation of Privilege | `strategy-js` forbidden globals/module access tests; do not expose host APIs in `ctx`. [VERIFIED: sandbox_host_globals.rs; AGENTS.md] |
| Strategy bypasses policy by hand-building actions | Tampering | Validate `Action[]` after sandbox output and before simulation/signing. [VERIFIED: REQUIREMENTS.md; STATE.md] |
| Policy denied action still reaches simulation/signing | Elevation of Privilege | Assert policy fail rows and simulation skipped/no signing side effects. [VERIFIED: stdio_handshake.rs; ASSUMED for added no-signing assertion] |
| Simulation failure still reaches signer | Tampering / Elevation of Privilege | Assert `simulation_failure` and absence of signing/execution side effects. [VERIFIED: stdio_handshake.rs; ASSUMED for added Phase 6 state check] |
| Private key leaks in errors/logs/docs | Information Disclosure | Use env-var key config; tests already assert broadcast error does not include private-key substring. [VERIFIED: executor-signer/tests/local_execution.rs] |
| Stdio logs corrupt JSON-RPC | Denial of Service / Tampering | Keep tracing/logs on stderr and assert stdout parses as JSON-RPC. [VERIFIED: common/mod.rs; AGENTS.md] |

## Sources

### Primary (HIGH confidence)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/.planning/phases/07-examples-tests-and-documentation/07-CONTEXT.md` - locked Phase 7 decisions and deferred scope.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/.planning/REQUIREMENTS.md` - VER-01..VER-05 definitions and v1 scope.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/.planning/ROADMAP.md` - Phase 7 definition and success criteria.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/.planning/STATE.md` - carry-forward decisions and test/clippy state.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/CLAUDE.md` and `/Users/user/Documents/GitHub/onchain-strategy-mcp/AGENTS.md` - project constraints and stack boundaries.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/tests/common/mod.rs` - stdio MCP harness.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-mcp/tests/stdio_handshake.rs` - policy/simulation/journal/status integration patterns.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/executor-evm/tests/common/anvil_fixture.rs` - Anvil fixture and skip contract.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/crates/strategy-js/tests/sandbox_host_globals.rs` - forbidden-host sandbox tests.
- `cargo tree`, `cargo test --workspace --no-run`, `cargo test -p executor-mcp --features anvil-tests --no-run`, `cargo --version`, `anvil --version` - environment/version verification.

### Secondary (MEDIUM confidence)
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/README.md` - conceptual docs needing refresh; implementation examples should be sourced from tests.
- `/Users/user/Documents/GitHub/onchain-strategy-mcp/config.example.toml` - current runtime config example, but likely needs Phase 6/7 refresh.

### Tertiary (LOW confidence)
- None; no web-only sources were used.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - verified from Cargo files, `cargo tree`, and tool version probes.
- Architecture: HIGH - based on locked roadmap/requirements and existing integration tests.
- Pitfalls: MEDIUM - several are inferred planning risks but anchored in existing constraints and harness patterns.
- Validation architecture: HIGH for existing tests/harnesses, MEDIUM for new example test names because exact file names are planner discretion.

**Research date:** 2026-04-29  
**Valid until:** 2026-05-06 for Phase 7 planning, because Phase 6 artifacts may still affect exact execution status assertions.
