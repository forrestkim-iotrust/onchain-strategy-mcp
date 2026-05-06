# Feature Research

**Domain:** MCP runtime for EVM automation  
**Researched:** 2026-04-24  
**Confidence:** Medium

## Table Stakes

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| MCP stdio server | Local MCP servers commonly run as subprocesses. | Medium | Must follow lifecycle/capability negotiation. |
| Tool schemas | Agents need discoverable, valid tool inputs. | Medium | Use Rust structs + JSON Schema. |
| Structured tool outputs | Agents need parseable run/execution results. | Medium | Avoid prose-only logs. |
| Strategy registration | Runtime must store strategy source and metadata. | Medium | Include source hash and status. |
| Sandboxed JS runner | Core programming surface for agents. | High | No arbitrary host access. |
| Small `ctx` API | Keeps strategy authoring easy and constrained. | Medium | `ctx.evm`, `ctx.actions`, `ctx.units`, `ctx.noop`. |
| `Action[]` model | Strategy output must be structured before execution. | Medium | Simpler than ActionGraph/DAG. |
| Generic EVM reads | Needed for broad EVM usability. | High | ABI-based `contractRead` plus ERC20/native helpers. |
| Generic EVM writes | Needed for "almost anything on EVM". | High | ABI-based `contractCall`, `rawCall`, `nativeTransfer`. |
| Simulation before signing | Basic execution safety. | High | Required before local signer use. |
| Policy before signing | Prevents agent-generated unsafe txs. | High | Chain/contract/selector/value/spend constraints. |
| Local signer | Completes v1 execution loop. | Medium-high | Local hot wallet assumption. |
| Broadcast + receipt | Runtime must close the loop. | Medium-high | Record tx hash and receipt. |
| Journal | Needed for management and debugging. | Medium | Persist source reads, actions, decisions, receipts, errors. |

## Differentiators

| Feature | Value | Complexity | Notes |
|---------|-------|------------|-------|
| Generic ABI call support | Avoids app-specific core while keeping EVM broad. | High | Primary v1 differentiator. |
| Agent-friendly JS strategy surface | Lets agents write actual automation logic. | Medium | Easier than YAML/graph DSL. |
| Runtime-enforced action pipeline | Separates strategy code from execution authority. | High | `Action[] -> simulate -> policy -> sign -> send`. |
| Strong local-first stance | Useful before cloud/multi-tenant complexity. | Low | Fits MCP subprocess model. |

## Anti-Features

| Feature | Why It Is Tempting | Why It Is Wrong For v1 | Alternative |
|---------|--------------------|------------------------|-------------|
| TypeScript execution | Better DX and types. | Requires compiler/module pipeline too early. | Plain JS, add `.d.ts` later. |
| External signer/detached execution | Cleaner custody story. | Adds external protocol and result ingestion before core loop works. | Local signer with `Signer` boundary. |
| Scheduler | Makes it feel like a bot. | Adds lifecycle and reliability complexity. | Explicit `strategy_run_once` first. |
| Dashboard | Easier to demo visually. | Pulls project away from MCP/runtime. | MCP tools/resources and examples. |
| Protocol recipe catalog | Fast examples. | Core bloat. | Generic calls plus small ERC20/native helpers. |

## MVP Definition

Launch with:

- MCP server over stdio.
- Strategy register/list/get/delete/run_once tools.
- Sandboxed JS `run(ctx)` returning `Action[]`.
- Generic `ctx.evm.readContract`.
- Generic `ctx.actions.contractCall`, `rawCall`, `nativeTransfer`.
- ERC20 balance/allowance/approve/transfer helpers.
- Local signer configured by environment/config.
- Simulation, policy check, broadcast, receipt wait.
- SQLite journal and execution status lookup.
- Example strategies and integration tests against a local EVM.

Defer:

- Streamable HTTP.
- TypeScript compiler.
- External signer.
- Detached execution.
- Scheduler/reconcile loop.
- Capability registry.
- Multi-tenant support.

## Sources

- MCP Server Features: https://modelcontextprotocol.io/specification/2025-11-25/server/index
- MCP Tools: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- MCP Transports: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Security Best Practices: https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices

---
*Feature research for: onchain-strategy-mcp*
*Researched: 2026-04-24*
