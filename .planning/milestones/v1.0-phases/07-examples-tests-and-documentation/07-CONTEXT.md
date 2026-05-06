# Phase 7: Examples, Tests, and Documentation - Context

**Gathered:** 2026-04-29
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 7 completes the v1 milestone by demonstrating the full onchain strategy loop and adding regression coverage/documentation for the runtime safety contract. It should add examples, tests, and docs only; it should not introduce new runtime product capabilities beyond what is needed to prove and explain the existing loop.

</domain>

<decisions>
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

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- Existing stdio integration harness lives under `crates/executor-mcp/tests/stdio_handshake.rs` with helpers for spawning the MCP binary, writing policy files, and driving JSON-RPC tool/resource calls.
- Existing anvil-gated tests already use local EVM fixtures and policy helpers.
- `crates/executor-core/tests/schema_snapshots.rs` and `tests/schemas/*.json` provide schema golden patterns.
- Current root docs include `README.md`, `AGENTS.md`, Korean docs, and project planning docs.

### Established Patterns
- Tests are Rust integration/unit tests run through package-specific `cargo test` commands.
- Anvil-dependent behavior should be gated behind existing `anvil-tests` feature patterns.
- Runtime guarantees are usually asserted through MCP-visible stdio responses and persisted state/journal/resource reads.
- The project avoids broad product surfaces; v1 is a local MCP runtime.

### Integration Points
- Examples should connect through existing strategy registration/run surfaces, policy config, EVM config, signer config, execution status, and journal resources.
- Safety tests should exercise policy, simulation, and sandbox boundaries without bypassing the public runtime path unless a lower-level unit test is specifically more precise.
- Documentation should align with `.planning/PROJECT.md` and `.planning/REQUIREMENTS.md` v1 scope.

</code_context>

<specifics>
## Specific Ideas

- Cover VER-01 with an ERC20 approve or transfer example against local anvil.
- Cover VER-02 with a generic ABI contract call example using a local fixture.
- Cover VER-03 through policy denial tests for disallowed chains/contracts/selectors.
- Cover VER-04 through a simulation-failure test that proves signing is not reached.
- Cover VER-05 through sandbox forbidden host-access tests.

</specifics>

<deferred>
## Deferred Ideas

- Protocol-specific recipe catalog remains out of scope for v1.
- Dashboard/marketplace docs remain out of scope for v1.
- External signer and detached execution examples remain v2 work.

</deferred>
