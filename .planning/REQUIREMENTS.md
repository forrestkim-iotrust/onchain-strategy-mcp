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
- [x] **STR-03**: Runtime can execute a registered strategy with a sandboxed `ctx`. *(03-03 strategy_run MCP tool wires Sandbox::execute + RuntimeContext through 8-step lifecycle.)*
- [x] **STR-04**: Strategy code cannot access private keys, filesystem, process APIs, arbitrary network, or direct RPC clients. *(03-01 strategy-js sandbox: D-03 limits, D-11 forbidden-globals scrub + 8 regression tests, no loader/dyn-load features.)*
- [x] **STR-05**: Strategy returns `Action[]` or `noop`, and runtime rejects unsupported return shapes. *(03-03 validate_strategy_output + STRATEGY_INVALID_OUTPUT -32018; 9 D-08a regression tests cover number/object/null/promise/non-function/phase4-action shapes.)*

### Context API

- [x] **CTX-01**: `ctx.evm.readContract` can perform ABI-based generic contract reads. *(04-01 executor-evm crate + read_contract eth_call lifecycle + ctx.evm.readContract host binding; demonstrable end-to-end via `cargo test -p executor-evm --features anvil-tests --test read_contract_anvil read_counter_number_returns_zero`.)*
- [x] **CTX-02**: `ctx.evm.erc20Balance` can read ERC20 balances. *(04-02 executor_evm::erc20::erc20_balance_of + ctx.evm.readErc20.balanceOf + flat alias ctx.evm.erc20Balance, anvil-gated end-to-end against MockERC20 fixture.)*
- [x] **CTX-03**: `ctx.evm.erc20Allowance` can read ERC20 allowances. *(04-02 executor_evm::erc20::erc20_allowance + ctx.evm.readErc20.allowance + flat alias ctx.evm.erc20Allowance.)*
- [x] **CTX-04**: `ctx.evm.nativeBalance` can read native token balance. *(04-02 executor_evm::native::native_balance + ctx.evm.readNative.balance + flat alias ctx.evm.nativeBalance, decimal-string per D-03.)*
- [x] **CTX-05**: `ctx.actions.contractCall` can create ABI-based contract call actions.
- [x] **CTX-06**: `ctx.actions.rawCall` can create explicit raw calldata actions.
- [x] **CTX-07**: `ctx.actions.erc20Approve` and `ctx.actions.erc20Transfer` can create ERC20 actions.
- [x] **CTX-08**: `ctx.actions.nativeTransfer` can create native transfer actions.
- [x] **CTX-09**: `ctx.units` and address helpers reduce common EVM value/address mistakes. *(04-04 executor_evm::units::{parse_units, format_units} + executor_evm::address::{is_address, checksum, ZERO_ADDRESS} + ctx.units.{parseUnits, formatUnits} + ctx.address.{isAddress, checksum, zeroAddress} sandbox bindings; 22 ctx_units_address tests + 14 lib tests; HR-01 final regression green.)*

### Execution Pipeline

- [x] **EXE-01**: Runtime validates `Action[]` before any simulation or signing.
- [x] **EXE-02**: Runtime ABI-encodes contract call actions into transaction requests.
- [x] **EXE-03**: Runtime simulates transaction requests before signing.
- [x] **EXE-04**: Runtime denies signing when simulation fails.
- [x] **EXE-05**: Runtime applies policy before signing.
- [x] **EXE-06**: Runtime denies signing when policy rejects an action.
- [ ] **EXE-07**: Runtime signs approved transaction requests with a local signer.
- [ ] **EXE-08**: Runtime broadcasts signed transactions to the configured RPC.
- [ ] **EXE-09**: Runtime waits for receipt and records confirmed/failed status.

### Policy

- [x] **POL-01**: Policy can restrict allowed chain IDs.
- [x] **POL-02**: Policy can restrict target contract addresses.
- [x] **POL-03**: Policy can restrict function selectors.
- [x] **POL-04**: Policy can restrict max native value per action.
- [x] **POL-05**: Policy can restrict max ERC20 spend for helper-generated ERC20 actions.
- [x] **POL-06**: Raw calldata actions are denied unless explicitly allowed by policy.

### State and Journal

- [x] **STJ-01**: Runtime persists strategies and strategy metadata locally. *(02-01 schema + repo; 02-02 MCP wiring; 02-02 strategies_persist_across_restart end-to-end stdio test.)*
- [x] **STJ-02**: Runtime persists each strategy run with run ID, strategy ID, started time, and status. *(02-01 base CRUD + ULID + phase2_emittable; 02-03 lifecycle tests + run_roundtrip_insert_get_update_status end-to-end MCP stdio proof + run_status_schema_includes_future_variants D-08a.)*
- [x] **STJ-03**: Runtime records source reads performed during each run. *(03-02 RuntimeContext::flush emits one journal_source_reads row per run with kind="strategy_source"; 03-03 stdio test verifies end-to-end.)*
- [x] **STJ-04**: Runtime records returned actions and validation errors. *(03-03 record_action / record_validation_error / record_runtime_error helpers; one journal_actions row per run with outcome ∈ {noop, actions, validation_error, runtime_error}.)*
- [x] **STJ-05**: Runtime records simulation results and policy decisions.
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
| STR-03 | Phase 3 | Complete (03-03 strategy_run MCP tool — 8-step handler + 19 D-08a stdio tests) |
| STR-04 | Phase 3 | Complete (03-01 strategy-js sandbox + D-11 regression suite) |
| STR-05 | Phase 3 | Complete (03-03 validate_strategy_output + STRATEGY_INVALID_OUTPUT -32018) |
| CTX-01 | Phase 4 | Complete (04-01 executor-evm crate + ctx.evm.readContract host binding) |
| CTX-02 | Phase 4 | Complete (04-02 erc20_balance_of + ctx.evm.readErc20.balanceOf + flat alias erc20Balance) |
| CTX-03 | Phase 4 | Complete (04-02 erc20_allowance + ctx.evm.readErc20.allowance + flat alias erc20Allowance) |
| CTX-04 | Phase 4 | Complete (04-02 native_balance + ctx.evm.readNative.balance + flat alias nativeBalance) |
| CTX-05 | Phase 4 | Closed |
| CTX-06 | Phase 4 | Closed |
| CTX-07 | Phase 4 | Closed |
| CTX-08 | Phase 4 | Closed |
| CTX-09 | Phase 4 | Closed |
| EXE-01 | Phase 5 | Complete |
| EXE-02 | Phase 5 | Complete |
| EXE-03 | Phase 5 | Complete |
| EXE-04 | Phase 5 | Complete (05-05 anvil-backed stdio proof asserts simulation_failure/revert) |
| EXE-05 | Phase 5 | Complete |
| EXE-06 | Phase 5 | Complete (05-05 stdio grid asserts policy denials before signing) |
| EXE-07 | Phase 6 | Pending |
| EXE-08 | Phase 6 | Pending |
| EXE-09 | Phase 6 | Pending |
| POL-01 | Phase 5 | Complete (05-05 stdio rule grid) |
| POL-02 | Phase 5 | Complete (05-05 stdio rule grid) |
| POL-03 | Phase 5 | Complete (05-05 stdio rule grid) |
| POL-04 | Phase 5 | Complete (05-05 stdio rule grid) |
| POL-05 | Phase 5 | Complete (05-05 stdio rule grid) |
| POL-06 | Phase 5 | Complete (05-05 stdio rule grid) |
| STJ-01 | Phase 2 | Complete (02-01 schema + 02-02 strategies_persist_across_restart) |
| STJ-02 | Phase 2 | Complete (02-01 base CRUD + 02-03 lifecycle + run_roundtrip_insert_get_update_status) |
| STJ-03 | Phase 3 | Complete (03-02 RuntimeContext::flush emits journal_source_reads; 03-03 stdio coverage) |
| STJ-04 | Phase 3 | Complete (03-03 record_action / record_validation_error / record_runtime_error → journal_actions) |
| STJ-05 | Phase 5 | Complete (05-05 journal resource assertions for policy/simulation pass, fail, and skipped rows) |
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
