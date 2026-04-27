# Requirements: onchain-strategy-mcp

**Defined:** 2026-04-24  
**Core Value:** AI agent가 EVM 자동화 로직을 실제 온체인 실행으로 바꾸되, 모든 실행은 policy 검사를 거치고 기록으로 남아야 한다.

## v1 Requirements

### MCP Runtime

- [x] **MCP-01**: Server can run as a stdio MCP server without writing non-MCP data to stdout.
- [x] **MCP-02**: Server exposes JSON-schema-backed tools for strategy, execution, and policy operations.
- [x] **MCP-03**: Server exposes resources for strategy details, execution reports, and journal entries.
- [x] **MCP-04**: Server exposes prompts for writing and reviewing EVM automation strategies.

### Strategy Runtime

- [x] **STR-01**: Agent can register a JavaScript strategy with name, source, and metadata. *(Phase 2-01 storage; 02-02 MCP wiring + stdio tests; 02-03 Phase 2 close.)*
- [x] **STR-02
**: Agent can list, inspect, and delete registered strategies.
- [ ] **STR-03**: Runtime can execute a registered strategy with a sandboxed `ctx`.
- [ ] **STR-04**: Strategy code cannot access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients.
- [ ] **STR-05**: Strategy returns `Action[]` or `noop`, and runtime rejects unsupported return shapes.

### Context API

- [ ] **CTX-01**: `ctx.evm.readContract` can perform ABI-based generic contract reads.
- [ ] **CTX-02**: `ctx.evm.erc20Balance` can read ERC20 balances.
- [ ] **CTX-03**: `ctx.evm.erc20Allowance` can read ERC20 allowances.
- [ ] **CTX-04**: `ctx.evm.nativeBalance` can read native token balance.
- [ ] **CTX-05**: `ctx.actions.contractCall` can create ABI-based contract call actions.
- [ ] **CTX-06**: `ctx.actions.rawCall` can create explicit raw calldata actions.
- [ ] **CTX-07**: `ctx.actions.erc20Approve` and `ctx.actions.erc20Transfer` can create ERC20 actions.
- [ ] **CTX-08**: `ctx.actions.nativeTransfer` can create native transfer actions.
- [ ] **CTX-09**: `ctx.units` and address helpers reduce common EVM value/address mistakes.

### Execution Pipeline

- [ ] **EXE-01**: Runtime validates `Action[]` before any simulation or signing.
- [ ] **EXE-02**: Runtime ABI-encodes contract call actions into transaction requests.
- [ ] **EXE-03**: Runtime simulates transaction requests before signing.
- [ ] **EXE-04**: Runtime denies signing when simulation fails.
- [ ] **EXE-05**: Runtime applies policy before signing.
- [ ] **EXE-06**: Runtime denies signing when policy rejects an action.
- [ ] **EXE-07**: Runtime signs approved transaction requests with a local signer.
- [ ] **EXE-08**: Runtime broadcasts signed transactions to the configured RPC.
- [ ] **EXE-09**: Runtime waits for receipt and records confirmed/failed status.

### Policy

- [ ] **POL-01**: Policy can restrict allowed chain IDs.
- [ ] **POL-02**: Policy can restrict target contract addresses.
- [ ] **POL-03**: Policy can restrict function selectors.
- [ ] **POL-04**: Policy can restrict max native value per action.
- [ ] **POL-05**: Policy can restrict max ERC20 spend for helper-generated ERC20 actions.
- [ ] **POL-06**: Raw calldata actions are denied unless explicitly allowed by policy.

### State and Journal

- [x] **STJ-01**: Runtime persists strategies and strategy metadata locally. *(02-01 schema + repo; 02-02 MCP wiring; 02-02 strategies_persist_across_restart end-to-end stdio test.)*
- [x] **STJ-02**: Runtime persists each strategy run with run ID, strategy ID, started time, and status. *(02-01 base CRUD + ULID + phase2_emittable; 02-03 lifecycle tests + run_roundtrip_insert_get_update_status end-to-end MCP stdio proof + run_status_schema_includes_future_variants D-08a.)*
- [ ] **STJ-03**: Runtime records source reads performed during each run.
- [ ] **STJ-04**: Runtime records returned actions and validation errors.
- [ ] **STJ-05**: Runtime records simulation results and policy decisions.
- [ ] **STJ-06**: Runtime records tx hash, receipt status, gas used, and execution errors.
- [ ] **STJ-07**: Agent can query execution status by execution/run ID.

### Examples and Verification

- [ ] **VER-01**: Repository includes a local EVM/anvil example for ERC20 transfer or approve.
- [ ] **VER-02**: Repository includes a generic contract call example using ABI.
- [ ] **VER-03**: Tests prove policy prevents disallowed chains/contracts/selectors.
- [ ] **VER-04**: Tests prove failed simulation prevents signing.
- [ ] **VER-05**: Tests prove strategy sandbox blocks forbidden host access.

## v2 Requirements

### Deferred Capabilities

- **V2-01**: TypeScript authoring support through optional type definitions or transpilation.
- **V2-02**: External signer adapters.
- **V2-03**: Detached execution protocol.
- **V2-04**: Scheduler and long-running reconcile loops.
- **V2-05**: Capability registry or package format.
- **V2-06**: Streamable HTTP transport with authorization.
- **V2-07**: Multi-account and multi-tenant management.
- **V2-08**: Visual dashboard or strategy graph UI.

## Out of Scope

| Feature | Reason |
|---------|--------|
| TypeScript compiler in v1 | Adds module/transpile complexity before runtime loop is proven. |
| Custom DSL/opcode VM | Plain JS plus `ctx` is enough for v1 and easier for agents. |
| Dashboard/landing page | Runtime is the product surface through MCP. |
| Hosted custody | v1 local signer is a local hot-wallet runtime, not a wallet product. |
| Protocol recipe catalog | Generic EVM reads/calls provide breadth without core bloat. |
| Scheduler in v1 | Explicit run/register/status must work first. |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| MCP-01 | Phase 1 | Complete (01-01 scaffold, 01-02 stderr-only tracing, 01-03 stdout_is_strict_jsonrpc test) |
| MCP-02 | Phase 1 | Complete (01-02 ExecutorServer + 8 tool handlers + schema goldens) |
| MCP-03 | Phase 1 | Complete (01-03 resources: 3 URI templates + -32002 not_found) |
| MCP-04 | Phase 1 | Complete (01-03 prompts: 2 placeholder prompts with arg schemas) |
| STR-01 | Phase 2 | Complete (02-01 storage + 02-02 MCP wiring + stdio tests) |
| STR-02 | Phase 2 | Complete (02-02 strategy_list/get/delete tools + 14 stdio tests) |
| STR-03 | Phase 3 | Pending |
| STR-04 | Phase 3 | Pending |
| STR-05 | Phase 3 | Pending |
| CTX-01 | Phase 4 | Pending |
| CTX-02 | Phase 4 | Pending |
| CTX-03 | Phase 4 | Pending |
| CTX-04 | Phase 4 | Pending |
| CTX-05 | Phase 4 | Pending |
| CTX-06 | Phase 4 | Pending |
| CTX-07 | Phase 4 | Pending |
| CTX-08 | Phase 4 | Pending |
| CTX-09 | Phase 4 | Pending |
| EXE-01 | Phase 5 | Pending |
| EXE-02 | Phase 5 | Pending |
| EXE-03 | Phase 5 | Pending |
| EXE-04 | Phase 5 | Pending |
| EXE-05 | Phase 5 | Pending |
| EXE-06 | Phase 5 | Pending |
| EXE-07 | Phase 6 | Pending |
| EXE-08 | Phase 6 | Pending |
| EXE-09 | Phase 6 | Pending |
| POL-01 | Phase 5 | Pending |
| POL-02 | Phase 5 | Pending |
| POL-03 | Phase 5 | Pending |
| POL-04 | Phase 5 | Pending |
| POL-05 | Phase 5 | Pending |
| POL-06 | Phase 5 | Pending |
| STJ-01 | Phase 2 | Complete (02-01 schema + 02-02 strategies_persist_across_restart) |
| STJ-02 | Phase 2 | Complete (02-01 base CRUD + 02-03 lifecycle + run_roundtrip_insert_get_update_status) |
| STJ-03 | Phase 3 | Pending |
| STJ-04 | Phase 3 | Pending |
| STJ-05 | Phase 5 | Pending |
| STJ-06 | Phase 6 | Pending |
| STJ-07 | Phase 6 | Pending |
| VER-01 | Phase 7 | Pending |
| VER-02 | Phase 7 | Pending |
| VER-03 | Phase 7 | Pending |
| VER-04 | Phase 7 | Pending |
| VER-05 | Phase 7 | Pending |

**Coverage:**
- v1 requirements: 45 total
- Mapped to phases: 45
- Unmapped: 0

---
*Requirements defined: 2026-04-24*
*Last updated: 2026-04-24 after initial definition*
