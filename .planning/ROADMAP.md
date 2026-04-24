# Roadmap: onchain-strategy-mcp

## Overview

v1 builds a local MCP runtime that lets an AI agent write JavaScript EVM automation strategies and execute them end-to-end through a controlled runtime. The roadmap starts with MCP correctness and durable state, then adds sandboxed JS, generic EVM read/write actions, simulation/policy, local signing/broadcasting, and finally examples/tests that prove the core loop works.

## Phases

- [ ] **Phase 1: MCP Runtime Surface** - Create the Rust workspace and stdio MCP server with stable tools/resources/prompts.
- [ ] **Phase 2: Strategy State and Journal** - Persist strategies, runs, and journal records locally.
- [ ] **Phase 3: JavaScript Strategy Runner** - Run sandboxed JavaScript strategies with a constrained `ctx`.
- [ ] **Phase 4: EVM Context and Actions** - Implement generic EVM reads/writes plus ERC20/native helpers.
- [ ] **Phase 5: Simulation and Policy Gate** - Validate actions, simulate, and enforce policy before signing.
- [ ] **Phase 6: Local Managed Execution** - Sign locally, broadcast transactions, wait receipts, and record execution reports.
- [ ] **Phase 7: Examples, Tests, and Documentation** - Prove the runtime with local EVM examples and verification tests.

## Phase Details

### Phase 1: MCP Runtime Surface
**Goal**: A stdio MCP server boots cleanly and exposes the initial runtime contract.  
**Depends on**: Nothing  
**Requirements**: MCP-01, MCP-02, MCP-03, MCP-04  
**Success Criteria**:
1. MCP client can initialize the server over stdio.
2. Tool list exposes strategy, execution, and policy groups with JSON schemas.
3. Resource list exposes strategy, execution, and journal URI shapes.
4. Prompt list exposes strategy authoring/review prompts.
**Plans**: 3 plans

Plans:
- [x] 01-01: Rust workspace and crate skeleton
- [ ] 01-02: MCP stdio server and tool schema wiring
- [ ] 01-03: MCP resources/prompts and stdout/stderr discipline checks

### Phase 2: Strategy State and Journal
**Goal**: Runtime can persist strategies, runs, metadata, and journal records.  
**Depends on**: Phase 1  
**Requirements**: STR-01, STR-02, STJ-01, STJ-02  
**Success Criteria**:
1. Agent can register, list, inspect, and delete strategies.
2. Strategy source and metadata persist across server restarts.
3. Each run gets a durable run ID and status row.
**Plans**: 3 plans

Plans:
- [ ] 02-01: SQLite schema and repository layer
- [ ] 02-02: Strategy management tools
- [ ] 02-03: Run and journal base model

### Phase 3: JavaScript Strategy Runner
**Goal**: Runtime executes sandboxed JavaScript strategies and accepts only valid `Action[]`/`noop` outputs.  
**Depends on**: Phase 2  
**Requirements**: STR-03, STR-04, STR-05, STJ-03, STJ-04  
**Success Criteria**:
1. Agent can run a registered JS strategy once.
2. Forbidden host access is blocked.
3. Source reads and returned actions/errors are journaled.
4. Invalid return shapes are rejected with actionable MCP tool errors.
**Plans**: 3 plans

Plans:
- [ ] 03-01: QuickJS sandbox and runtime limits
- [ ] 03-02: Minimal `ctx` host API and source-read capture
- [ ] 03-03: Action output validation and journal integration

### Phase 4: EVM Context and Actions
**Goal**: Strategy code can express broad EVM reads and write actions through `ctx`.  
**Depends on**: Phase 3  
**Requirements**: CTX-01, CTX-02, CTX-03, CTX-04, CTX-05, CTX-06, CTX-07, CTX-08, CTX-09  
**Success Criteria**:
1. `ctx.evm.readContract` reads arbitrary ABI-compatible contract methods.
2. ERC20/native read helpers work against a local EVM.
3. `contractCall`, `rawCall`, ERC20, and native actions produce validated `Action[]`.
4. Unit/address helpers reduce common amount/address errors.
**Plans**: 4 plans

Plans:
- [ ] 04-01: Alloy provider and ABI read adapter
- [ ] 04-02: ERC20/native read helpers
- [ ] 04-03: Contract call, raw call, ERC20, and native action builders
- [ ] 04-04: Units/address helpers and action validation fixtures

### Phase 5: Simulation and Policy Gate
**Goal**: No transaction can reach the signer before simulation and policy approval.  
**Depends on**: Phase 4  
**Requirements**: EXE-01, EXE-02, EXE-03, EXE-04, EXE-05, EXE-06, POL-01, POL-02, POL-03, POL-04, POL-05, POL-06, STJ-05  
**Success Criteria**:
1. Runtime ABI-encodes actions into transaction requests.
2. Simulation failures stop execution before signing.
3. Policy failures stop execution before signing.
4. Policy supports chain, contract, selector, native value, ERC20 spend, and raw calldata restrictions.
5. Simulation and policy decisions are journaled.
**Plans**: 4 plans

Plans:
- [ ] 05-01: Action-to-transaction normalization
- [ ] 05-02: Simulation adapter and failure handling
- [ ] 05-03: Policy model and deny-by-default evaluator
- [ ] 05-04: Journaled decisions and MCP error reporting

### Phase 6: Local Managed Execution
**Goal**: Approved actions execute on-chain through a local signer and produce receipt-backed reports.  
**Depends on**: Phase 5  
**Requirements**: EXE-07, EXE-08, EXE-09, STJ-06, STJ-07  
**Success Criteria**:
1. Runtime signs approved transaction requests with a local signer.
2. Runtime broadcasts signed transactions to configured RPC.
3. Runtime waits for receipts and records confirmed/failed status.
4. Agent can query execution status by ID.
**Plans**: 3 plans

Plans:
- [ ] 06-01: Signer boundary and local private-key signer
- [ ] 06-02: Broadcast and receipt watcher
- [ ] 06-03: Execution report and status tools

### Phase 7: Examples, Tests, and Documentation
**Goal**: The repo demonstrates the full runtime loop and has enough tests to prevent unsafe regressions.  
**Depends on**: Phase 6  
**Requirements**: VER-01, VER-02, VER-03, VER-04, VER-05  
**Success Criteria**:
1. Local EVM example executes an ERC20 approve or transfer.
2. Generic ABI contract call example executes successfully.
3. Tests prove policy blocks disallowed actions.
4. Tests prove simulation failure prevents signing.
5. Tests prove JS sandbox blocks forbidden host access.
**Plans**: 3 plans

Plans:
- [ ] 07-01: Local EVM fixtures and example strategies
- [ ] 07-02: Safety and policy test suite
- [ ] 07-03: README, AGENTS, and usage docs refresh

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. MCP Runtime Surface | 1/3 | In progress | - |
| 2. Strategy State and Journal | 0/3 | Not started | - |
| 3. JavaScript Strategy Runner | 0/3 | Not started | - |
| 4. EVM Context and Actions | 0/4 | Not started | - |
| 5. Simulation and Policy Gate | 0/4 | Not started | - |
| 6. Local Managed Execution | 0/3 | Not started | - |
| 7. Examples, Tests, and Documentation | 0/3 | Not started | - |
