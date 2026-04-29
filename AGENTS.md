# AGENTS.md

## Project

`onchain-strategy-mcp` is a local MCP runtime that lets an AI agent write, review, run, and audit EVM automation strategies.

v1 uses sandboxed JavaScript over a small `ctx` API. Strategies return `Action[]`; the runtime validates, simulates, policy-checks, signs with a local hot-wallet key from the operator environment, broadcasts to the configured RPC, waits for receipts, and records journal/report state.

Core value: AI agent가 EVM 자동화 로직을 실제 온체인 실행으로 바꾸되, 모든 실행은 policy 검사를 거치고 기록으로 남아야 한다.

## Strategy authoring loop

1. Use `write_evm_strategy` when drafting strategy JavaScript for the current `ctx` API.
2. Keep strategy code deterministic and data-oriented: read through `ctx.evm.*`, return `ctx.actions.*`, and avoid host access assumptions.
3. Use `review_evm_strategy` before registration to check action shape, address/amount handling, policy expectations, and secret handling.
4. Register reviewed source with `strategy_register`.
5. Run explicitly with `strategy_run`; v1 has no autonomous scheduling loop.
6. Capture the returned run ID for status and journal review.

Checked-in examples:

- `examples/strategies/erc20-approve.js` — ERC20 approve-shaped local Anvil strategy.
- `examples/strategies/generic-counter-call.js` — generic ABI counter `increment()` call.
- `examples/policies/local-anvil.toml` — local chain `31337` policy fixture.

## Safety checks before execution

- Agents must not request or print raw private keys.
- Raw private keys must stay only in the operator environment variable named by `[signer].private_key_env`, normally `EXECUTOR_PRIVATE_KEY`.
- Do not put raw private keys in strategy JS, committed config, prompts, logs, reports, or comments.
- Do not bypass policy failures, simulation failures, sandbox errors, or validation errors.
- Confirm policy scope before running non-noop strategies: chain ID, contract address, selector, native value, ERC20 spend, and raw-call settings.
- Treat the v1 signer as local hot-wallet custody. A signer address may appear in execution reports; the private key value must not.

## Execution status and journal review

After every `strategy_run`:

1. Inspect the returned run ID with `execution_get`.
2. If using resources, read `execution://{run_id}` for the same receipt-backed execution report.
3. Check each action report for status, transaction hash, receipt status, gas used, signer address, and stable error kind/detail.
4. Review journal resources for source reads, action validation, simulation outcome, policy decision, and execution outcome.
5. If execution is denied or fails, fix strategy/policy/config first; do not retry by weakening safety checks unless the operator intentionally changes policy.

## Commands agents should run

Documentation and safety regression checks:

```bash
cargo test -p executor-mcp --features anvil-tests --test verification_examples -- --nocapture
cargo test -p executor-mcp --test verification_safety
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Useful local files to inspect before strategy work:

```text
config.example.toml
examples/policies/local-anvil.toml
examples/strategies/erc20-approve.js
examples/strategies/generic-counter-call.js
```

## Hard boundaries

- Do not add a dashboard, landing page, marketplace, hosted platform, or protocol recipe catalog in v1.
- Do not add a TypeScript compiler, custom DSL, opcode VM, workflow DAG, external signer adapter, or detached execution path in v1.
- Strategy code must not access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients.
- Strategy code returns `Action[]`; it does not sign or broadcast.
- Simulation and policy must run before signing.
- Stdio MCP servers must not write logs to stdout. Use stderr/tracing.

## Technology stack

- Rust workspace
- `rmcp` for MCP server support
- `tokio` for async runtime
- `serde` / `schemars` for structured contracts and JSON Schema
- `alloy` for EVM ABI/RPC/transaction primitives
- `rquickjs` for sandboxed JavaScript
- `rusqlite` for local state and journal
- `tracing` for stderr-only logs
- `thiserror` for typed errors

## Project references

Use planning artifacts before implementation:

- Requirements: `.planning/REQUIREMENTS.md`
- Roadmap: `.planning/ROADMAP.md`
- State: `.planning/STATE.md`
- Phase notes: `.planning/phases/`

Do not commit unless the user explicitly approves, except when executing an approved GSD plan that requires atomic task commits.

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **onchain-strategy-mcp** (1889 symbols, 5062 relationships, 160 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/onchain-strategy-mcp/context` | Codebase overview, check index freshness |
| `gitnexus://repo/onchain-strategy-mcp/clusters` | All functional areas |
| `gitnexus://repo/onchain-strategy-mcp/processes` | All execution flows |
| `gitnexus://repo/onchain-strategy-mcp/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
