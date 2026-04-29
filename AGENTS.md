# AGENTS.md

## Project

`onchain-strategy-mcp` is an MCP runtime that lets an AI agent code, run, and manage EVM automation strategies.

v1 uses sandboxed JavaScript over a small `ctx` API. Strategies return `Action[]`; the runtime validates, simulates, policy-checks, signs with a local signer, broadcasts, waits for receipts, and records a journal.

Core value: AI agent가 EVM 자동화 로직을 실제 온체인 실행으로 바꾸되, 모든 실행은 policy 검사를 거치고 기록으로 남아야 한다.

## Technology Stack

Planned stack:

- Rust workspace
- `rmcp` for MCP server support
- `tokio` for async runtime
- `serde` / `schemars` for structured contracts and JSON Schema
- `alloy` for EVM ABI/RPC/transaction primitives
- `rquickjs` for sandboxed JavaScript
- `rusqlite` for local state and journal
- `tracing` for stderr-only logs
- `thiserror` for typed errors

## Architecture

Target crate boundaries:

```text
crates/
  executor-mcp/
  executor-core/
  strategy-js/
  executor-evm/
  executor-policy/
  executor-signer/
  executor-state/
```

Runtime flow:

```text
strategy_run_once
  -> load strategy
  -> run sandboxed JS
  -> receive Action[]
  -> validate actions
  -> simulate
  -> policy check
  -> local sign
  -> broadcast
  -> wait receipt
  -> journal/report
```

## Hard Boundaries

- Do not add a dashboard, landing page, marketplace, hosted platform, or protocol recipe catalog in v1.
- Do not add a TypeScript compiler, custom DSL, opcode VM, or workflow DAG in v1.
- Strategy code must not access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients.
- Strategy code returns `Action[]`; it does not sign or broadcast.
- Simulation and policy must run before signing.
- Local signer is v1-only hot-wallet custody; keep signer behind an interface for later external signers.
- Stdio MCP servers must not write logs to stdout. Use stderr/tracing.

## GSD Workflow

Use planning artifacts before implementation:

- Project context: `.planning/PROJECT.md`
- Requirements: `.planning/REQUIREMENTS.md`
- Roadmap: `.planning/ROADMAP.md`
- State: `.planning/STATE.md`
- Research: `.planning/research/`

Start implementation with:

```text
$gsd-plan-phase 1
```

Do not commit unless the user explicitly approves.

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
