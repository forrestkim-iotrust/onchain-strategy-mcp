# Stack Research

**Domain:** MCP runtime for EVM automation  
**Researched:** 2026-04-24  
**Confidence:** Medium-high

## Recommended Stack

### Core

| Technology | Current reference | Purpose | Rationale |
|------------|-------------------|---------|-----------|
| Rust | Rust 2024 edition | Runtime implementation | Strong boundary control for MCP, signer, policy, EVM adapter, JS runner, and journal layers. |
| `rmcp` | `1.5.0` from crates.io search | MCP server SDK | Official Rust SDK line for MCP. Supports server tools, resources, prompts, stdio, async runtime patterns. |
| `tokio` | Current stable | Async runtime | Required foundation for MCP server IO, RPC calls, receipt waiting, and concurrent strategy runs. |
| `serde` / `serde_json` | Current stable | Data contracts | Strategy actions, policies, journal entries, and MCP tool schemas should be structured JSON. |
| `schemars` | `1.2.1` from crates.io search | JSON Schema | MCP tools need explicit input/output schemas. MCP recommends JSON Schema 2020-12 by default. |
| `alloy` | `2.0.1` from crates.io search | EVM ABI/RPC/tx primitives | Best fit for generic contract reads/calls, calldata encoding, simulation, signing, broadcast, and receipts. |
| `rquickjs` | `0.11.0` from crates.io search | Sandboxed JavaScript | Embeds JavaScript without adding a TypeScript compiler or Node runtime. |
| `rusqlite` | `0.39.0` from crates.io search | Local durable state | Good enough for v1 local/single-operator journal, strategies, runs, policies, and receipts. |

### Supporting

| Library | Purpose |
|---------|---------|
| `tracing`, `tracing-subscriber` | Structured stderr logs; never pollute MCP stdout in stdio mode. |
| `thiserror` | Stable error taxonomy across MCP/tool/runtime/policy/EVM layers. |
| `sha2` or equivalent | Strategy source hash, action hash, journal integrity fields. |
| `uuid` or ULID crate | Strategy/run/action IDs. |
| `tempfile` | Isolated integration tests. |

## MCP Implications

- MCP protocol version `2025-11-25` is the current latest version in the official spec.
- MCP uses JSON-RPC and lifecycle negotiation. Initialization must negotiate protocol version and capabilities.
- Server primitives are prompts, resources, and tools. Tools are model-controlled, so write-capable tools need explicit safety boundaries.
- Stdio is the right first transport. The server must only write valid MCP messages to stdout; logs go to stderr.
- Streamable HTTP can come later. It adds authorization, session, and Origin concerns that are not required for v1.

## What Not To Use In v1

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| TypeScript compiler | Adds build/transpile complexity before core runtime is proven. | Plain JS strategy source, optional `.d.ts` later. |
| Node.js runtime embedding | Harder to sandbox tightly. | QuickJS via `rquickjs`. |
| Custom DSL/opcode VM | Delays the first useful runtime. | JavaScript + small `ctx` API returning `Action[]`. |
| Protocol-specific recipe catalog | Core becomes app-specific quickly. | Generic ABI contract read/call plus ERC20/native helpers. |
| Streamable HTTP first | More auth/session risk early. | Stdio first. |

## Sources

- MCP Base Protocol Overview: https://modelcontextprotocol.io/specification/2025-11-25/basic/index
- MCP Lifecycle: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- MCP Transports: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Server Features: https://modelcontextprotocol.io/specification/2025-11-25/server/index
- MCP Tools: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- MCP Security Best Practices: https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices
- Official Rust SDK: https://github.com/modelcontextprotocol/rust-sdk
- Official TypeScript SDK: https://ts.sdk.modelcontextprotocol.io/

---
*Stack research for: onchain-strategy-mcp*
*Researched: 2026-04-24*
