# Research Summary

**Domain:** MCP runtime for EVM automation  
**Researched:** 2026-04-24

## Key Findings

**Stack:** Rust workspace, `rmcp`, `tokio`, `serde`, `schemars`, `alloy`, `rquickjs`, `rusqlite`, `tracing`, `thiserror`.

**MCP:** Target protocol `2025-11-25`. Implement lifecycle/capability negotiation and stdio first. Use tools for operations, resources for strategy/execution/journal inspection, and prompts for strategy authoring/review.

**Runtime:** v1 should avoid TS compiler, custom DSL, opcode VM, scheduler, detached execution, and dashboard. The useful center is plain JS + small `ctx` + `Action[]`.

**EVM:** "못하는 게 거의 없는" 느낌 comes from generic ABI-based `contractRead`, `contractCall`, explicit `rawCall`, and native transfer, not from protocol-specific recipes.

**Safety:** Simulation and policy before signing. Local signer is acceptable for v1 if documented as local hot-wallet custody and guarded by allowlists/limits.

## Recommended v1 Build Sequence

1. Rust workspace and contracts.
2. MCP stdio server.
3. SQLite state/journal.
4. JS runner and `ctx`.
5. EVM reads and `Action[]` validation.
6. Simulation and policy.
7. Local signer, broadcast, receipt.
8. Examples/tests/docs.

## Sources

- https://modelcontextprotocol.io/specification/2025-11-25/basic/index
- https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- https://modelcontextprotocol.io/specification/2025-11-25/server/index
- https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices
- https://github.com/modelcontextprotocol/rust-sdk
- https://ts.sdk.modelcontextprotocol.io/
