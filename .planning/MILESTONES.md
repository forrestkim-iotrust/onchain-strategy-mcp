# Milestones

## v1.0 v1.0 MVP (Shipped: 2026-05-04)

**Phases completed:** 7 phases, 24 plans, 48 tasks

**Key accomplishments:**

- 4-crate Cargo workspace on 2024 edition with rmcp 1.5 deps, 7 JsonSchema tool/prompt input structs in executor-core, and a Wave 0 stdio + schema-snapshot integration harness.
- rmcp 1.5 `ExecutorServer` serves 8 tools over stdio (4 write-capable return structured `unimplemented` errors, 4 read-only return placeholders), config loader + stderr-only tracing wired, and 3 integration tests + 7 schema goldens prove the contract.
- Added the `prompts` (`#[prompt_router]` + 2 placeholders) and `resources` (3 URI templates + always-not-found read) surfaces, attached `#[prompt_handler]` to the same `impl ServerHandler` block as `#[tool_handler]` (Pitfall 6), and sealed the phase gate with 4 new integration tests (resources/prompts contract, stdout-strict JSON-RPC, schema round-trip).
- `schema.rs`
- 1. [Rule 1 - Bug] `collect_enums` walker spec did not match `RunStatus.json` shape
- One-liner:
- One-liner:
- Wired the assembled Phase-3 sandbox + journal infrastructure into the agent-facing `strategy_run` MCP tool, three new wire codes (-32011/-32017/-32018), the live `journal://{run_id}` resource, and a 19-test D-08a stdio integration suite — closing STR-03, STR-05, STJ-04 and Phase 3.
- One-liner:
- One-liner:
- Task 1 — Action enum + per-variant validators (`b709828`)
- One-liner:
- Plan 05-02 (simulate) consumes:
- Plan 05-03 (policy load + eval) consumes:
- Plan 05-04 (orchestrator wiring) consumes:
- 1. [Rule 3 - Blocking] Anvil was required for the final stdio simulation test
- Anvil-backed simulation_failure stdio proof plus durable policy/simulation decision journaling and six-rule policy_violation stdio coverage
- Fail-closed local signer boundary using Alloy PrivateKeySigner plus non-secret MCP signer config parsing
- Receipt-backed sequential local managed execution with per-action SQLite audit rows and Alloy wallet-provider broadcast
- JSON-schema-backed receipt status reports exposed consistently through execution_get and execution://{run_id}
- Runnable local Anvil strategy examples proven through strategy_run, signer broadcast, receipts, and execution_get reports
- MCP-level policy, simulation, and sandbox safety regressions proving unsafe paths stop before signing, tx hash persistence, or host access
- Local runtime documentation now shows the Anvil strategy_run loop, env-var-only hot-wallet signer boundary, and safety verification commands

---
