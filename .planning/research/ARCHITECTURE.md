# Architecture Research

**Domain:** MCP runtime for EVM automation  
**Researched:** 2026-04-24  
**Confidence:** Medium

## Recommended Architecture

```text
MCP client / agent
  |
  v
executor-mcp
  - MCP lifecycle
  - tools/resources
  - JSON schemas
  - stdio transport
  |
  v
executor-core
  - Strategy
  - StrategyRun
  - Action[]
  - PolicyDecision
  - ExecutionReport
  |
  +--> strategy-js
  |     - sandboxed JS
  |     - ctx API
  |
  +--> evm
  |     - ABI encode/decode
  |     - read/simulate/send/receipt
  |
  +--> policy
  |     - chain allowlist
  |     - target/selector/value/spend limits
  |
  +--> signer
  |     - LocalSigner in v1
  |     - interface for later external signer
  |
  +--> state
        - SQLite
        - journal
        - strategy/run/execution status
```

## Component Responsibilities

| Component | Responsibility |
|-----------|----------------|
| `executor-mcp` | MCP protocol surface, tools, resources, schema wiring, stdio entrypoint. |
| `executor-core` | Domain types and orchestration of strategy run lifecycle. |
| `strategy-js` | QuickJS runtime, `ctx` host API, JS execution budgets. |
| `executor-evm` | Contract reads, ABI encoding, simulation, send, receipt wait. |
| `executor-policy` | Allow/deny decisions before signing. |
| `executor-signer` | Signer trait and local signer implementation. |
| `executor-state` | SQLite repository, journal, status queries. |

## Suggested Workspace

```text
Cargo.toml
crates/
  executor-mcp/
  executor-core/
  strategy-js/
  executor-evm/
  executor-policy/
  executor-signer/
  executor-state/
examples/
tests/
```

## Runtime Flow

```text
strategy_run_once
  -> load strategy
  -> create ctx
  -> run JS
  -> collect source reads
  -> receive Action[]
  -> validate actions
  -> simulate actions
  -> policy check
  -> sign locally
  -> broadcast
  -> wait receipt
  -> persist journal/report
```

## MCP Surface Shape

Tools:

- `strategy_register`
- `strategy_list`
- `strategy_get`
- `strategy_delete`
- `strategy_run_once`
- `execution_get`
- `policy_get`
- `policy_update`

Resources:

- `strategy://{strategy_id}`
- `execution://{execution_id}`
- `journal://{execution_id}`

Prompts:

- `write_evm_strategy`
- `review_evm_strategy`

## Build Order

1. Workspace and domain contracts.
2. MCP stdio server with placeholder tools.
3. SQLite state and journal.
4. JS runner and small `ctx` API.
5. EVM read/action validation.
6. Simulation and policy.
7. Local signer, broadcast, receipt.
8. Examples and tests.

## Sources

- MCP Lifecycle: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- MCP Transports: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Server Features: https://modelcontextprotocol.io/specification/2025-11-25/server/index
- MCP Tools: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- Official Rust SDK: https://github.com/modelcontextprotocol/rust-sdk

---
*Architecture research for: onchain-strategy-mcp*
*Researched: 2026-04-24*
