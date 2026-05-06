# Pitfalls Research

**Domain:** MCP runtime for EVM automation  
**Researched:** 2026-04-24  
**Confidence:** Medium-high

## Pitfalls

| Pitfall | Warning Sign | Prevention |
|---------|--------------|------------|
| Overbuilding before first run | TypeScript, DAG, scheduler, external signer appear before run_once works. | Keep v1 to JS + `ctx` + `Action[]` + local execution. |
| Overbroad MCP tools | A single tool accepts arbitrary JS/calldata and executes immediately. | Separate strategy registration, run, simulation/policy, and execution report. |
| Stdio logging bug | Server logs to stdout and breaks JSON-RPC. | All logs to stderr through `tracing`; stdout only MCP messages. |
| JS host escape | Strategy can call filesystem/network/process/private key. | Expose only explicit `ctx` host functions. |
| Signing before policy | Agent-generated tx reaches signer unchecked. | Enforce lifecycle in core, not just docs. |
| Raw calldata bypasses policy | `rawCall` makes selector/value/spend invisible. | Decode selector when possible; require strict target/value/selector allowlist. |
| App-specific core creep | Aave/Uniswap/etc. helpers enter core too early. | Keep core generic; ERC20/native helpers only. |
| Local key treated like product custody | Users think runtime safely manages long-term assets. | Document local hot-wallet assumption and require limits. |
| Missing journal fields | Failed runs cannot be explained. | Persist source reads, action, simulation result, policy decision, tx hash, receipt/error. |
| Mainnet accident | Example strategy sends real funds. | Default to local/testnet; require explicit chain allowlist and dry-run examples. |

## Security Notes

- MCP tools are model-controlled. Write-capable tools need clear policy boundaries.
- Human-in-the-loop UX is recommended by the MCP tool spec; in this repo, policy/dry-run/journal are the runtime safety layer.
- For stdio, stdout must contain only MCP messages.
- HTTP transport should be deferred until authorization and session risks are addressed.

## Sources

- MCP Tools: https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- MCP Transports: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Security Best Practices: https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices

---
*Pitfalls research for: onchain-strategy-mcp*
*Researched: 2026-04-24*
