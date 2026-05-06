---
phase: 05-simulation-and-policy-gate
artifact: CONTEXT
status: locked
gathered: 2026-04-27
mode: planner-locked   # /gsd-discuss-phase was skipped — researcher recommendations adopted as defaults
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md       # EXE-01..06 + POL-01..06 + STJ-05
  - .planning/ROADMAP.md            # Phase 5 entry, Plans 05-01..05-04
  - .planning/phases/05-simulation-and-policy-gate/05-RESEARCH.md
  - .planning/phases/05-simulation-and-policy-gate/05-PATTERNS.md
  - .planning/phases/04-evm-context-and-actions/04-CONTEXT.md
  - .planning/phases/04-evm-context-and-actions/04-REVIEW-FIX.md
  - .planning/phases/04-evm-context-and-actions/04-{01,02,03,04}-SUMMARY.md
  - .planning/phases/03-javascript-strategy-runner/03-CONTEXT.md
  - AGENTS.md   # line 35 — executor-policy/ target crate
decisions:
  - D-01: Crate layout — new `crates/executor-policy/` (alloy-free per Phase 4 D-02 carry-forward); new modules `executor-evm/src/{normalize,simulate}.rs` (alloy stays in executor-evm only — researcher win over PATTERNS suggestion).
  - D-02: Action → TxRequest normalization table — per-variant (ContractCall / RawCall / Erc20Transfer / Erc20Approve / NativeTransfer / Noop=skip).
  - D-03: Refactor `dry_run_abi_encode` — extract shared `encode_call_input` so Phase-4 dry-run AND Phase-5 normalize share the encoder. MR-03 propagation preserved.
  - D-04: Add `ERC20_WRITE_ABI` constant — `transfer 0xa9059cbb` + `approve 0x095ea7b3` to `executor-evm/src/erc20.rs` (sibling of Phase-4 `ERC20_ABI` which is read-only).
  - D-05: Simulation adapter — `provider.call(req).block(BlockId)` per-action sequential; from-address from new `[evm.simulation_from]` (default = anvil account[0]); per-call timeout reuses `[evm].call_timeout_ms` (Phase 4 D-04).
  - D-06: Policy DSL — TOML `[policy]` config section, 6 dimensions (chain, contract, selector, max_native_value, erc20_spend, raw_calldata). Deny-by-default per dimension.
  - D-07: Gate ordering — **policy → simulation** (cheapest deny first); two-pass orchestration in `tools.rs::strategy_run`; both gates short-circuit on first denial.
  - D-08: Wire error reuse — `-32017 STRATEGY_RUNTIME_ERROR` extended with new `data.kind ∈ {policy_violation, simulation_failure, max_actions_exceeded, policy_not_loaded}` (researcher win over PATTERNS' new -32019/-32020 — preserves Phase 4 D-12 reservation).
  - D-09: Journal — new `journal_decisions` table with per-run monotonic `seq` (MR-04 carry-forward).
  - D-10: `JournalActionOutcome::{SimulationFailure, PolicyDenied}` — already declared (Phase 3 future-lock); Phase 5 unblocks emission by widening `phase3_emittable → phase5_emittable`.
  - D-11: `StrategyOutcome::Actions` gains `decisions: Vec<ActionDecision>` field on the success path; failure path returns -32017 with partial decisions visible via `journal://{run_id}`.
  - D-12: Action[] length cap = 32 per run (BR-02 carry-forward — enforced at `validate_strategy_output`, NOT only at builder).
  - D-13: Anti-pattern carry-forward (NON-NEGOTIABLE) — HR-01, MR-01, MR-03, MR-04, BR-01, BR-02, WR-01, WR-04, plus prior D-12 / D-15d.
  - D-14: `simulation_from` validation — must be a valid EIP-55 address; loaded via `EvmConfig::from_raw` at boot.
  - D-15: Policy missing/malformed at boot — fail-closed (strategy_run returns -32017 `data.kind="policy_not_loaded"` until policy file present + parses).
  - D-16: ERC20 spend tracking = approve + transfer summed cumulatively per run per token; reset on new run (conservative per A-7).
  - D-17: chain_id — fetched once per provider via `provider.chain_id()` and cached on `ExecutorServer`; lazy refresh on cache miss only.
  - D-18: `MAX_ACTIONS_PER_RUN = 32` constant lives in `executor-mcp::validation` (sibling of `MAX_TAGS = 16`); enforced inside `validate_strategy_output`.
  - D-19: `sanitize_revert_reason` promoted from `pub(crate)` to `pub` in `executor-evm/src/read.rs` (Plan 05-02 prerequisite — `executor-evm::simulate` consumes it).
  - D-20: `executor-policy` does NOT depend on `executor-evm` (consumes `Action` via `executor-core` only — alloy-free); orchestration in `executor-mcp::tools::strategy_run` calls `executor-evm::normalize` → `executor-policy::evaluate` → `executor-evm::simulate` in sequence.
---

# Phase 5: Simulation and Policy Gate — Context (Locked)

**Status:** locked. `/gsd-discuss-phase` was intentionally skipped per orchestrator brief; the researcher's recommendations in `05-RESEARCH.md` are adopted as **default-locked decisions**, deviating only where REQUIREMENTS.md, AGENTS.md, PROJECT.md, or Phase 4 contracts force a different choice. The two researcher-vs-PATTERNS conflicts are resolved in favour of the researcher (D-01 alloy isolation per Phase 4 D-02; D-08 wire-code reuse per Phase 4 D-12).

This document is the agent-facing decision log for Phase 5 plans (05-01 / 05-02 / 05-03 / 05-04). Every plan's `decisions:` frontmatter MUST reference a subset of D-01..D-20 below; every implementation choice in those plans MUST be traceable here.

---

<domain>
## Phase Boundary

**Goal (verbatim from ROADMAP):** No transaction can reach the signer before simulation and policy approval.

**This phase delivers:**
- New crate `crates/executor-policy/` (AGENTS.md line 35) — alloy-free TOML policy DSL parser + 6-dimension deny-by-default evaluator.
- New modules `crates/executor-evm/src/{normalize.rs, simulate.rs}` — `Action → TransactionRequest` normalization (5 variants + Noop skip) and per-action sequential `eth_call` simulation with timeout/sanitization.
- New `[policy]` section in `ExecutorConfig` pointing at a `policy.toml` file; fail-closed boot if missing/malformed.
- New `[evm.simulation_from]` field (default = anvil account[0]) for the simulator's `from` address.
- Extended `data.kind` taxonomy on `-32017 STRATEGY_RUNTIME_ERROR`: `policy_violation`, `simulation_failure`, `max_actions_exceeded`, `policy_not_loaded` (no new wire codes — Phase 4 D-12 precedent).
- New `journal_decisions` table + `record_decision` / `list_decisions_for_run` repo methods (MR-04 carry-forward — per-run monotonic `seq`).
- `StrategyOutcome::Actions` widened with `decisions: Vec<ActionDecision>` on the success path.
- `phase3_emittable → phase5_emittable` rename — `JournalActionOutcome::{SimulationFailure, PolicyDenied}` becomes emittable; `RunStatus::{SimulationDenied, PolicyDenied}` becomes legal terminal states (Phase 2 D-05 future-lock unblocked).
- Two new gate-pipeline steps in `executor-mcp::tools::strategy_run`: **policy → simulation**, both short-circuit on first denial.
- `journal://{run_id}` resource gains a `decisions[]` array.
- Schema goldens added/regenerated: `Decision.json`, `PolicyVerdict.json`, `SimulationOutcome.json`, `PolicyConfig.json` (replaces 574B stub), `StrategyRunResponse.json` (regen for new field).

**This phase does NOT deliver:**
- Local signer / private-key handling (Phase 6).
- tx broadcast / receipt waiting / tx-hash recording (Phase 6).
- Per-strategy policy binding (v2; researcher A-10).
- `policy_update` runtime mutation (v2; researcher Q-7 — stays at -32010).
- Bundled / parallel simulation (researcher A-3).
- Hot-reload of policy file (researcher A-6 — restart server to update).
- Gas estimation in simulation payload (Phase 6 signer concern).
- `ctx.evm.chainId` exposure to strategy code (Phase 4 D-07 — chain identity is a host-side policy concern only).

When Phase 5 ships: `strategy_run` rejects any disallowed chain / contract / selector / native value / ERC20 spend / raw calldata before the signer would see it; simulator reverts surface as `-32017 simulation_failure` with sanitized decoded reason; every gate verdict (pass + fail) lands in `journal_decisions` keyed by `(run_id, action_index, gate)`; the success-path response carries a `decisions[]` array agents can consume to confirm what was approved.
</domain>

<requirements_text>
## Phase Requirements (verbatim from REQUIREMENTS.md)

| ID | Verbatim text | Source line |
|----|---------------|-------------|
| **EXE-01** | "Runtime validates `Action[]` before any simulation or signing." | REQUIREMENTS.md:38 |
| **EXE-02** | "Runtime ABI-encodes contract call actions into transaction requests." | REQUIREMENTS.md:39 |
| **EXE-03** | "Runtime simulates transaction requests before signing." | REQUIREMENTS.md:40 |
| **EXE-04** | "Runtime denies signing when simulation fails." | REQUIREMENTS.md:41 |
| **EXE-05** | "Runtime applies policy before signing." | REQUIREMENTS.md:42 |
| **EXE-06** | "Runtime denies signing when policy rejects an action." | REQUIREMENTS.md:43 |
| **POL-01** | "Policy can restrict allowed chain IDs." | REQUIREMENTS.md:50 |
| **POL-02** | "Policy can restrict target contract addresses." | REQUIREMENTS.md:51 |
| **POL-03** | "Policy can restrict function selectors." | REQUIREMENTS.md:52 |
| **POL-04** | "Policy can restrict max native value per action." | REQUIREMENTS.md:53 |
| **POL-05** | "Policy can restrict max ERC20 spend for helper-generated ERC20 actions." | REQUIREMENTS.md:54 |
| **POL-06** | "Raw calldata actions are denied unless explicitly allowed by policy." | REQUIREMENTS.md:55 |
| **STJ-05** | "Runtime records simulation results and policy decisions." | REQUIREMENTS.md:63 |

**Note on EXE-01:** Phase 4 already validates Action shape at the JSON-output gate (`validate_strategy_output` + `dry_run_abi_encode` + per-kind validators). EXE-01 is therefore *closeable in Phase 5 by reference* — Phase 5 reuses the existing Phase 4 validation; the Phase-5 deliverable for EXE-01 is documenting that validation precedes normalization in the new pipeline order AND adding the `MAX_ACTIONS_PER_RUN = 32` cap at the same gate (D-12).
</requirements_text>

<decisions>
## Locked Decisions

### Crate layout

- **D-01: New workspace member `crates/executor-policy/`. New modules `crates/executor-evm/src/{normalize.rs, simulate.rs}`.**
  - **Why a new crate (not extension of executor-evm):** AGENTS.md line 35 names `executor-policy/` as the target; `executor-evm` is read-focused (Provider/ABI/decode); mixing in policy DSL muddies the boundary; dep graph hygiene (`executor-policy` → `executor-evm`-or-not, NEVER reverse).
  - **Why alloy stays per-crate (not promoted to workspace.dependencies):** PATTERNS suggested promoting alloy to `[workspace.dependencies]` on the 3-consumer threshold (executor-evm + executor-policy + executor-mcp). RESEARCH locks `executor-policy` as **alloy-free** (consumes `Action` via `executor-core`, U256/Address via `alloy-primitives` only). Phase 4 D-02 isolation contract is preserved: alloy lives in `executor-evm` only. **Researcher wins.** alloy stays per-crate-pinned in `executor-evm/Cargo.toml`.
  - **Module layout:**
    ```
    crates/executor-policy/
      Cargo.toml                    # alloy-free deps: executor-core, alloy-primitives 1, serde, serde_json, thiserror, toml, tracing
      src/
        lib.rs                      # #![deny(clippy::print_stdout, ...)] + module decls + pub uses
        error.rs                    # PolicyError + SimulationError-like (alloy-free)
        config.rs                   # PolicyConfig::from_raw(toml::Value) -> Result<_, PolicyError>
        model.rs                    # Policy struct + per-dimension subtypes (Chains, Contracts, Selectors, NativeValueCaps, Erc20Spend, RawCall)
        load.rs                     # parse policy.toml file path → PolicyConfig
        eval.rs                     # PolicyEvaluator::evaluate(&Decision, &PolicyConfig) → PolicyVerdict (stateless; ERC20 cumulative tally tracked at orchestrator)
        decision.rs                 # Decision input shape (chain_id, action_index, action_kind, to, selector, native_value, erc20_amount) + verdict factory
        selector.rs                 # 4-byte selector extraction from raw calldata (POL-03)
      tests/
        common/
          mod.rs
          fixtures/
            policy.permissive.toml   # allows everything on chain 31337
            policy.deny_all.toml
            policy.bad_address.toml
        eval_chains.rs               # POL-01 unit tests
        eval_contracts.rs            # POL-02
        eval_selectors.rs            # POL-03
        eval_native_value.rs         # POL-04
        eval_erc20_spend.rs          # POL-05
        eval_raw_calldata.rs         # POL-06
        load_toml.rs                 # parse + validation tests
    ```
  - **executor-evm new modules:**
    ```
    crates/executor-evm/src/
      normalize.rs   # Plan 05-01 — Action → NormalizedAction { tx, source, selector, native_value, erc20_amount }
      simulate.rs    # Plan 05-02 — simulate_one(provider, cfg, tx, block, from) → SimulationOutcome
    ```
  - **Workspace `members` update:** root `Cargo.toml` adds `crates/executor-policy` to the existing list. `crates/executor-evm` already in.
  - **Dep graph (locked):**
    - `executor-policy` → `executor-core` + `alloy-primitives` + `serde` + `toml` + `thiserror` + `tracing` (NO `executor-evm`, NO `alloy`).
    - `executor-mcp` → `executor-policy` (new) + `executor-evm` (existing) + others.
    - `executor-evm` → no change to deps.

### Action → TransactionRequest normalization (EXE-02)

- **D-02: Per-variant normalization table. Output type `NormalizedAction { tx, source, selector, native_value, erc20_amount }`.**
  - **Per-variant table:**

    | Action variant | `tx.to` | `tx.data` | `tx.value` | `selector` | `native_value` | `erc20_amount` |
    |----------------|---------|-----------|------------|------------|----------------|----------------|
    | `Noop` | — (skipped — no row, no decision; counts as success) | — | — | — | — | — |
    | `ContractCall` | `cc.address → Address` | `encode_call_input(abi, function, args)` (D-03 shared encoder) | `U256::from_str_radix(cc.value, 10)` | `Some(data[0..4])` | `tx.value` | `None` |
    | `RawCall` | `rc.address` | `Bytes::from_str(rc.data)` (Phase-4 validated) | `U256::from_str_radix(rc.value, 10)` | `if data.len() >= 4 { Some(data[0..4]) } else { None }` | `tx.value` | `None` |
    | `Erc20Transfer` | `et.token` | selector `0xa9059cbb` ++ ABI-encoded `(to, amount)` via `ERC20_WRITE_ABI` (D-04) | `U256::ZERO` | `Some([0xa9, 0x05, 0x9c, 0xbb])` | `0` | `Some(et.amount as U256)` |
    | `Erc20Approve` | `ea.token` | selector `0x095ea7b3` ++ ABI-encoded `(spender, amount)` | `U256::ZERO` | `Some([0x09, 0x5e, 0xa7, 0xb3])` | `0` | `Some(ea.amount as U256)` |
    | `NativeTransfer` | `nt.to` | `Bytes::new()` (empty `0x`) | `U256::from_str_radix(nt.value, 10)` | `None` | `tx.value` | `None` |

  - **Output struct (Plan 05-01 owns):**
    ```rust
    // crates/executor-evm/src/normalize.rs
    use alloy::rpc::types::TransactionRequest;
    use alloy_primitives::{Address, U256};

    #[derive(Debug, Clone)]
    pub struct NormalizedAction {
        pub tx: TransactionRequest,                 // to, data, value populated; gas/nonce/chainId NOT (Phase 6)
        pub source: NormalizedActionKind,
        pub selector: Option<[u8; 4]>,
        pub native_value: U256,
        pub erc20_amount: Option<U256>,
    }

    #[derive(Debug, Clone, Copy)]
    pub enum NormalizedActionKind {
        ContractCall, RawCall, Erc20Transfer, Erc20Approve, NativeTransfer,
    }

    pub fn normalize_action(action: &Action) -> Result<Option<NormalizedAction>, EvmError>;
    // Returns Ok(None) for Noop; Ok(Some(_)) for the five emitting variants.
    ```

  - **Selector for sub-4-byte calldata:** `RawCall` with `data.len() < 4` → `selector = None` (Pitfall 4). Policy POL-03 skips for `None`; POL-06 raw_call gate still applies.
  - **Stable encode error taxonomy (extends Phase 4 `EvmError::Encode`):** `"bad_decimal_value"`, `"bad_address_to"`, `"erc20_abi"` (if D-04 fragments fail to parse — should never happen in practice). All wire-safe per MR-01.

- **D-03: Shared encoder `encode_call_input(abi: &str, function: &str, args: &[serde_json::Value]) → Result<Bytes, EvmError>`.**
  - **Why:** Phase 4's `dry_run_abi_encode` (`crates/executor-evm/src/action.rs:156`) parses the ABI, resolves the overload, encodes the args, then DROPS the bytes. Phase 5 needs the bytes. Extract shared `encode_call_input` and have BOTH `dry_run_abi_encode` (Phase 4 — discards) AND `normalize_contract_call` (Phase 5 — keeps) call it. **Pitfall P-5.**
  - **Location:** `crates/executor-evm/src/dyn_abi.rs` (Phase-4 file owns the dyn-abi machinery already). Sibling fn signature `pub fn encode_call_input(...) -> Result<Bytes, EvmError>`.
  - **MR-03 / Phase 4 BR-01 propagation preserved:** the encoder uses `?` throughout; `Cow::Owned` category strings flow into `EvmError::Encode { category, detail_for_log }` which is wire-safe.
  - **Refactor scope (Plan 05-01 owns):** modify `executor-evm/src/action.rs::dry_run_abi_encode` to delegate to the new function and discard the result. Existing Phase-4 tests (`action::tests::dry_run_*` 04-04 negative grid) MUST stay green unchanged — this is a pure refactor.

- **D-04: `ERC20_WRITE_ABI` constant in `crates/executor-evm/src/erc20.rs`.**
  - **Why a sibling, not extending `ERC20_ABI`:** Phase 4 `ERC20_ABI` ships READ-only fragments (balanceOf, allowance, decimals, symbol, name, totalSupply). Adding `transfer`/`approve` to the same constant would (a) churn the schema golden if `ERC20_ABI` is ever serialized, (b) blur read-vs-write semantic separation. Sibling `ERC20_WRITE_ABI` is clearer.
  - **Constant shape:**
    ```rust
    // crates/executor-evm/src/erc20.rs
    /// OpenZeppelin-compatible ERC20 WRITE ABI fragments.
    /// Selectors are universal: transfer = 0xa9059cbb, approve = 0x095ea7b3.
    pub const ERC20_WRITE_ABI: &str = r#"[
        {"type":"function","name":"transfer","inputs":[{"name":"to","type":"address"},{"name":"value","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"},
        {"type":"function","name":"approve","inputs":[{"name":"spender","type":"address"},{"name":"value","type":"uint256"}],"outputs":[{"name":"","type":"bool"}],"stateMutability":"nonpayable"}
    ]"#;
    ```
  - **Selector verification (Plan 05-01 acceptance):** unit test asserting that `encode_call_input(ERC20_WRITE_ABI, "transfer", [to, amount])[..4] == [0xa9, 0x05, 0x9c, 0xbb]`.

### Simulation adapter (EXE-03 / EXE-04)

- **D-05: `simulate_one(provider, cfg, tx, block, from) → SimulationOutcome`. Per-action sequential. From-address from `[evm.simulation_from]`.**
  - **API (locked):**
    ```rust
    // crates/executor-evm/src/simulate.rs
    pub async fn simulate_one(
        provider: Arc<DynProvider>,
        cfg: &EvmConfig,
        tx: &TransactionRequest,
        block: BlockId,
        from: Option<Address>,
    ) -> SimulationOutcome;

    pub enum SimulationOutcome {
        Pass { return_bytes: Bytes, gas_estimate: Option<u64> },
        Fail { reason: SimulationFailReason, raw_for_log: String },
    }

    pub enum SimulationFailReason {
        Revert { decoded: Option<String> },   // sanitized via WR-04 carry-forward (D-19)
        Transport,
        Timeout,
    }
    ```
  - **Why Outcome enum (not `Result<_, EvmError>`):** revert is a NORMAL simulation result that means "deny signing" (EXE-04) — distinct from `EvmError::Transport` ("anvil down"). Distinguishing the two prevents conflating denial with infrastructure failure.
  - **Provider call:** `provider.call(tx).block(block_id).await` — same alloy 2.0 surface used by Phase 4 `read_contract` (verified at `executor-evm/src/read.rs:121`).
  - **Per-call timeout:** reuse `EvmConfig::call_timeout` (Phase 4 D-04 default 1s, range 50ms..30s). Pattern: `tokio::time::timeout(cfg.call_timeout, future).await`.
  - **From-address:** new `[evm.simulation_from]` field; default = anvil account[0] (`0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92265` — note the EIP-55 checksum form). If absent, default applies (devnet-friendly); for non-anvil RPCs the operator must set it explicitly. **D-14 validates the address at `EvmConfig::from_raw`.**
  - **Block tag:** `BlockId::latest()` for v1. Pinning to a strategy-run start block is a v2 concern (out of scope per researcher).
  - **Per-action sequential:** iterate normalized actions, call `Handle::current().block_on(simulate_one(...))` per action **inside the existing `spawn_blocking` closure** (Phase 4 D-04 / WR-01 carry-forward). Do NOT spawn a new `spawn_blocking` for the simulation loop.
  - **Total wall-clock budget:** worst case 32 actions × 1s = 32s. Researcher A-4 proposes `[evm.simulation_total_timeout_ms]` (default 5_000ms). **Phase 5 lock:** include the field with default 5_000ms; documented as "best-effort total cap" — Plan 05-02 wires `tokio::time::timeout(total_timeout, ...)` around the loop. If the loop-level timeout fires mid-action, the in-flight action's per-call timeout still wins (whichever fires first). Failure mode = `SimulationFailReason::Timeout` for the in-flight action.
  - **Reuse of Phase-4 helpers (D-19 prerequisite):** `simulate_one` MUST consume `executor_evm::read::sanitize_revert_reason`. Currently `pub(crate)` — D-19 promotes to `pub` so the simulator reuses it without copy-paste. WR-04 carry-forward — every revert reason on the wire is sanitized (control-char strip + 256-byte cap).
  - **Gas estimation NOT populated in v1.** `SimulationOutcome::Pass.gas_estimate = None`. Field reserved for Phase 6.
  - **Failure-modes mapping:**

    | Source | `SimulationOutcome` |
    |--------|---------------------|
    | `Provider::call` returns `Ok(bytes)` | `Pass { return_bytes: bytes, gas_estimate: None }` |
    | `Provider::call` returns `Err` and error decodes as revert | `Fail { reason: Revert { decoded: try_extract_revert_reason(raw).map(sanitize_revert_reason) }, raw_for_log: raw }` |
    | `Provider::call` returns `Err` (transport) | `Fail { reason: Transport, raw_for_log: raw }` |
    | `tokio::time::timeout` fires | `Fail { reason: Timeout, raw_for_log: "tokio::time::timeout fired" }` |

### Policy DSL (POL-01..06)

- **D-06: TOML `[policy]` section + 6-dimension deny-by-default evaluator.**
  - **TOML schema (locked):**
    ```toml
    # policy.toml — referenced from [policy] in main config.toml

    # POL-01: chain id allowlist. Empty = deny all.
    [chains]
    allow = [31337, 1, 137, 8453]

    # POL-02: per-chain contract address allowlist.
    [contracts.31337]
    allow = [
      "0x5fbdb2315678afecb367f032d93f642f64180aa3",   # Counter
      "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",   # MockERC20
    ]
    [contracts.1]
    allow = []   # mainnet locked-out by default

    # POL-03: function-selector allowlist per (chain, contract). 4-byte 0x-hex
    # or "any" sentinel. Applies to ContractCall + Erc20Transfer + Erc20Approve.
    # Does NOT apply to RawCall (POL-06 owns that gate — see policy interaction below).
    [selectors."31337:0x5fbdb2315678afecb367f032d93f642f64180aa3"]
    allow = ["0xd09de08a"]   # increment()

    [selectors."31337:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"]
    allow = ["any"]

    # POL-04: max native value per single action (wei decimal string).
    [native_value.31337]
    max_per_action = "1000000000000000000"   # 1 ETH

    # POL-05: ERC20 spend caps per (chain, token). "Spend" = transfer.amount
    # + approve.amount, summed cumulatively per RUN per token. Helper-generated
    # only (Erc20Transfer / Erc20Approve); raw_call/contract_call that happen
    # to call transfer/approve are NOT inspected here (constrained by
    # contracts + selectors + raw_calldata instead).
    [erc20_spend."31337:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"]
    max_per_run = "1000000000000000000000"   # 1000 tokens (18 dp)

    # POL-06: raw_call gate. Default DENY.
    [raw_call]
    allow_global = false
    allow = [
      { chain = 31337, contract = "0x5fbdb2315678afecb367f032d93f642f64180aa3", selector = "0xd09de08a" },
    ]
    ```
  - **Why TOML:** matches existing `[state]`, `[evm]`, `[logging]`, `[mcp]` sections (`crates/executor-mcp/src/config.rs`). Strategists already author TOML to operate the runtime.
  - **Why deny-by-default uniformly:** POL-06 explicitly deny-by-default; the model only makes sense if EVERY dimension is — empty `[chains]` → no chain allowed, empty `[contracts.<id>]` → no contract on that chain allowed. Apply uniformly.
  - **Policy/RawCall interaction (Pitfall A-5 resolved):** **selector check (POL-03) does NOT apply to `RawCall`** — POL-06 `[raw_call]` is the exclusive gate. Rationale: defense-in-depth across both gates would require operators to maintain two parallel allowlists for the same tuple; researcher A-5 default. ContractCall + Erc20Transfer + Erc20Approve still pass through selector check.
  - **Evaluation order (cheap-first short-circuit):**
    1. CHAIN (POL-01) — single hashmap lookup.
    2. CONTRACT (POL-02) — per-chain hashmap lookup.
    3. RAW gate (POL-06) — only for RawCall variant.
    4. SELECTOR (POL-03) — only for non-RawCall variants with `selector = Some(_)`.
    5. NATIVE-VALUE (POL-04) — only if `native_value > 0`.
    6. ERC20-SPEND (POL-05) — only for Erc20Transfer/Approve; running U256 sum vs cap, maintained at orchestrator (`tools.rs::strategy_run`) NOT inside `evaluate`.
  - **Stable violation taxonomy (locked):** `chain_not_allowed` / `contract_not_allowed` / `selector_not_allowed` / `native_value_exceeds` / `erc20_spend_exceeds` / `raw_call_denied`. These exact strings populate `data.rule` on the wire (D-08).
  - **Validation at policy load:**
    - Every address parses via `Address::from_str` (lenient — policy authors may write lowercase or EIP-55 checksum); rejected on malformed.
    - Every selector matches `^0x[0-9a-fA-F]{8}$` or literal `"any"`.
    - Every U256 cap parses as decimal-string (D-03 BigInt convention preserved).
    - `#[serde(deny_unknown_fields)]` on every struct (Phase 4 D-08 carry-forward).
    - Every chain in `[chains.allow]` MUST have a corresponding `[contracts.<id>]` sub-table (Pitfall P-10 — catches typos at boot).
  - **Hot-reload:** NOT in v1. `Arc<RwLock<PolicyConfig>>` field on `ExecutorServer`; loaded once at boot. `policy_update` MCP tool stays unimplemented (-32010) per researcher Q-7.
  - **`policy_get`:** returns the loaded policy verbatim via `serde_json::to_value(&Arc<PolicyConfig>)` (researcher Q-6). Replaces the Phase-1 placeholder body.

### Gate ordering and orchestration (EXE-05 / EXE-06)

- **D-07: Policy → Simulation. Two-pass orchestration. Short-circuit on first denial.**
  - **Why policy first:** policy = in-memory hashmap+set lookups (sub-microsecond). Simulation = network round-trip (10–500ms). A policy-denied action MUST NOT consume an `eth_call` slot (cost + correctness — for 32 actions with deny on action[5], we shouldn't pay 32 RPCs to discover 31 valid simulations of actions we'll never sign).
  - **Pipeline order in `executor-mcp::tools::strategy_run` (Plan 05-04 owns):**
    ```
    STEP A. Phase 4 validate_strategy_output            (closes EXE-01)
    STEP B. enforce MAX_ACTIONS_PER_RUN cap (D-12 / D-18) → -32018 max_actions_exceeded path goes through validate_strategy_output too (BR-02)
    STEP C. for each action: normalize → NormalizedAction (Plan 05-01 owns)
    STEP D. POLICY LOOP — for (idx, na) in normalized:
              policy.evaluate(&Decision::from(na, chain_id), &policy) → PolicyVerdict
              record_decision(gate="policy", verdict=...)
              if Deny: transition Running → PolicyDenied; return -32017 policy_violation. Break.
    STEP E. SIM LOOP — for (idx, na) in normalized:
              simulate_one(provider, cfg, &na.tx, BlockId::latest(), Some(simulation_from))
              record_decision(gate="simulation", verdict=...)
              if Fail: transition Running → SimulationDenied; return -32017 simulation_failure. Break.
    STEP F. record journal_actions outcome=actions (Phase 3 path; unchanged)
    STEP G. (Phase 6 inserts signer here)
    STEP H. transition Running → Succeeded; return StrategyRunResponse with decisions[]
    ```
  - **Two-pass (D then E) vs interleaved (P[i]→S[i]→P[i+1]→S[i+1]):** in v1 with no inter-action state both produce the same observable outcome on success. Two-pass is chosen for journal clarity — every policy-pass row lands before the simulation loop begins, so reading `journal_decisions` during a sim failure shows "policy passed all 5; simulation failed on action[2]" cleanly.
  - **Mutex discipline (D-15d / Phase 4 carry-forward):** `state.blocking_lock()` MUST be RELEASED before every `block_on(provider.call(...))`. Re-acquired only for `record_decision` writes. Same pattern as Phase 4 `tools.rs:259-265`.
  - **Concurrency (WR-01 carry-forward):** all of D + E runs inside the EXISTING `spawn_blocking` closure. No nested `spawn_blocking`, NO `block_in_place`. Direct `Handle::current().block_on(...)`.
  - **ERC20 cumulative tally:** orchestrator owns `HashMap<(u64, Address), U256>`; passes by `&mut` to `evaluate` only for the POL-05 dimension. Stateless evaluator otherwise.

### Wire shape extension

- **D-08: Reuse `-32017 STRATEGY_RUNTIME_ERROR` with new `data.kind` values: `policy_violation`, `simulation_failure`, `max_actions_exceeded`, `policy_not_loaded`. NO new wire codes.**
  - **Why not -32019/-32020 (PATTERNS suggestion):** Phase 4 D-12 reserved -32019 ("stays reserved"); -32020 unallocated. Allocating per-failure-mode codes pollutes the namespace; agents already dispatch on `data.kind`. The phrase "runtime error" covers "the runtime denied the action" — these are decisions BY the runtime DURING the run. **Researcher wins.** Phase 4 D-12 precedent honored.
  - **Data shape on wire:**
    ```jsonc
    // Policy denial (-32017)
    {
      "code": "strategy_runtime_error",
      "kind": "policy_violation",
      "rule": "contract_not_allowed",
      "action_index": 1,
      "detail": "policy violation: contract 0xdead... not allowed on chain 31337",
      "run_id": "01ARZ..."
    }

    // Simulation failure (-32017)
    {
      "code": "strategy_runtime_error",
      "kind": "simulation_failure",
      "fail_reason": "revert" /* or "transport" or "timeout" */,
      "action_index": 0,
      "decoded_revert": "ERC20: insufficient balance",  // sanitized; null if unknown
      "detail": "simulation failed: evm revert: ERC20: insufficient balance",
      "run_id": "01ARZ..."
    }

    // Max actions cap (-32018; this is shape, NOT runtime — see Pitfall P-6)
    // Note: max_actions_exceeded actually surfaces via -32018 STRATEGY_INVALID_OUTPUT
    // per Pitfall P-6 / researcher Q-8 — keeping wire taxonomy clean. Listed here
    // as a Phase-5 wire surface because the cap is Phase-5-introduced.
    {
      "code": "strategy_invalid_output",
      "detail": "actions length 33 exceeds MAX_ACTIONS_PER_RUN 32"
    }

    // Policy not loaded (-32017)
    {
      "code": "strategy_runtime_error",
      "kind": "policy_not_loaded",
      "detail": "policy violation: policy file not loaded — set [policy].path in config",
      "run_id": "01ARZ..."
    }
    ```
  - **`detail` prefix discipline (BR-01 carry-forward):** every Phase-5 wire detail starts with one of the stable prefixes `"policy violation: "` or `"simulation failed: "`. The Phase-4 `executor_evm::EvmError::Display` prefixes (`"evm rpc error: "`, `"evm revert: "`, etc.) propagate through unchanged. **`strategy-js::sandbox::classify_message` does NOT need new arms** — Phase 5 gate runs AFTER `Sandbox::execute` returns, so policy/sim errors never round-trip through JS. (Documented; if a future builder helper invokes a policy hook from inside JS, BR-01 reclassification will need to extend.)
  - **Implementation:** new `executor-mcp::errors` factories `policy_violation(action_index, rule, detail, run_id)` and `simulation_failure(action_index, fail_reason, decoded, run_id)` and `policy_not_loaded(run_id)` and (for the cap) `strategy_invalid_output("actions length ... exceeds ...")`. Each delegates to existing typed-error constructors with structured `data` payload. Raw alloy / serde / rusqlite text NEVER reaches the wire (HR-01/MR-01 carry-forward).

### Decision journal (STJ-05)

- **D-09: New `journal_decisions` table. Per-(action, gate) granularity. Per-run monotonic `seq`.**
  - **Schema (additive — `CREATE TABLE IF NOT EXISTS`; no migration):**
    ```sql
    CREATE TABLE IF NOT EXISTS journal_decisions (
        id           TEXT PRIMARY KEY,             -- ULID
        run_id       TEXT NOT NULL REFERENCES runs(id),
        action_index INTEGER NOT NULL,             -- 0-based
        gate         TEXT NOT NULL,                -- "policy" | "simulation"
        verdict      TEXT NOT NULL,                -- "pass" | "fail" | "skipped"
        rule         TEXT,                         -- stable rule name when verdict="fail"
        detail       TEXT,                         -- stable taxonomy string (wire-safe)
        payload_json TEXT,                         -- serialized Decision/SimulationOutcome (NOT NULL on fail)
        recorded_at  TEXT NOT NULL,                -- RFC3339
        seq          INTEGER NOT NULL,             -- per-run monotonic (MR-04)
        UNIQUE (run_id, seq)
    );
    CREATE INDEX IF NOT EXISTS idx_journal_decisions_run_id ON journal_decisions(run_id);
    ```
  - **MR-03 carry-forward:** `payload_json = serde_json::to_string(&payload)?` — `?`-propagate via `StateError::SerializationError`. NEVER silent `unwrap_or_else(|_| "[]".into())`.
  - **MR-04 carry-forward:** `next_decision_seq(conn, run_id) -> Result<i64, StateError>` mirrors `next_log_seq` / `next_source_read_seq`. `list_decisions_for_run` orders by `(recorded_at ASC, seq ASC)`.
  - **Repo API (Plan 05-04 owns):**
    ```rust
    // crates/executor-state/src/journal.rs
    pub fn record_decision(
        conn: &Connection,
        run_id: &str,
        action_index: i64,
        gate: DecisionGate,             // enum Policy | Simulation
        verdict: DecisionVerdict,       // enum Pass | Fail | Skipped
        rule: Option<&str>,
        detail: Option<&str>,
        payload: Option<&serde_json::Value>,
    ) -> Result<String, StateError>;     // returns the ULID

    pub fn list_decisions_for_run(conn: &Connection, run_id: &str)
        -> Result<Vec<DecisionEntry>, StateError>;

    // store façade in store.rs
    impl StateStore {
        pub fn record_decision(&self, ...) -> Result<String, StateError>;
        pub fn list_decisions_for_run(&self, ...) -> Result<Vec<DecisionEntry>, StateError>;
        #[doc(hidden)]
        pub fn __test_record_decision_with_time(&self, ..., recorded_at: &str)
            -> Result<String, StateError>;   // for same-ms ordering test
    }
    ```

- **D-10: `phase3_emittable → phase5_emittable`. Unblock `JournalActionOutcome::{SimulationFailure, PolicyDenied}` emission.**
  - **Current state (Phase 3 / Phase 4):** `JournalActionOutcome::phase3_emittable` returns `false` for SimulationFailure + PolicyDenied. The `record_action_outcome` gate at `executor-state/src/journal.rs:141` blocks emission.
  - **Phase 5 change:** rename `phase3_emittable` → `phase5_emittable`; widen the predicate to return `true` for ALL six variants (Noop, Actions, ValidationError, RuntimeError, SimulationFailure, PolicyDenied). Update the call site. Researcher Q-4 (rename, not add-a-sibling).
  - **Same change for `RunStatus`:** the transition guard at `executor-state/src/runs.rs::update_run_status_with_transition` already accepts `SimulationDenied` / `PolicyDenied` as wire-locked variants — verify (or extend) the transition table to permit `Running → SimulationDenied` and `Running → PolicyDenied` AS terminal states.
  - **Phase-3 reservation test inversion:** the existing `update_run_status_with_transition_rejects_phase5_reserved_target` test (in `crates/executor-state/tests/run_lifecycle_transition.rs`) inverts: rename to `..._rejects_phase6_reserved_target` and switch the canary to `RunStatus::Canceled` (or whichever variant is still reserved post-Phase-5). Plan 05-04 owns the rename.

### Response shape

- **D-11: `StrategyOutcome::Actions` gains `decisions: Vec<ActionDecision>`. Success path only.**
  - **Definition (lands in `crates/executor-core/src/schema/execution.rs`):**
    ```rust
    pub enum StrategyOutcome {
        Noop,
        Actions {
            actions: Vec<Action>,
            #[serde(default)]                 // backward-compat for any pre-Phase-5 deserializer
            decisions: Vec<ActionDecision>,    // length == actions.len() on success
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct ActionDecision {
        pub action_index: u32,
        pub policy: GateVerdict,
        pub simulation: GateVerdict,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum GateVerdict {
        Pass,
        Skipped,                             // simulation skipped because policy denied earlier
        Fail { rule: String, detail: String },
    }
    ```
  - **Failure path:** the response is NOT returned; an MCP -32017 error is raised. Partial decisions live in `journal://{run_id}` (decision rows up to and including the failing one).
  - **Noop path:** `StrategyOutcome::Noop` — no actions, no decisions. Noop never reaches the gate pipeline.
  - **Schema golden (Plan 05-04):** regenerate `StrategyOutcome.json` and `StrategyRunResponse.json`. Add new goldens `Decision.json` (input shape to evaluator), `PolicyVerdict.json`, `SimulationOutcome.json`, `PolicyConfig.json` (replaces the existing 574B stub).

### Caps and validation

- **D-12: `MAX_ACTIONS_PER_RUN = 32` cap. Enforced at `validate_strategy_output` (BR-02 carry-forward).**
  - **Why 32:** covers realistic compositions (approve+swap+transfer+settle ~ 4 actions) with 8x headroom; protects against a runaway agent returning 10_000 actions burning 10_000 eth_calls. Researcher A-1 default.
  - **Wire surface:** `-32018 STRATEGY_INVALID_OUTPUT` with detail `"actions length {n} exceeds MAX_ACTIONS_PER_RUN {32}"`. Researcher Q-8 / Pitfall P-6 — shape problems stay on -32018; -32017 is for "your strategy executed but the runtime/policy/simulator rejected".
  - **Enforcement site (BR-02 carry-forward):** `executor-mcp::tools::validate_strategy_output` (NOT only at the strategy-js builder level). The builder-level cap (if any) is defense-in-depth; the JSON-output gate is the source of truth. Mirrors Phase 4 `MAX_ABI_BYTES` enforcement at `validate_strategy_output`.
  - **Constant location:** `crates/executor-mcp/src/validation.rs` — `pub const MAX_ACTIONS_PER_RUN: usize = 32;` sibling of existing `MAX_TAGS = 16`.

- **D-18: `MAX_ACTIONS_PER_RUN = 32`** (numeric duplicate of D-12 — listed separately for frontmatter referencing convenience).

### Simulator from-address validation

- **D-14: `[evm.simulation_from]` must be a valid EIP-55 address (or lowercase 40-hex). Validated at `EvmConfig::from_raw` boot.**
  - **Default:** `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266` (anvil account[0], EIP-55 form).
  - **Validation:** `Address::parse_checksummed(s, None).or_else(|_| Address::from_str(s))` — lenient like Phase 4 D-09. Mixed-case-but-bad-checksum rejected with stable detail `"simulation_from looks checksummed but checksum is invalid"`.
  - **Plan 05-02 ownership:** extends `EvmConfig::from_raw` and the `[evm]` config section. Adds a `from_raw` test asserting bad-checksum rejection. Backward-compat: if `[evm.simulation_from]` is absent, default applies.

### Fail-closed boot

- **D-15: Policy missing/malformed at boot — fail-closed.**
  - **Behaviour:** if `[policy].path` is missing OR file does not exist OR file fails to parse, EVERY `strategy_run` call returns `-32017 policy_violation` with `data.kind = "policy_not_loaded"` and `data.detail = "policy violation: policy file not loaded — set [policy].path in config"`.
  - **NOT a server-boot panic.** The server still starts (matches Phase 4 D-04 lazy-EVM rationale: agents may register strategies without policy/EVM live yet). The denial happens at `strategy_run` time. Rationale: server boot must remain robust; `strategy_run` is the security-relevant choke point.
  - **Why not silently default-allow:** REQUIREMENTS POL-06 ("denied unless explicitly allowed by policy") makes default-allow a safety hole. A missing policy file in production would let everything through. Fail-closed is the correct default.
  - **`Arc<RwLock<PolicyConfig>>` field on `ExecutorServer`** is loaded from `[policy].path` in `ExecutorServer::new_with_config`. Failure to load is captured and stored as `Arc<RwLock<Option<PolicyConfig>>>` (or equivalent — `None` triggers `policy_not_loaded`). Plan 05-03 picks the cleaner shape; researcher recommends the option-wrapped form.

### ERC20 spend tracking

- **D-16: ERC20 spend = approve.amount + transfer.amount, summed cumulatively per RUN per (chain, token). Reset on new run.**
  - **Why conservative (sum approve+transfer):** approve N tokens AUTHORIZES future spends of N. For policy purposes, an approve IS a potential spend — researcher P-3 / A-7. The model can over-reject (approve N then transfer through that approval double-counts), but never under-estimates spend.
  - **Where the tally lives:** orchestrator (`tools.rs::strategy_run`). `HashMap<(u64, Address), U256>` accumulator passed to `evaluate` for the POL-05 step only. Stateless evaluator otherwise.

### chain_id source

- **D-17: chain_id fetched once via `provider.get_chain_id().await`; cached as `Arc<OnceCell<u64>>` field on `ExecutorServer`.**
  - **Why cache:** chain_id NEVER changes for a given RPC URL during a server lifetime — chain forks take effect on server restart. Researcher A-8.
  - **Why lazy:** server boot must not depend on devnet liveness (Phase 4 D-04 carry-forward).
  - **Stale-cache risk:** zero in v1 single-operator runtime. Documented assumption.
  - **Pre-Sim-Loop fetch:** `tools.rs::strategy_run` calls `self.evm_provider().await?` then `self.chain_id().await?` BEFORE STEP D (policy loop) — both gates need chain_id. Same `OnceCell` initialization pattern as `evm_provider`.
  - **`ctx.evm.chainId` exposure to JS — STILL FORBIDDEN (Phase 4 D-07).** chain identity stays a host-side policy concern. Phase 5 internalizes the value; strategies remain unaware.

### Anti-pattern carry-forward

- **D-13: Eleven NON-NEGOTIABLE carry-forwards from Phase 3 + Phase 4.**

  - **(a) HR-01: Forbidden-globals scrub runs BEFORE host bindings.** Phase 5 adds NO new ctx surface (policy is host-side, never strategy-visible — researcher recommendation explicitly avoids `ctx.policy` namespace). The Phase-3 `FORBIDDEN_GLOBALS_SCRUB` site at `sandbox.rs` is untouched; regression test `sandbox_blocks_host_globals` MUST stay green. Plan 05-04 acceptance includes this assertion.

  - **(b) MR-01: No raw alloy/reqwest/serde/rusqlite text on the wire.** Every Phase-5 wire `data.detail` is either (i) one of the stable prefixes (`"policy violation: ..."`, `"simulation failed: ..."`), or (ii) a sanitized revert reason (D-19). Raw provider/serde text routes to `tracing::warn!` only. Tests model on `executor-evm/src/error.rs::display_strings_are_stable_and_wire_safe`.

  - **(c) MR-03: No silent serde fallback.** `record_decision`'s `payload_json: serde_json::to_string(&payload)?` propagates failure as `StateError::SerializationError`. Mirrors Phase-3 fix at `record_action_outcome`.

  - **(d) MR-04: Per-run monotonic `seq` for ORDER BY tie-break.** `journal_decisions.seq` mirrors `journal_logs.seq` and `journal_source_reads.seq`. Plan 05-04 acceptance: same-ms regression test asserts decisive ordering.

  - **(e) BR-01: Stable wire taxonomy reaches the wire even after JS round-trip.** Phase 5 gate runs AFTER `Sandbox::execute` returns; the JS round-trip is not in the simulation/policy path. Verification: stdio test asserts `data.kind == "simulation_failure"` (NOT `"exception"`) and `data.kind == "policy_violation"` (NOT `"exception"`). Plan 05-04 owns these tests. **Future-compat:** if a future builder helper invokes a policy hook from inside JS, the `classify_message` arms in `sandbox.rs::658-718` must extend; documented in 05-04 SUMMARY for Phase-6+ awareness.

  - **(f) BR-02: Caps enforced at JSON-output gate, not constructor.** D-12 / D-18 — Action[] length cap at `validate_strategy_output`. Raw calldata length cap (Phase 4 already enforces) re-validated at output gate. Policy file size cap enforced at `Config::policy_config()` load (NOT only at TOML parse).

  - **(g) WR-01: No `block_in_place` from inside `spawn_blocking`.** Phase 5 simulator inside the existing `tools.rs::strategy_run` `spawn_blocking` closure uses `Handle::current().block_on(simulate_one(...))` directly. Pattern locked at `sandbox.rs:1130/1141` — DO NOT regress.

  - **(h) WR-04: Sanitize attacker-controllable revert text.** Sim revert reasons routed through `executor_evm::read::sanitize_revert_reason` (D-19 promotes to `pub`). Strip control chars, cap at 256 bytes, append `…`. Test mirrors `sanitize_revert_reason_strips_control_chars_and_caps_length` at `read.rs:300`.

  - **(i) D-12 transition guard (terminal-state safety, Phase 2/3 carry).** Every Phase-5 status mutation routes through `update_run_status_with_transition` (`runs.rs:152`). NEVER use the deprecated `update_run_status`. Terminal sinks `SimulationDenied` and `PolicyDenied` join Succeeded/Failed in the terminal set.

  - **(j) D-15d / mutex discipline.** `state.blocking_lock()` released BEFORE `block_on(provider.call(...))`. Phase 5 simulator + policy evaluator drop the storage lock before any RPC, then re-acquire it for `record_decision`. Reference Phase 4 `tools.rs:259-265`.

  - **(k) MR-03 propagation for `journal_decisions.payload_json`.** `record_decision` callers serialize via `serde_json::to_string(&payload).map_err(|e| StateError::SerializationError(format!("journal_decisions.payload: {e}")))?`. Propagates, never silently empty.

### Refactor: promote sanitize_revert_reason

- **D-19: `sanitize_revert_reason` promoted from `pub(crate)` to `pub` in `executor-evm/src/read.rs`.**
  - **Why:** Phase 5's `executor-evm::simulate::simulate_one` (sibling module) needs to call it for revert-reason sanitization. The function exists at `crates/executor-evm/src/read.rs:255` (Phase-4 WR-04 fix). Sibling-module access works as `pub(crate)` ALREADY — promotion to `pub` is forward-compat for any future cross-crate consumer (e.g., a v2 broadcast watcher). **Plan 05-02 makes this change as a one-line refactor; existing `read.rs` tests stay green.**
  - **Acceptance:** `grep -c 'pub fn sanitize_revert_reason' crates/executor-evm/src/read.rs` ≥ 1 (was `pub(crate)`).

### Crate dep contract

- **D-20: `executor-policy` is alloy-free. Orchestration in `executor-mcp::tools::strategy_run` calls `executor-evm::normalize` → `executor-policy::evaluate` → `executor-evm::simulate`.**
  - **Why:** preserves Phase 4 D-02 isolation. `executor-policy` consumes `Action` (via `executor-core`), `Address`/`U256` (via `alloy-primitives` only — already a transitive dep, NOT the full `alloy` crate). The `Decision` shape passed to `evaluate` is a plain struct of `(chain_id: u64, action_index: u32, action_kind: NormalizedActionKind, to: Address, selector: Option<[u8;4]>, native_value: U256, erc20_amount: Option<U256>)`. The orchestrator builds this struct from `NormalizedAction` (which lives in `executor-evm`). `executor-policy` NEVER sees a `Provider`, `TransactionRequest`, or `DynProvider`.
  - **Dep graph (Cargo.toml-level):**
    - `executor-policy/Cargo.toml`: `executor-core = { path = "../executor-core" }`, `alloy-primitives = "1"`, `serde`, `serde_json`, `thiserror`, `toml`, `tracing`. NO `alloy`. NO `executor-evm`.
    - `executor-mcp/Cargo.toml`: adds `executor-policy = { path = "../executor-policy" }`. (Already has `executor-evm`.)
    - `executor-evm/Cargo.toml`: NO change.

</decisions>

<canonical_refs>
## Canonical References

### Project planning
- `.planning/PROJECT.md` §Constraints (sandbox isolation, EVM generality, observability, deny-by-default)
- `.planning/REQUIREMENTS.md` EXE-01..06 + POL-01..06 + STJ-05 (verbatim above)
- `.planning/ROADMAP.md` §"Phase 5: Simulation and Policy Gate" (4-plan split, success criteria)

### Phase 5 artefacts
- `05-RESEARCH.md` — alloy 2.0 simulation surface, dyn-abi normalization, policy DSL TOML schema, journal_decisions design, Pitfalls 1–14, Assumption Log A-1..A-10, Open Questions Q-1..Q-8
- `05-PATTERNS.md` — file-level analogs (mirror Phase 3 + Phase 4 conventions)
- `05-VALIDATION.md` — per-task automated verify map (sibling file)

### Prior phase artefacts (mirror conventions)
- `.planning/phases/04-evm-context-and-actions/04-CONTEXT.md` — D-NN style, alloy isolation (D-02), error code reuse precedent (D-12), MR-01/03/04 carry-forward (D-15)
- `.planning/phases/04-evm-context-and-actions/04-REVIEW-FIX.md` — WR-01 (block_in_place forbidden), WR-04 (sanitize_revert_reason), BR-01 (taxonomy reaches wire), BR-02 (cap-at-output-gate)
- `.planning/phases/04-evm-context-and-actions/04-{01,02,03,04}-PLAN.md` — task / verification / acceptance shape
- `.planning/phases/04-evm-context-and-actions/04-{01,02,03,04}-SUMMARY.md` — what Phase 4 actually delivered (Action enum, ERC20_ABI read-only, sanitize_revert_reason at read.rs:255 pub(crate), test count baseline)
- `.planning/phases/03-javascript-strategy-runner/03-CONTEXT.md` — D-NN numbering style, error-code reservations (-32011/-32017/-32018; -32019 reserved), ctx host-injection mechanism
- `.planning/phases/03-javascript-strategy-runner/03-REVIEW-FIX.md` — HR-01 / MR-01 / MR-03 / MR-04 origins

### External
- alloy 2.0 docs.rs: `Provider::call`, `Provider::get_chain_id`, `TransactionRequest::default().to(_).input(_).value(_).from(_)`
- alloy-primitives docs.rs: `Address::parse_checksummed`, `U256::from_str_radix`
- toml 0.8: `#[serde(deny_unknown_fields)]` precedent at `executor-mcp/src/config.rs:17`
- AGENTS.md line 35 — `executor-policy/` named as target crate
- EIP-55 mixed-case checksum spec (already adopted in Phase 4)
</canonical_refs>

<code_context>
## Existing Code Insights (verified in tree)

### Reusable assets (do not re-create)
- `crates/executor-evm/src/{action,erc20,read,error,dyn_abi}.rs` — Phase 4. `dry_run_abi_encode` at `action.rs:156` is the refactor target (D-03). `ERC20_ABI` at `erc20.rs:23` is read-only — D-04 adds sibling `ERC20_WRITE_ABI`. `sanitize_revert_reason` at `read.rs:255` is `pub(crate)` — D-19 promotes to `pub`. `EvmError::Encode` taxonomy at `error.rs` already supports `Cow<'static, str>` for runtime categories (Phase 4 BR-01 fix).
- `crates/executor-mcp/src/{tools,errors,validation,config,server,resources}.rs`. `STRATEGY_RUNTIME_ERROR = -32017` at `errors.rs:42`; `STRATEGY_INVALID_OUTPUT = -32018` at `errors.rs:45`. `map_evm_error` at `errors.rs:131` is the template for new `policy_violation` / `simulation_failure` factories (D-08). `validate_strategy_output` at `tools.rs:437` widens with the MAX_ACTIONS_PER_RUN cap (D-12). `policy_update` at `tools.rs:381` STAYS at `unimplemented_err("policy_update", 5)` (researcher Q-7); `policy_get` at `tools.rs:388` gets the live config body (researcher Q-6). `MAX_TAGS = 16` at `validation.rs:11` is the sibling-constant precedent for `MAX_ACTIONS_PER_RUN = 32` (D-18).
- `crates/executor-state/src/{schema,journal,store,runs}.rs`. `JournalActionOutcome::{SimulationFailure, PolicyDenied}` at `execution.rs:60-68` — already declared (Phase 3 D-06 future-lock). `phase3_emittable` at `execution.rs:76` — D-10 renames to `phase5_emittable` and widens. `record_action_outcome` gate at `journal.rs:141` consumes the predicate. `RunStatus::{SimulationDenied, PolicyDenied}` at `execution.rs:34-36` and string converters at `runs.rs:39-52` — already wired; D-10 only unblocks the transition table.
- `crates/executor-core/src/schema/policy.rs` — currently a 574B stub (Phase 1 placeholder). Plan 05-03 replaces with real `PolicyConfig` schema.
- `crates/executor-core/src/schema/execution.rs:90` — `StrategyOutcome::Actions { actions }` — D-11 widens with `decisions` field.
- `crates/strategy-js/src/sandbox.rs::execute` — Phase 3 entry point. **Phase 5 does NOT modify this file.** Policy/sim gates live in `tools.rs::strategy_run` AFTER `Sandbox::execute` returns.

### Established patterns
- **Mutex placement (Phase-3+4 carry):** Tokio Mutex<Connection>; never held across `await`. EVM RPC + sim run inside `spawn_blocking` with storage lock dropped before `block_on`. Phase 5 inherits.
- **Future-reserved enum gates:** Phase 2 D-05 RunStatus, Phase 3 D-06 JournalActionOutcome — Phase 5 unblocks the reserved variants.
- **Per-crate dep pinning:** alloy in `executor-evm/Cargo.toml`; toml in workspace; rusqlite/sha2/ulid/chrono in `executor-state/Cargo.toml`. Phase 5 adds `executor-policy/Cargo.toml` with alloy-free deps (D-20).
- **Schema-golden discipline:** every new agent-facing struct/enum gets a golden test. Plan 05-04 regenerates `StrategyOutcome.json` / `StrategyRunResponse.json` and adds 4 new goldens (`Decision.json`, `PolicyVerdict.json`, `SimulationOutcome.json`, `PolicyConfig.json`).
- **Stable wire taxonomy / `tracing::warn!` for raw text** (Phase 3 MR-01 / Phase 4 D-12 carry-forward). Plan 05-04 errors.rs extension MUST NOT regress.
- **`spawn_server_with_state` + `call_tool` test helpers** at `executor-mcp/tests/common/mod.rs` — Plan 05-04 stdio tests use directly.

### Integration points
- `ExecutorServer::new_with_config` (server.rs) — Plan 05-03 adds `policy: Arc<RwLock<Option<PolicyConfig>>>` field; Plan 05-04 wires the chain_id `OnceCell<u64>`.
- `tools.rs::strategy_run` 8-step lifecycle handler — Plan 05-04 inserts STEPS B/C/D/E/F (normalize / policy / simulate / journal_actions) BETWEEN the existing validate (STEP A) and transition-to-Succeeded (STEP H).
- `tools.rs::policy_get` (line 388) — Plan 05-03 fills the body with `serde_json::to_value(&Arc<PolicyConfig>)`. Plan 05-03 owns.
- `tools.rs::policy_update` (line 381) — STAYS at `unimplemented_err("policy_update", 5)`. v2 concern.
- `validate_strategy_output` (tools.rs:437) — Plan 05-01 (or 05-04) adds the MAX_ACTIONS_PER_RUN cap branch.
- `journal_decisions` table — additive `CREATE TABLE IF NOT EXISTS` in `schema.rs`; existing developer DBs pick it up at next `open_conn`. No migration crate (Phase 2 D-03b precedent).
- `crates/executor-mcp/src/resources.rs::read_journal` — Plan 05-04 adds `decisions: [...]` to the JSON body for `journal://{run_id}`.

</code_context>

<deferred>
## Deferred Ideas (DO NOT plan in Phase 5)

- Local signer / private-key handling (Phase 6).
- Tx broadcast / receipt waiting / tx-hash recording (Phase 6).
- Per-strategy policy binding (`strategy_register` adds `policy_id`) — v2 (researcher A-10).
- `policy_update` runtime mutation — v2 (researcher Q-7); stays `-32010`.
- Bundled simulation (`eth_callBundle`, `trace_callMany`) — v2 (researcher A-3).
- Parallel simulation — v2 (sequential is deterministic + matches journal order).
- Hot-reload of policy file — v2 (restart server to update; researcher A-6).
- Gas estimation in simulation payload — Phase 6 (signer concern).
- `ctx.evm.chainId` exposure to JS — still forbidden (Phase 4 D-07 carry-forward).
- Pinning simulation block tag to strategy-run start — v2 MEV concern (researcher §Simulation).
- `[evm.simulation_total_timeout_ms]` adaptive scaling per Action[] length — v2.
- Per-tenant providers / multi-policy / per-strategy policy — v2 (researcher A-10).
- New MCP wire codes -32019 / -32020 — DO NOT allocate (Phase 4 D-12 reuse pattern; researcher A-9 win over PATTERNS).
- EIP-1191 chain-prefixed checksums for policy addresses — Phase 4 D-11 carry; out of v1.
- Promoting `alloy` to `[workspace.dependencies]` — out of scope (D-01 / D-20 keep alloy in `executor-evm` only; researcher win over PATTERNS).
- Promoting `alloy-primitives` to `[workspace.dependencies]` — already a transitive dep; second consumer (`executor-policy`) is alloy-free for full alloy and uses `alloy-primitives` directly. Promotion is a hygiene improvement that may happen in Plan 05-01 Wave 0 if `cargo tree -p executor-policy` shows duplicate version resolution; otherwise stays per-crate-pinned.
</deferred>

---

*Phase: 05-simulation-and-policy-gate*
*Context locked: 2026-04-27 (planner-locked from 05-RESEARCH.md after /gsd-discuss-phase was skipped). Two researcher-vs-PATTERNS conflicts resolved in favour of researcher: D-01 (alloy isolation per Phase 4 D-02) and D-08 (wire-code reuse per Phase 4 D-12).*
