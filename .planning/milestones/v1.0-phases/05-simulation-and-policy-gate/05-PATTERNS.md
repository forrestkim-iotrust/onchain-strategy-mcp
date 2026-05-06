---
phase: 05
artifact: PATTERNS
status: complete
mapped: 2026-04-27
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md           # EXE-01..06 + POL-01..06 + STJ-05
  - .planning/phases/04-evm-context-and-actions/04-CONTEXT.md
  - .planning/phases/04-evm-context-and-actions/04-REVIEW-FIX.md
  - .planning/phases/03-javascript-strategy-runner/03-REVIEW-FIX.md   # HR-01 / MR-01 / MR-03 / MR-04 carry-forward
  - AGENTS.md                            # executor-policy crate boundary (line 35)
consumed_by:
  - gsd-planner (05-PLAN.md per-plan analog references)
---

# Phase 5: Simulation and Policy Gate — Pattern Map

**Files analyzed:** ~13 new + 4 modified
**Analogs found:** 13 / 13 (executor-policy is greenfield, but every component has a strong analog in Phase 3 / Phase 4 code)

---

## Crate Layout Recommendation — `executor-policy` as a NEW crate

**Decision:** Create a new workspace member `crates/executor-policy/`. Do **NOT** extend `executor-evm`.

**Rationale (locks four pre-existing constraints):**

1. **AGENTS.md line 35** explicitly lists `executor-policy/` as a target crate boundary — Phase 5 is the first opportunity to honour it.
2. **Boundary clarity** — `executor-evm` is read-focused (`Provider::call`, ABI decode, no signing intent). Mixing in policy DSL evaluation + Action→TxRequest normalization would muddy the responsibility line.
3. **Dep graph hygiene** — `executor-policy` will depend on `executor-evm` (for `Provider`, `validate_address`, `dry_run_abi_encode`, `EvmError`). The reverse dep would be wrong: `executor-evm` must never know about policy verdicts.
4. **Workspace dep precedent** — Phase 4 D-01 explicitly anticipated this: *"Phase 5 will likely add `executor-mcp` as a second consumer for TransactionRequest construction, at which point alloy promotion happens."* Phase 5 is the moment to (a) add `executor-policy` as a new alloy consumer and (b) decide whether to promote alloy 2.0 to `[workspace.dependencies]`. Recommendation: **promote alloy** now (3 consumers: executor-evm, executor-policy, executor-mcp via TransactionRequest construction in tools.rs handler) — this is the workspace promotion threshold the comment in `crates/executor-evm/Cargo.toml:11-14` predicted.

**Module layout (mirrors `executor-evm/src/` shape):**

```
crates/executor-policy/
  Cargo.toml
  src/
    lib.rs            # re-export pattern: re-exports Decision, PolicyVerdict, etc.
    error.rs          # PolicyError + SimulationError (Display = wire-safe taxonomy strings; detail_for_log for tracing)
    config.rs         # PolicyConfig::from_raw(toml::Value) -> Result<_, PolicyError>; defaults locked here
    normalize.rs      # Action -> TxRequest (ABI encode + value + selector resolution)
    simulate.rs       # SimulationAdapter::simulate(provider, cfg, tx) -> Result<SimulationOutcome, _>
    policy.rs         # PolicyEvaluator::evaluate(&Decision, &PolicyConfig) -> PolicyVerdict
    selector.rs       # 4-byte selector extraction from calldata (POL-03)
  tests/
    common/
      mod.rs           # mirrors crates/executor-evm/tests/common/mod.rs
      anvil_fixture.rs # consumed via executor-evm = { features = ["test-fixtures"] }
      fixtures/
        revert_counter.hex    # contract that always reverts (sim-fail test)
    normalize.rs              # unit (no anvil)
    policy_evaluator.rs       # unit (no anvil)
    simulate_anvil.rs         # anvil-gated; mirrors read_contract_anvil.rs
```

---

## Existing-Code Analogs Table

| New / Modified File | Closest Existing Analog | Key Convention to Mirror |
|---------------------|-------------------------|--------------------------|
| `crates/executor-policy/Cargo.toml` | `crates/executor-evm/Cargo.toml:11-44` | Per-crate dep pinning + `[features] test-fixtures` block; `[dev-dependencies] tokio = { workspace = true, features = ["rt", "macros"] }`; cite Phase 4 comment block. |
| `crates/executor-policy/src/lib.rs` | `crates/executor-evm/src/lib.rs:16-46` | `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]` + module declarations + `pub use` re-export pattern (`pub use config::PolicyConfig; pub use policy::{Decision, PolicyVerdict};`). |
| `crates/executor-policy/src/error.rs` | `crates/executor-evm/src/error.rs:11-83` | `thiserror::Error` enum with `Display` returning ONLY stable taxonomy prefixes (e.g. `"policy violation: chain_denied"`); `detail_for_log: String` per variant; `data_kind() -> &'static str` dispatcher; **unit test** asserting `Display` is wire-safe (no raw addresses / selectors / numeric leaks beyond stable taxonomy). |
| `crates/executor-policy/src/config.rs` | `crates/executor-evm/src/config.rs:1-50` | `Default` impl returning empty/permissive policy; `from_raw(...)` validates and returns `PolicyError::Config` on parse failure; range checks for any numeric caps mirror `call_timeout_ms in 50..=30_000` style. |
| `crates/executor-policy/src/normalize.rs` | `crates/executor-evm/src/action.rs:156-208` (`dry_run_abi_encode`) + `crates/executor-evm/src/read.rs:78-117` (TransactionRequest assembly) | Re-uses `validate_address` / `validate_calldata` / `validate_decimal_amount` / `validate_abi_size` from `executor_evm::action::*`. Phase 4 *discards* the encoded bytes; Phase 5 *keeps* them. Output type: `TxRequest { to: Address, data: Bytes, value: U256, chain_id: Option<u64> }`. |
| `crates/executor-policy/src/simulate.rs` | `crates/executor-evm/src/read.rs:73-147` (`read_contract`) | `tokio::time::timeout(cfg.call_timeout, provider.call(tx).block(BlockTag::Latest.to_block_id()))`; reuse `classify_provider_error` revert/transport heuristic at line 186-212. **Re-export `sanitize_revert_reason` from executor-evm** (currently `pub(crate)` — promote to `pub` in 04 follow-up OR copy locally) so simulator-side revert reasons get the WR-04 sanitization treatment. |
| `crates/executor-policy/src/policy.rs` | `crates/executor-mcp/src/validation.rs:79-95` (`validate_action_kind_allowlisted`) | Allowlist-based deny-by-default. Returns stable error category strings (`"chain_denied"`, `"target_denied"`, `"selector_denied"`, `"native_value_cap_exceeded"`, `"erc20_spend_cap_exceeded"`, `"raw_calldata_denied"`). Each maps to a POL-NN requirement. |
| `crates/executor-policy/src/selector.rs` | `crates/executor-evm/src/action.rs:77-98` (`validate_calldata`) | Hex parse + first-4-bytes extraction. Reuses `Bytes::from_str` for hex. |
| `crates/executor-policy/tests/simulate_anvil.rs` | `crates/executor-evm/tests/read_contract_anvil.rs:1-233` | `#![cfg(feature = "anvil-tests")]`; `mod common;`; `let Some(fixture) = AnvilFixture::try_spawn() else { return };`; `eprintln!` skip pattern; `#[tokio::test(flavor = "multi_thread")]`; deploy-via-bytecode helper. Test names: `simulate_increment_counter_passes`, `simulate_revert_returns_simulation_failure`. |
| `crates/executor-policy/tests/policy_evaluator.rs` | `crates/executor-evm/src/action.rs::tests` (lines 210-385) | Pure-function unit tests, no anvil, table-driven `cat_of` extractor pattern; per-rule `<scenario>_<expected>` naming. |
| `crates/executor-policy/tests/normalize.rs` | `crates/executor-evm/src/action.rs::tests` | Same; covers all 5 Action variants → TxRequest with golden bytes assertion for one stable case (e.g. ERC20 transfer). |
| **MODIFIED:** `crates/executor-state/src/schema.rs` (lines 36-78) | `journal_logs` (lines 69-78) **AND** `journal_source_reads` (lines 39-50) — both already carry `seq INTEGER NOT NULL, UNIQUE (run_id, seq)` (D-15d / MR-04 carry-forward) | Add `CREATE TABLE IF NOT EXISTS journal_decisions (id TEXT PK, run_id TEXT NOT NULL REFERENCES runs(id), action_index INTEGER NOT NULL, gate TEXT NOT NULL, verdict TEXT NOT NULL, reason TEXT NOT NULL, payload_json TEXT, recorded_at TEXT NOT NULL, seq INTEGER NOT NULL, UNIQUE (run_id, seq))`. **Index on `(run_id)`** like `idx_journal_logs_run_id`. |
| **MODIFIED:** `crates/executor-state/src/journal.rs` | `record_log` (lines 169-183) for the seq-assignment, `record_source_read` (lines 96-112) for kind/target/payload pattern, `record_action_outcome` (lines 135-155) for the `phase_emittable` gate | New `record_decision(conn, run_id, action_index, gate, verdict, reason, payload_json) -> Result<String, StateError>` + `next_decision_seq` helper (clone of `next_log_seq` lines 160-167) + `__test_record_decision_with_time` doc-hidden variant + `list_decisions_for_run`. Wire-string converters mirror `outcome_to_wire` / `outcome_from_wire` (lines 56-81). |
| **MODIFIED:** `crates/executor-state/src/store.rs` | `record_action_outcome` façade (lines 154-161) + `record_log` façade (lines 163-165) + `__test_record_log_with_time` (lines 169-177) | Add `StateStore::record_decision`, `StateStore::list_decisions_for_run`, and a `#[doc(hidden)] __test_record_decision_with_time` mirror. |
| **MODIFIED:** `crates/executor-state/src/lib.rs:12-23` | Existing `pub use` block | Add `pub use journal::DecisionEntry;`. |
| **MODIFIED:** `crates/executor-core/src/schema/execution.rs` | `JournalActionOutcome` (lines 58-82) — Phase 5 variants `SimulationFailure` / `PolicyDenied` are ALREADY locked there | (a) Promote `RunStatus::SimulationDenied` / `PolicyDenied` from "phase 5 reserved" to **emittable** by widening `phase2_emittable` → splitting into a new `phase5_emittable` gate, or by replacing the gate semantics. The transition guard at `runs::update_run_status_with_transition` (lines 152-196) already routes through `phase2_emittable` — adjust to also accept the Phase 5 variants. (b) Add new `Decision` schema struct + `SimulationOutcome` + `PolicyVerdict` enums (all `JsonSchema, Serialize, Deserialize, schemars`). Mirror `StrategyOutcome` shape (lines 87-93) — tag = "kind" / rename_all = "snake_case". Wire-locked at Phase 5 introduction so future gates can extend without churn. |
| **MODIFIED:** `crates/executor-core/src/schema/strategy.rs::StrategyRunResponse` (file `crates/executor-core/src/schema/execution.rs:95-110`) | Existing `StrategyRunResponse` | Add `#[serde(default)] pub decisions: Vec<Decision>` field. **Schema-golden REGEN required** (`StrategyRunResponse.json`, `Decision.json`, `PolicyVerdict.json`, `SimulationOutcome.json`). |
| **MODIFIED:** `crates/executor-core/src/schema/policy.rs` (currently 574B stub) | The 12-line stub already exists for `policy_update` placeholder | Replace stub with: real `PolicyConfig` schema (chains, targets, selectors, native_value_caps, erc20_spend_caps, raw_calldata_allow). Mirror `StrategyRegisterInput` (`schema/strategy.rs`) `deny_unknown_fields` + per-field `#[schemars(description = ...)]`. |
| **MODIFIED:** `crates/executor-mcp/src/config.rs` | `EvmSection` (lines 70-94) + `Config::evm_config()` (line 100-102) | Add `#[serde(default)] pub policy: PolicyFileSection` to `Config`; new `PolicyFileSection { path: Option<String> }` with `#[serde(deny_unknown_fields)]` + `Config::policy_config(&self) -> Result<PolicyConfig, PolicyError>` that reads + parses the referenced TOML fixture. **Test invariant carry-forward:** `rejects_unknown_top_level_fields` (line 187) currently uses `[policy]` as the canary unknown — UPDATE the test to use a different canary section (e.g. `[bogus]`) since `[policy]` becomes legal in Phase 5. |
| **MODIFIED:** `crates/executor-mcp/src/errors.rs` | `map_evm_error` (lines 131-148) — perfect template — and `STRATEGY_RUNTIME_ERROR` (-32017) wire code | Add NEW wire codes (next available -32019 / -32020): `SIMULATION_FAILED = -32019`, `POLICY_VIOLATION = -32020`. Add `map_simulation_error(e, run_id)` mirror of `map_evm_error` and `map_policy_error(verdict, run_id)` mirror that emits `data.code = "simulation_failed"` / `"policy_violation"` + `data.kind = e.data_kind()` + raw to `tracing::warn!`. **HR-01/MR-01 carry-forward:** raw revert text (sanitized via WR-04) is the only attacker-controlled string permitted on the wire. |
| **MODIFIED:** `crates/executor-mcp/src/tools.rs::strategy_run` (lines 232-371) | The 8-step lifecycle handler | **Insert NEW Steps 6.5 (Normalize+Policy) and 6.6 (Simulate) BETWEEN current step 6 (validate) and step 7 (transition Running→Succeeded).** Order is **policy first, then simulate** (Requirements EXE-05/06 require deny-before-sign; cheap rule check before RPC). On policy failure → `record_decision(gate="policy", verdict="fail")` → `transition(Running, PolicyDenied)` → return `map_policy_error`. On sim failure → `record_decision(gate="simulation", verdict="fail")` → `transition(Running, SimulationDenied)` → return `map_simulation_error`. Both use the existing `transition` helper at line 508-523. **Concurrency carry-forward:** sim's `provider.call` is called via `tokio::runtime::Handle::current().block_on(...)` from inside the `spawn_blocking` closure — DO NOT wrap in `block_in_place` (WR-01 fix at sandbox.rs:1130/1141). Action[] iteration MUST drop the storage mutex BEFORE every `block_on` (D-04 mutex discipline). |
| **MODIFIED:** `crates/executor-mcp/src/tools.rs::policy_update` + `policy_get` (lines 373-398) | Existing handlers | Replace `unimplemented_err("policy_update", 5)` with real implementation: parse + validate `PolicyUpdateInput`, write to in-memory `Arc<RwLock<PolicyConfig>>` on `ExecutorServer`. Update `policy_get` to return the live config (drop placeholder JSON). |
| **MODIFIED:** `crates/executor-mcp/src/server.rs::ExecutorServer` (lines 39-50) | `evm_config: EvmConfig` and `evm_provider: OnceCell` fields | Add `policy: Arc<RwLock<executor_policy::PolicyConfig>>` field. Construct in `new_with_config`/`from_config`. Carry through to `tools.rs::strategy_run` via `self.policy.read().await.clone()` BEFORE `spawn_blocking` (no holding RwLock across `block_on`). |
| **MODIFIED:** `crates/executor-mcp/src/resources.rs::read_journal` (lines 167-243) | Existing journal:// resource shape | Add `decisions: [...]` field to the JSON body. Mirror the `actions` row pattern at lines 210-220 — `serde_json::to_value(d.verdict)` for canonical snake_case (NEVER `format!("{:?}",..)`). |
| **MODIFIED:** `crates/executor-mcp/src/tools.rs::validate_strategy_output` (lines 437-495) | The Phase-4 BR-02 carry-forward (lines 477-487) — abi-cap-at-output-gate | **Add a new Action[] length cap** (recommend 32; mirrors `MAX_TAGS = 16` philosophy at `validation.rs:11`). Phase 5 size-cap-at-gate is the BR-02 carry-forward generalised. Cap any new policy-file-size or raw-calldata-length limits at this same gate, NOT only at the constructor. |
| **MODIFIED:** `crates/executor-state/src/error.rs` | Existing `StateError` (lines 4-29) | Likely no new variant — `StateError::Storage` covers `journal_decisions` insert failures. If a Phase-5-specific kind emerges, follow the `SerializationError(String)` pattern at line 27-29 (MR-03 — separate variant for journal-payload serde failures so the wire surfaces a stable taxonomy string). |

---

## Workspace Dependency Conventions

| Concern | Convention | Source / Precedent |
|---------|------------|--------------------|
| **alloy promotion** | **Recommended:** promote `alloy = { version = "2.0", default-features = false, features = [...] }` to `[workspace.dependencies]` IF executor-policy + executor-evm + executor-mcp all consume it (3-consumer threshold). | `crates/executor-evm/Cargo.toml:11-14` comment block; Phase 4 D-01 forecast. |
| **toml** | Already at `[workspace.dependencies] toml = "0.8"` (root `Cargo.toml:20`). Reuse via `toml = { workspace = true }` in `executor-policy/Cargo.toml`. | Phase 1 logging precedent. |
| **rusqlite / sha2 / ulid / chrono** | DO NOT promote to workspace. Stay pinned in `executor-state/Cargo.toml:14-21`. Phase 5 only TOUCHES the schema, not the storage stack. | `crates/executor-state/Cargo.toml:11-14` comment. |
| **executor-policy → executor-evm dep** | `executor-evm = { path = "../executor-evm" }` exact form mirrors `executor-mcp/Cargo.toml:31`. Reuses `DynProvider` re-export, `EvmConfig`, `EvmError`, `validate_*`, `dry_run_abi_encode`, `sanitize_revert_reason`. Confirms D-02 isolation rule transitively. | `crates/executor-mcp/Cargo.toml:25-31`. |
| **anvil-tests gating** | `[features] anvil-tests = []` flag; `executor-policy = { path = "...", features = ["test-fixtures"] }` to access `executor-evm`'s `tests/common/anvil_fixture.rs`. | Phase 4 D-14, `crates/executor-evm/Cargo.toml:38-40`. |

---

## Test Harness Conventions

### Anvil-gated simulation tests (`crates/executor-policy/tests/simulate_anvil.rs`)

- **Skip-cleanly contract** (D-14 carry-forward): `let Some(fixture) = AnvilFixture::try_spawn() else { return };` — **never panic** on missing anvil. Source: `crates/executor-evm/tests/read_contract_anvil.rs:64-68`, `common/anvil_fixture.rs:23-53`.
- **Bytecode fixture commit pattern**: `revert_counter.hex` committed under `tests/fixtures/`. Deploy helper mirrors `deploy_counter` at `read_contract_anvil.rs:33-61` (strip `0x`, hex-decode, `with_deploy_code`, `pending.get_receipt().await`).
- **Test runtime**: `#[tokio::test(flavor = "multi_thread")]` (line 63 / 91 / 134 / 163 in read_contract_anvil.rs) — required because alloy's HTTP transport uses tokio's multi-thread reactor.
- **Test naming**: `<scenario>_<expected>` snake_case + `_anvil` not in name (file suffix carries it). Examples mirror Phase 4: `simulate_pure_view_call_passes`, `simulate_revert_returns_simulation_failure`, `simulate_oog_returns_simulation_failure`, `simulate_unreachable_rpc_returns_evm_rpc_error`.

### Mock policy unit tests (`crates/executor-policy/tests/policy_evaluator.rs`)

- **No anvil**, no async — pure-function. Pattern mirrors `crates/executor-evm/src/action.rs::tests` (lines 210-385).
- **Per-rule coverage**: one test per POL-NN requirement (`policy_rejects_disallowed_chain_id` POL-01, `policy_rejects_disallowed_target` POL-02, `policy_rejects_disallowed_selector` POL-03, `policy_rejects_native_value_above_cap` POL-04, `policy_rejects_erc20_spend_above_cap` POL-05, `policy_rejects_raw_call_when_not_allowlisted` POL-06).
- **Stable error category extractor**: `cat_of(&PolicyError) -> &str` helper mirrors `cat_of` at `action.rs:215-222`.

### Stdio integration tests (extend `crates/executor-mcp/tests/stdio_handshake.rs`)

- **Naming convention** (file lines 30, 95, 999, 1119, etc.): `<subject>_<verb_phrase>_<expected>`. Phase 5 additions:
  - `strategy_run_emits_decisions_when_actions_pass_policy_and_simulation`
  - `strategy_run_returns_policy_violation_for_disallowed_chain` (-32020)
  - `strategy_run_returns_policy_violation_for_disallowed_target` (-32020)
  - `strategy_run_returns_policy_violation_for_disallowed_selector` (-32020)
  - `strategy_run_returns_policy_violation_for_value_cap` (-32020)
  - `strategy_run_returns_policy_violation_for_erc20_spend_cap` (-32020)
  - `strategy_run_returns_policy_violation_for_raw_calldata_when_not_allowed` (-32020)
  - `strategy_run_returns_simulation_failed_when_revert` (-32019; needs anvil OR mock provider — recommend dedicated `_anvil`-suffixed file under `executor-mcp/tests/strategy_run_sim_anvil.rs`)
  - `strategy_run_journal_records_pass_decisions` — `journal://{run_id}` resource read shows `decisions[]` rows with `verdict: "pass"`
  - `strategy_run_journal_records_fail_decision_on_policy_denied`
  - `strategy_run_caps_action_array_length_at_32` (BR-02 carry-forward generalisation)
- **Helper reuse**: `spawn_server_with_state` (common/mod.rs:73), `call_tool` (line 116), `extract_json_result` (line 135).
- **Schema golden round-trip**: a new `decisions_schema_golden_match` test paralleling the existing `schema_contract_round_trip` (line 390) regenerates `Decision.json` / `PolicyVerdict.json` / `SimulationOutcome.json` and asserts byte-equality with committed goldens.

### State-layer unit tests (extend `crates/executor-state/tests/`)

- Add `journal_decision_seq.rs` mirroring `journal_source_read_seq.rs:1-81`:
  - `record_decision_assigns_monotonic_seq_within_run`
  - `list_decisions_orders_by_recorded_at_then_seq` (uses `__test_record_decision_with_time` with two same-instant inserts)
  - `seq_is_per_run_not_global`
- Extend `run_lifecycle_transition.rs` with:
  - `update_run_status_with_transition_accepts_running_to_simulation_denied` — proves the gate now allows the Phase 5 variant.
  - `update_run_status_with_transition_accepts_running_to_policy_denied`.
  - Both rejected pre-Phase-5; the test `update_run_status_with_transition_rejects_phase5_reserved_target` (line 57) inverts: rename to `update_run_status_with_transition_rejects_phase6_reserved_target` and switch the canary to `RunStatus::Canceled`.

---

## Schema-Golden Discipline

- **Locked once at Phase 5 introduction** (mirrors Phase 2 D-05 future-lock for `RunStatus`, Phase 3 D-06 for `JournalActionOutcome`).
- **Files added under `crates/executor-core/tests/schemas/`:**
  - `Decision.json` — full Decision struct.
  - `PolicyVerdict.json` — enum `Pass | Fail | Skipped` + `reason: String`.
  - `SimulationOutcome.json` — enum `Pass | Fail` + `reason: String` + `gas_used: Option<String>`.
  - `PolicyConfig.json` — TOML-parsed shape (replaces existing 574B stub).
  - `PolicyUpdateInput.json` — REGEN (the existing 356B stub becomes a full schema).
- **Files modified (REGEN required):**
  - `StrategyRunResponse.json` (currently 6.7K) — new `decisions: Vec<Decision>` field.
  - `JournalActionOutcome.json` — already enumerates all 6 variants; no change.
  - `RunStatus.json` — already enumerates `simulation_denied` / `policy_denied`; no change.
- **Generation pattern** (Phase 4 precedent): the existing `schema_contract_round_trip` test in `crates/executor-mcp/tests/stdio_handshake.rs:390` reads each `.json` golden and round-trips through `schemars::schema_for!(T)` — Phase 5 extends this list.

---

## Anti-patterns Carry-Forward (Phase 3 + Phase 4 lessons applied to Phase 5)

| Lesson | Source | Phase-5 Application |
|--------|--------|---------------------|
| **HR-01: forbidden-globals scrub before bindings** | Phase 3 | New `ctx.policy.*` host bindings (if any) install AFTER the FORBIDDEN_GLOBALS_SCRUB. **Phase 5 design note**: do NOT add a `ctx.policy` namespace — policy is host-side, not strategy-visible. The strategy returns Action[] and the gate runs after. This avoids the lesson entirely. |
| **MR-01: no raw alloy/reqwest/serde text on the wire** | Phase 3 / Phase 4 | `PolicyError::Display` and `SimulationError::Display` MUST emit ONLY stable taxonomy strings. Raw revert / RPC text → `tracing::warn!` via `detail_for_log`. Tests modeled on `error.rs:132-162` (`display_strings_are_stable_and_wire_safe`). |
| **MR-03: never silently fall back to "[]" or empty on serde failure** | Phase 3 | `record_decision(payload_json: serde_json::to_string(verdict)?)` propagates serde failure as `StateError::SerializationError` (NOT empty fallback). Mirror `tools.rs:539-543`. |
| **MR-04: per-run monotonic seq for ORDER BY tie-break** | Phase 4 | `journal_decisions` MUST carry `seq INTEGER NOT NULL` + `UNIQUE (run_id, seq)`. `next_decision_seq` mirrors `next_log_seq` at `journal.rs:160-167`. `list_decisions_for_run` orders by `(recorded_at ASC, seq ASC)` like `list_logs_for_run` at line 264. |
| **BR-01: stable wire taxonomy must reach the wire even after JS round-trip** | Phase 4 | New `simulation_failed` / `policy_violation` `data.code` strings MUST survive the JS sandbox boundary. Phase 5's gate runs **AFTER** Sandbox::execute, so the JS round-trip is not in the simulation/policy path itself — but if any policy denial happens to be re-classified through `classify_message` (e.g. policy throws inside a builder helper invoked via JS), the stable prefix taxonomy must extend `sandbox.rs::classify_message` (lines 658-718). **Recommendation**: prefix new policy/sim wire strings with `"policy violation: "` / `"simulation failed: "` and add matching arms to `classify_message` so future builder-side policy hooks are forward-compatible. |
| **BR-02: caps enforced at JSON-output gate, not constructor** | Phase 4 | (a) Action[] length cap — enforce in `validate_strategy_output` (tools.rs:437-495). (b) Raw calldata length cap (POL-06) — re-validate at `validate_strategy_output` even though the `RawCallAction` shape was hand-built. (c) Policy file size cap — enforce at `Config::policy_config()` load time, NOT only at TOML parse. |
| **WR-01: no `block_in_place` from inside `spawn_blocking`** | Phase 4 (sandbox.rs WR-01 fix) | Phase 5 sim adapter inside the existing `tools.rs::strategy_run` `spawn_blocking` closure uses `tokio::runtime::Handle::current().block_on(simulate(...))` directly. Pattern locked at `sandbox.rs:1130` and `:1141`. |
| **WR-04: sanitize attacker-controllable revert text** | Phase 4 | Sim revert reasons routed through `executor_evm::read::sanitize_revert_reason` (currently `pub(crate)` at line 255 — **promote to `pub` in 05-01** so `executor-policy::simulate` can import it). Strip control chars, cap at 256 bytes, append `…`. Test mirrors `sanitize_revert_reason_strips_control_chars_and_caps_length` at `read.rs:300-327`. |
| **D-12 transition guard (terminal-state safety)** | Phase 2 / Phase 3 | Every Phase-5 status mutation routes through `update_run_status_with_transition` (`runs.rs:152-196`). NEVER use the deprecated `update_run_status` (lines 105-129). The terminal-state rejection (`Succeeded → *` and `Failed → *`) carries forward — Phase 5 adds `SimulationDenied` and `PolicyDenied` as terminal sinks; extend `runs.rs:166-170` to include them in the terminal set. |
| **D-15d / mutex discipline** | Phase 4 | `state.blocking_lock()` MUST be released before `block_on(provider.call(...))`. Phase 5 simulator + policy evaluator also drop the storage lock before any RPC, then re-acquire it for `record_decision`. Reference: `tools.rs:259-265` for the existing pattern. |
| **journal_decisions.payload_json MR-03 propagation** | Phase 3 | `record_decision` callers serialize via `serde_json::to_string(&payload).map_err(|e| StateError::SerializationError(format!("journal_decisions.payload: {e}")))?` — propagates, never silently empty. Mirror at `tools.rs:539-543`. |

---

## Commit / Branch / Tracking Conventions

- **Conventional-commits scope** (per Phase 4 precedent — see `git log` `feat(04-NN): ...` series):
  - `feat(05-01): scaffold executor-policy crate + PolicyConfig + PolicyError`
  - `feat(05-02): Action -> TxRequest normalize + simulation adapter`
  - `feat(05-03): policy evaluator (POL-01..POL-06) + journal_decisions`
  - `feat(05-04): wire policy + sim into strategy_run handler + stdio coverage`
  - `test(05-NN): ...` for test-only changes
  - `docs(05): code review + verification reports`
- **Per-plan summary file**: `.planning/phases/05-simulation-and-policy-gate/05-NN-SUMMARY.md` (mirrors `04-01-SUMMARY.md` … `04-04-SUMMARY.md`).
- **Phase close**: `docs(05): code review + verification reports (PASS-WITH-NOTES, NNN tests, ...)` — pattern from commit `84e3238` at HEAD.
- **Schema golden regen**: any plan that touches `executor-core/src/schema/*.rs` must regen the matching `tests/schemas/*.json` and commit both in the same commit as the schema change (Phase 2 D-05 / Phase 4 D-08 lock pattern).
- **Anvil-feature gating in CI**: `cargo test --workspace` runs without anvil; add `cargo test --workspace --features anvil-tests` as the gated step. Mirrors `crates/executor-evm/Cargo.toml:36-37`.

---

## Coverage Summary

- New crate scaffolding: 1 (`executor-policy`)
- New modules: 7 (lib, error, config, normalize, simulate, policy, selector)
- Modified files: 11 (`schema.rs`, `journal.rs`, `store.rs`, `lib.rs`, `executor-state/error.rs` (likely no change), `execution.rs`, `policy.rs` schema, `config.rs`, `errors.rs`, `tools.rs`, `server.rs`, `resources.rs`)
- Schema goldens added: 4 (`Decision`, `PolicyVerdict`, `SimulationOutcome`, `PolicyConfig`)
- Schema goldens regen: 2 (`StrategyRunResponse`, `PolicyUpdateInput`)
- Test files added: 4 (`normalize.rs`, `policy_evaluator.rs`, `simulate_anvil.rs`, `journal_decision_seq.rs`)
- Test files extended: 2 (`stdio_handshake.rs`, `run_lifecycle_transition.rs`)

**Files with NO direct analog** (planner falls back to RESEARCH.md): NONE — every Phase-5 file has at least a role-match analog already in the codebase.
