---
phase: 05
artifact: RESEARCH
status: complete
researched: 2026-04-27
domain: EVM tx normalization + simulation + policy DSL + decision journal
confidence: HIGH
requirements:
  - EXE-01
  - EXE-02
  - EXE-03
  - EXE-04
  - EXE-05
  - EXE-06
  - POL-01
  - POL-02
  - POL-03
  - POL-04
  - POL-05
  - POL-06
  - STJ-05
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md
  - .planning/ROADMAP.md
  - .planning/phases/04-evm-context-and-actions/04-CONTEXT.md
  - .planning/phases/04-evm-context-and-actions/04-04-SUMMARY.md
  - .planning/phases/04-evm-context-and-actions/04-REVIEW-FIX.md
  - .planning/phases/03-javascript-strategy-runner/03-REVIEW-FIX.md
  - AGENTS.md  # line 35: executor-policy/ is the target crate
---

# Phase 5: Simulation and Policy Gate — Research

**Researched:** 2026-04-27
**Domain:** EVM tx normalization + per-action simulation (eth_call) + deny-by-default policy DSL + decision journal table
**Confidence:** HIGH (every recommendation traces to verified Phase 4 code, AGENTS.md line 35, alloy 2.0 idioms already in use, or REQUIREMENTS verbatim text)

## Project Constraints (from CLAUDE.md)

- GitNexus indexes the codebase. Run `gitnexus_impact` before editing any symbol; check `gitnexus_detect_changes()` before committing. Plan tasks must include these gates where they edit existing symbols (e.g., `validate_strategy_output`, `Action`, `JournalActionOutcome`, `record_action_outcome`, `ExecutorServer`).
- Forbidden globally (user instructions): `git reset --hard`, `git checkout --`, `git restore --source`, `git clean -fd`. Plan rollback strategies must use additive undo (revert commits).
- No "claude" mention in commit messages.
- Workspace lints `print_stdout`/`print_stderr`/`dbg_macro` are deny — every diagnostic goes through `tracing::*` (already enforced).

## Summary

Phase 5 inserts a two-stage gate (policy → simulation) between Phase 4's `validate_strategy_output` (which produces a `Vec<Action>`) and Phase 6's signer. Per success criterion 1 the runtime must ABI-encode actions into transaction requests; per criteria 2/3 simulation OR policy failure stops execution before signing; per criterion 4 the policy supports six dimensions (chain, contract, selector, native value, ERC20 spend, raw calldata); per criterion 5 every gate decision is journaled.

This phase is a **runtime-internal pipeline change** with **no new MCP tool** beyond the (already stubbed) `policy_get` / `policy_update`. The wire surface (the `strategy_run` response shape) gains a `decisions: [...]` array on the `outcome` so agents can see which actions were rejected and why; failures (any-deny) surface through the existing `-32017` taxonomy with new `data.kind` values (`policy_violation`, `simulation_failure`).

**Primary recommendation:** Land **`crates/executor-policy/`** as a new workspace member (AGENTS.md line 35 already names it). Put the `Action -> TransactionRequest` normalizer plus the simulation adapter inside `executor-evm` (`src/normalize.rs`, `src/simulate.rs`) — keep alloy isolated to that crate per Phase 4 D-02. `executor-policy` is alloy-free and depends only on `executor-core` types + `serde` + `toml` + `alloy-primitives` (already a transitive dep) for `Address`/`U256` decimal parsing. Gate ordering: **policy first** (cheap, deny-by-default short-circuits before any RPC), then per-action simulation, then journal each decision row, then either return success (Phase 6 picks up the approved `TransactionRequest`s) or surface the first denial as `-32017` with the new `data.kind`.

## Phase Goal Recap (verbatim from ROADMAP)

> **Goal**: No transaction can reach the signer before simulation and policy approval.
> **Depends on**: Phase 4
> **Requirements**: EXE-01, EXE-02, EXE-03, EXE-04, EXE-05, EXE-06, POL-01, POL-02, POL-03, POL-04, POL-05, POL-06, STJ-05
> **Success Criteria**:
> 1. Runtime ABI-encodes actions into transaction requests.
> 2. Simulation failures stop execution before signing.
> 3. Policy failures stop execution before signing.
> 4. Policy supports chain, contract, selector, native value, ERC20 spend, and raw calldata restrictions.
> 5. Simulation and policy decisions are journaled.
> **Plans**: 4 plans (05-01 normalize, 05-02 simulate, 05-03 policy DSL, 05-04 journal + MCP).

## Phase Requirements (verbatim from REQUIREMENTS.md)

| ID | Verbatim text | Source |
|----|---------------|--------|
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

**Note on EXE-01:** Phase 4 already validates Action shape at the JSON-output gate (`validate_strategy_output` + `dry_run_abi_encode` + `validate_action_kind_allowlisted`). EXE-01 is therefore *closeable in Phase 5 by reference* — Phase 5 reuses the existing Phase 4 validation; no new validator is needed for EXE-01 itself. The Phase 5 deliverable for EXE-01 is documenting that validation precedes normalization in the new pipeline order.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Action -> TransactionRequest normalize | `executor-evm` (new `src/normalize.rs`) | — | Owns alloy types per D-02; this is just ABI-encoding + `TransactionRequest` building. |
| Per-action simulation (eth_call) | `executor-evm` (new `src/simulate.rs`) | — | Single shared `Arc<DynProvider>` already lives here; reuses the timeout pattern from `read.rs`. |
| Policy DSL parse/eval | `crates/executor-policy/` (new crate) | `executor-core` (shared types) | AGENTS.md line 35 names it; alloy-free per D-02; plain `toml` + `serde` + `alloy-primitives` for address/U256. |
| Decision journal table | `executor-state` (extend `journal.rs` + `schema.rs`) | — | Mirrors STJ-03/04 patterns (per-run `seq`, `?`-propagated serde, MR-04 carry-forward). |
| MCP wire shape (decisions, errors) | `executor-mcp` (extend `tools.rs::strategy_run` + `errors.rs`) | `executor-core` (response struct) | Pipeline orchestration sits where Phase 4 left it. |
| Phase 5 pipeline orchestration | `executor-mcp::tools::strategy_run` | — | Same handler runs validation -> normalize -> policy -> simulate -> journal -> respond. |

## Crate Layout Recommendation (LOCKED)

**Decision:** new workspace member `crates/executor-policy/` + new files inside `crates/executor-evm/`.

```
crates/
  executor-evm/
    src/
      ... (existing Phase 4 modules)
      normalize.rs      # NEW Plan 05-01: Action -> TransactionRequest
      simulate.rs       # NEW Plan 05-02: eth_call gate w/ timeout, sequential per-action
  executor-policy/      # NEW workspace member (AGENTS.md line 35)
    Cargo.toml
    src/
      lib.rs
      model.rs          # Policy struct (chains/contracts/selectors/native/erc20/raw)
      load.rs           # TOML load + validate
      eval.rs           # evaluate_action(policy, action, chain_id) -> Decision
      decision.rs       # Decision/Verdict types + violation taxonomy strings
      error.rs          # PolicyError (parse_error, validation_error)
    tests/
      load_toml.rs
      eval_chains.rs
      eval_contracts.rs
      eval_selectors.rs
      eval_native_value.rs
      eval_erc20_spend.rs
      eval_raw_calldata.rs
  executor-state/
    src/
      schema.rs         # MODIFY: add journal_decisions table (CREATE IF NOT EXISTS)
      journal.rs        # MODIFY: record_decision + list_decisions_for_run
  executor-mcp/
    src/
      tools.rs          # MODIFY: strategy_run pipeline: validate -> normalize -> policy -> simulate -> respond
      errors.rs         # MODIFY: simulation_failure / policy_violation kinds added
      config.rs         # MODIFY: [policy] section (path to policy.toml)
      server.rs         # MODIFY: load policy at boot (Arc<Policy> field)
  executor-core/
    src/
      schema/
        execution.rs    # MODIFY: StrategyOutcome::Actions gains decisions field; new ActionDecision struct
```

**Cargo.toml workspace.members extension:** add `crates/executor-policy`.

**executor-policy `[dependencies]`** (alloy-free per Phase 4 D-02):
```toml
[dependencies]
executor-core = { path = "../executor-core" }
alloy-primitives = "1"          # already a transitive workspace dep; for Address parse + U256
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
toml = { workspace = true }
tracing = { workspace = true }
```

`executor-evm` does NOT depend on `executor-policy`. `executor-mcp` depends on both. `executor-policy` does NOT depend on `executor-evm` (it consumes `Action` via `executor-core`). This breaks any temptation to cycle.

`alloy-primitives` is ALREADY in `executor-evm`'s deps (Phase 4 04-CONTEXT D-01) and is used inside `executor-state` only as `alloy-primitives` not `alloy` — promoting it to `[workspace.dependencies]` becomes worthwhile here (Phase 4 D-01 promotion threshold = 2 consumers; Phase 5 makes it 3). Recommend planner add this promotion in 05-01 Wave 0.

## Action -> TxRequest Normalization (EXE-02)

A `TxRequest` here means alloy's `alloy::rpc::types::TransactionRequest` (already imported in `executor-evm/src/read.rs:26`). Phase 5 introduces only `to`, `data`, and `value` — gas/nonce/chainId are Phase 6's signer concerns.

**Per-variant normalization table (Plan 05-01 owns this):**

| Action variant | `to` | `data` | `value` | Notes |
|----------------|------|--------|---------|-------|
| `Noop` | — | — | — | NOT normalized. Counts as success; no journal_decisions row (or one row with `gate="noop"`, `verdict="pass"` — see "Open Questions / Q-1"). |
| `ContractCall` | `cc.address` -> `Address::from_str` | `Function::abi_encode_input(args)` (re-parse `cc.abi`, resolve overload by arg count, encode — same code path as `read_contract`) | `cc.value` -> `U256::from_str_radix` | Phase 4's `dry_run_abi_encode` already does steps 1–4 and discards the bytes; Phase 5's normalizer is the same code with the bytes RETAINED. **Refactor opportunity:** extract a shared `encode_call_input(abi, function, args) -> Result<Bytes, EvmError>` in `executor-evm/src/dyn_abi.rs` and have both `dry_run_abi_encode` and `normalize_contract_call` call it. |
| `RawCall` | `rc.address` | `Bytes::from_str(&rc.data)` (already validated by Phase 4 `validate_calldata`) | `rc.value` -> `U256` | Trivial — bytes are pre-encoded by the agent. Selector check (POL-03) extracts `data[0..4]` if `data.len() >= 4` else `None` (selector is meaningless for sub-4-byte calldata, common for fallback receivers). |
| `Erc20Transfer` | `et.token` | `function transfer(address,uint256)` selector `0xa9059cbb` + ABI-encoded `(et.to, et.amount)` | `"0"` | Selector is fixed (`0xa9059cbb`) — no ABI parse needed. Build via the bundled `executor_evm::erc20::ERC20_ABI` constant + `Function::abi_encode_input`. |
| `Erc20Approve` | `ea.token` | `approve(address,uint256)` selector `0x095ea7b3` + ABI-encoded `(ea.spender, ea.amount)` | `"0"` | Same approach as `Erc20Transfer`. **ERC20_ABI bundles ONLY read functions today** (Phase 4 04-04); planner must extend `executor-evm/src/erc20.rs::ERC20_ABI` with `transfer` and `approve` entries OR add a separate `ERC20_WRITE_ABI` constant. Recommend separate constant for clarity. |
| `NativeTransfer` | `nt.to` | `Bytes::new()` (empty `0x`) | `nt.value` -> `U256` | The simplest case. |

**Stable Phase-5 normalize-error taxonomy** (extends EvmError::Encode `category`):
- `bad_decimal_value` — value field doesn't parse as U256 (Phase 4 Encode category already catches this for amounts; we reuse the same machinery for `value` fields).
- `bad_address_to` — `to`/`token`/`address` fails address parse (Phase 4 already covers this via `validate_address`).
- `abi_*` categories from Phase 4 carry forward unchanged for `ContractCall`.

**Output type:** introduce `executor_evm::normalize::NormalizedAction { tx: TransactionRequest, source: NormalizedActionKind, selector: Option<[u8; 4]>, native_value: U256, erc20_amount: Option<U256> }`. The `source` enum echoes the variant for the policy evaluator's dispatch; `selector` is pre-extracted so the policy doesn't re-decode; `native_value` and `erc20_amount` are pre-extracted as U256 so policy thresholds compare directly.

**Length cap on Action[] (DoS guard):** Recommend `MAX_ACTIONS_PER_RUN = 32`. Rationale: a strategy returning 10_000 actions would burn 10_000 eth_calls per run. 32 covers realistic compositions (approve+swap+transfer+settle ~ 4 actions) with 8x headroom. Enforce in `validate_strategy_output` (Plan 05-01 widens this) — the cap is a Phase-5 concern but the validator is the natural enforcement site. Wire detail: `"actions[{n}] exceeds max {MAX_ACTIONS_PER_RUN}"` -> `-32018 strategy_invalid_output`. **`[ASSUMED]` — see Assumptions Log A-1 for user-confirmable threshold.**

## Simulation Adapter Design (EXE-03, EXE-04)

**API:**
```rust
// crates/executor-evm/src/simulate.rs
pub async fn simulate_one(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    tx: &TransactionRequest,
    block: BlockId,
    from: Option<Address>,
) -> Result<SimulationOutcome, EvmError>;

pub enum SimulationOutcome {
    Pass { return_bytes: Bytes, gas_estimate: Option<u64> },
    Fail { reason: SimulationFailReason, raw_for_log: String },
}

pub enum SimulationFailReason {
    Revert { decoded: Option<String> },   // re-uses Phase 4's try_extract_revert_reason
    Transport,                             // RPC failed
    Timeout,                               // tokio::time::timeout fired
}
```

**Why an outcome enum (not just `Result`):** a revert is a NORMAL simulation result that means "deny signing" (success criterion 2) — it's not an error in the sense `EvmError::Transport` is. Distinguishing the two prevents conflating "anvil down" with "contract said no".

**Provider call:** `provider.call(tx).block(block_id).await`. Verified pattern in `executor-evm/src/read.rs:121` — same code path the read adapter uses, reused unchanged.

**`from` address (the policy hot-spot):** alloy's `Provider::call` defaults `from` to `Address::ZERO` when omitted. Address-zero is a poor simulation context: `msg.sender == 0x0` triggers branches (e.g., `require(msg.sender != address(0))`) that real signers never hit. Recommendation:

| Phase | from address | Source |
|-------|--------------|--------|
| Phase 5 | Configured `[evm.simulation_from]` (default = first anvil pre-funded account `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266`) | New `[evm]` field |
| Phase 6+ | Signer's account (Phase 6 owns this; Phase 5 wiring forward-compats) | `executor-signer` |

This is a **`[ASSUMED]`** simplification — see A-2.

**Block tag:** `BlockId::latest()` for v1. Pinning to a strategy-run start block is a v2 concern (it would require ctx.evm.* reads also pin to that block to maintain consistency, which is in scope for an MEV-aware v2 — explicitly out of scope here).

**Per-action vs bundled:** **per-action**, sequential. Three reasons: (1) simulating a bundle requires either eth_callBundle (Geth-only, NOT in alloy 2.0 stable) or stateful trace_callMany (slow + node-specific); (2) bundled simulation hides individual revert reasons (a 3-call bundle that fails reports the bundle failed, not which call); (3) Phase 5 has no inter-action dependency model — agents shouldn't rely on action[1] seeing action[0]'s state changes pre-broadcast. Document this in the Phase 5 contract: "simulation is independent per action; the actual on-chain order is Phase 6's signer concern, and agents must assume each action is simulated against the head state, NOT the post-state of prior actions." **`[ASSUMED]` user confirmation — see A-3.**

**Sequential, single-thread:** runs inside the existing `spawn_blocking` closure (Phase 4 D-04 / 04-REVIEW-FIX WR-01). Iterate normalized actions, call `tokio::runtime::Handle::current().block_on(simulate_one(...))` per action. Same lock-discipline as Phase 4: the storage `Mutex<StateStore>` is **dropped** before any `block_on(eth_call)`, but the journaling write at the end of each action picks the lock back up.

**Timeout policy:** reuse `cfg.call_timeout` (default 1s, range 50ms..30s — Phase 4 04-CONTEXT D-04). With `MAX_ACTIONS_PER_RUN = 32` worst case is 32s — exceeds the Phase 3 wall-clock 2s envelope. **Recommendation:** introduce `[evm.simulation_total_timeout_ms]` separately, default 5_000ms, total across all actions; per-action timeout stays at `call_timeout`. The Phase-3 QuickJS interrupt is no longer relevant here (Phase 5 runs AFTER QuickJS execute completes — strategy code is already done; simulation is host-only orchestration). **`[ASSUMED]` — A-4.**

**Failure-modes mapping (Plan 05-02 owns):**

| Source | `SimulationOutcome` | Wire |
|--------|---------------------|------|
| `Provider::call` returns `Ok(bytes)` | `Pass { return_bytes: bytes, gas_estimate: None }` | journal pass; pipeline continues |
| `Provider::call` returns `Err(...)` and error contains `revert`/`execution reverted`/`0x08c379a0` | `Fail { reason: Revert { decoded: try_extract_revert_reason(raw) }, raw_for_log: raw }` | journal fail; first denial -> `-32017 simulation_failure` |
| `Provider::call` returns `Err(...)` (transport) | `Fail { reason: Transport, raw_for_log: raw }` | journal fail; `-32017 simulation_failure` |
| `tokio::time::timeout` fires | `Fail { reason: Timeout, raw_for_log: "tokio::time::timeout fired" }` | journal fail; `-32017 simulation_failure` |

Reuse Phase 4's `classify_provider_error` + `try_extract_revert_reason` + `sanitize_revert_reason` from `executor-evm/src/read.rs:186-275` — extract these to a shared `executor-evm/src/error.rs` helper module so both `read_contract` and `simulate_one` consume them. No new sanitization logic.

**Gas estimation:** **NOT needed for v1 simulation.** Gas is Phase 6 (signer) territory. Simulation just answers "would this revert at head state?". Recording `gas_estimate: None` in Phase 5 journal payload is a forward-compatible field for Phase 6 to populate later.

## Policy DSL (POL-01..06)

**Schema (TOML — locked):**

```toml
# policy.toml — referenced from [policy] in config.toml

# POL-01: chain id allowlist. Empty = deny all.
[chains]
allow = [31337, 1, 137, 8453]

# POL-02: per-chain contract address allowlist. Implicit deny for any chain
# not listed AT THE [chains.<id>] level even if the chain id passes [chains].
# (POL-02 grain = address; deny-by-default.)
[contracts.31337]
# Either explicit allow list, or "any" sentinel.
allow = [
  "0x5fbdb2315678afecb367f032d93f642f64180aa3",  # Counter
  "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512",  # MockERC20
]

[contracts.1]
allow = []   # mainnet locked-out by default

# POL-03: function-selector allowlist per (chain, contract). Each entry is a
# `0x`-prefixed 4-byte selector. Special "any" (case-insensitive) admits all
# selectors for that contract; useful for trusted devnet contracts.
[selectors."31337:0x5fbdb2315678afecb367f032d93f642f64180aa3"]
allow = ["0xd09de08a"]   # increment()

[selectors."31337:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"]
allow = ["any"]

# POL-04: max native value attached per single action (wei). Global (per-chain).
[native_value.31337]
max_per_action = "1000000000000000000"   # 1 ETH

# POL-05: ERC20 spend caps. (chain, token) -> max_cumulative_per_run.
# "Spend" = the AMOUNT in erc20_transfer + the AMOUNT in erc20_approve, summed
# across all actions of the run, per token. Per the success-criterion wording
# this only applies to HELPER-generated actions (Erc20Transfer / Erc20Approve);
# raw_call / contract_call that happen to call transfer/approve are NOT
# inspected here (they're constrained by [contracts] + [selectors] + [raw]).
[erc20_spend."31337:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"]
max_per_run = "1000000000000000000000"   # 1000 tokens (18 dp)

# POL-06: raw_call gate. Default DENY. Either an explicit allowlist of
# (contract, selector) pairs OR a global allow flag (for tests only).
[raw_call]
allow_global = false                      # MUST be explicit
allow = [
  # Each entry is a {chain, contract, selector} triple. Selector "any" admits
  # any 4-byte prefix (still requires 0x and 4-byte length).
  { chain = 31337, contract = "0x5fbdb2315678afecb367f032d93f642f64180aa3", selector = "0xd09de08a" },
]
```

**Why TOML:** matches the existing `[state]`, `[evm]`, `[logging]` config sections (`crates/executor-mcp/src/config.rs:18-94`). Strategists already author TOML to operate the runtime. TOML's table-of-tables is a clean fit for the "per-chain, per-contract" structure.

**Why deny-by-default:** REQUIREMENTS.md `POL-06` ("Raw calldata actions are denied unless explicitly allowed") is the only requirement that explicitly says deny-by-default, but the model only makes sense if EVERY dimension is deny-by-default — otherwise an empty `[chains]` would mean "allow any chain" which violates POL-01's spirit. Apply uniformly.

**Evaluation order (cheap-first short-circuit):**

```
for each normalized_action:
  1. CHAIN check    (POL-01) — single hashmap lookup. Wrong chain -> deny, do not check anything else.
  2. CONTRACT check (POL-02) — per-chain hashmap lookup. Not allowed -> deny.
  3. RAW gate       (POL-06) — only for RawCall variant; fast bool/triple-set lookup.
  4. SELECTOR check (POL-03) — only if selector is Some(_); per-(chain, contract) hashmap lookup.
  5. NATIVE-VALUE   (POL-04) — only if native_value > 0; single U256 compare.
  6. ERC20-SPEND    (POL-05) — only for Erc20Transfer/Erc20Approve; running U256 sum vs cap.

If any step denies: produce a Decision::Deny { rule: "<dimension>", detail: "<stable taxonomy>" }
Else: produce Decision::Allow.
```

Note POL-03 selector check applies to `RawCall` AS WELL as `ContractCall`/Erc20\*. The raw_call gate (POL-06) is the *first* gate for RawCall and is more restrictive — but selector listed under `[raw_call.allow]` ALSO needs to be in the chain-level `[selectors]` set if you want defense-in-depth. **Recommendation: do NOT require both — `[raw_call.allow]` is the source of truth for raw_call selectors; `[selectors]` only constrains `ContractCall` and Erc20\* variants.** Document this explicitly to avoid confusion. **`[ASSUMED]` — A-5.**

**Stable violation taxonomy strings** (consumed by `data.detail`):

| Rule | Detail string |
|------|---------------|
| chain_not_allowed | `"chain {chain_id} not in policy allowlist"` |
| contract_not_allowed | `"contract {address} not allowed on chain {chain_id}"` |
| selector_not_allowed | `"selector {selector_hex} not allowed for {address} on chain {chain_id}"` |
| native_value_exceeds | `"native value {value} exceeds per-action cap {cap} on chain {chain_id}"` |
| erc20_spend_exceeds | `"cumulative spend of token {token} exceeds per-run cap {cap}"` |
| raw_call_denied | `"raw_call to {address} selector {selector} not in policy allowlist"` |

These exact strings live in `executor-policy/src/decision.rs` as `&'static str` factories. Wire-safety analog of MR-01: they NEVER carry raw transport/serde text, only typed identifiers.

**Validation at policy load:**
- Every address parses via `Address::from_str` (lenient — policy authors may supply lowercase). `executor-policy::load` returns a typed `PolicyError::ValidationError` if any address is malformed.
- Every selector matches `^0x[0-9a-fA-F]{8}$` (or the literal string `"any"`).
- Every U256 cap parses as decimal-string per D-03.
- Unknown TOML fields rejected via `#[serde(deny_unknown_fields)]` on every struct (matches Phase 4 04-CONTEXT D-08).

**Hot-reload:** **NOT in v1**. Policy is loaded once at server boot from the path in `[policy]` config section. Restart server to update. Locks the wire shape around an `Arc<Policy>` in `ExecutorServer` field.

**Default policy when path absent:** load fails -> server boot fails. RATIONALE: agents must NEVER reach the runtime with a default-allow policy. If the policy file is missing, refuse to start. The error message names the missing path explicitly. (Compare with `[evm]` which defaults silently — that's safe because EVM isn't security-sensitive at boot; policy IS.) **`[ASSUMED]` — A-6.**

**Per-strategy policy:** **NOT in v1**. Single global policy. Per-strategy policies are a v2 concern (would require `strategy_register` to bind a policy id, plus a registry of named policies — out of scope).

## Journal Extension (STJ-05)

**New table `journal_decisions`** (parallel to `journal_actions`, NOT a column on it):

```sql
CREATE TABLE IF NOT EXISTS journal_decisions (
    id           TEXT PRIMARY KEY,             -- ULID
    run_id       TEXT NOT NULL REFERENCES runs(id),
    action_index INTEGER NOT NULL,             -- 0-based index in StrategyOutcome::Actions.actions
    gate         TEXT NOT NULL,                -- "policy" | "simulation"
    verdict      TEXT NOT NULL,                -- "pass" | "fail"
    rule         TEXT,                         -- stable rule name when verdict="fail"; NULL for pass
    detail       TEXT,                         -- stable taxonomy string (wire-safe); NULL for pass
    payload_json TEXT,                         -- decoded TxRequest summary OR sim outcome (NOT NULL on fail)
    recorded_at  TEXT NOT NULL,                -- RFC3339
    seq          INTEGER NOT NULL,             -- per-run monotonic (MR-04 carry-forward)
    UNIQUE (run_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_journal_decisions_run_id
    ON journal_decisions(run_id);
```

**Rows per action (success path):** 1 policy-pass + 1 simulation-pass = 2 rows. Per Phase 5 spec policy gate runs FIRST; if policy denies, simulation is SKIPPED (1 row only). If policy passes but simulation fails: 2 rows (policy pass + simulation fail). On failure, processing of remaining actions stops (the first denial in either gate stops the pipeline) — so you get up to `2 * action_index_of_failure` rows.

**Mirror MR-04 carry-forward:** per-run `seq` column (single writer = `Mutex<StateStore>` makes the SELECT-then-INSERT pair race-free; schema-level `UNIQUE (run_id, seq)` is the backstop). Mirrors `journal_logs.seq` and Phase 4's `journal_source_reads.seq` — same pattern, no novelty.

**MR-03 carry-forward:** `payload_json` serialization MUST `?`-propagate serde failures via `StateError::SerializationError` — no silent `unwrap_or("[]")`. The Phase 5 helper `record_decision` mirrors `record_action_outcome` (`crates/executor-state/src/journal.rs:135-155`).

**`payload_json` shape (per gate):**

Policy decision payload:
```json
{
  "action_kind": "erc20_transfer",
  "action": { /* echoed Action variant — same wire shape as journal_actions */ },
  "selector": "0xa9059cbb",   // null when not applicable
  "chain_id": 31337
}
```

Simulation decision payload:
```json
{
  "action_kind": "erc20_transfer",
  "tx": { "to": "0x...", "data": "0x...", "value": "0" },
  "outcome": "pass",     // or "fail"
  "fail_reason": null,    // or "revert" | "transport" | "timeout"
  "decoded_revert": null  // or sanitized string when revert decoded
}
```

**JournalActionOutcome** already has `SimulationFailure` and `PolicyDenied` variants reserved (`crates/executor-core/src/schema/execution.rs:67-68`) — Phase 5 just unblocks emission. The `phase3_emittable` gate (`execution.rs:76`) becomes effectively-deprecated; either rename to `phase5_emittable` returning `true` for ALL six variants, OR add a sibling `phase5_emittable` predicate. **Recommend** widening `phase3_emittable` to `phase5_emittable` (single-rename refactor) — the gate's role is "what's safe to emit at the current phase boundary" and Phase 5 is now the boundary. Update the call site in `executor-state/src/journal.rs:141-145`.

**Action journal vs decision journal:** `journal_actions` row stays as today (one row per RUN with the entire `Vec<Action>` in payload, outcome ∈ `actions`/`simulation_failure`/`policy_denied`/...). `journal_decisions` is per-(action, gate). Rationale: keeping both means the read shape is friendly to two different consumers — a list of "what did the run produce" (journal_actions, 1 row) and a fine-grained "which actions passed which gates" (journal_decisions, 0..2N rows). The `journal://{run_id}` resource (Phase 3 03-03) gains a `decisions` field that lists rows from this table.

**Schema migration safety:** `CREATE TABLE IF NOT EXISTS` keeps the migration idempotent (Phase 2 D-03b carry-forward). Sqlite's lack of column-level ALTER is irrelevant here — we only add a new table.

**Wave 0 task in Plan 05-04:** the schema constant `SCHEMA_SQL` in `crates/executor-state/src/schema.rs:11-79` gets the new CREATE statement appended. Existing databases on developer machines pick the new table up at next `open_conn` (the SQL is executed every open).

## MCP Error Code Plan (LOCKED — extend -32017)

**Decision:** **Reuse `-32017 STRATEGY_RUNTIME_ERROR`** with two new `data.kind` values:
- `data.kind = "policy_violation"`
- `data.kind = "simulation_failure"`

**Why not allocate -32019/-32020:** Phase 4 already established the precedent (D-12: reuse -32017 with extended kinds for `evm_rpc_error` / `evm_decode_error` / `evm_revert`). Allocating a new wire code per phase eventually pollutes the namespace; agents already dispatch on `data.kind`, not the numeric code. The phrase "runtime error" is broad enough to cover "the runtime denied the action" — these are decisions made BY the runtime DURING the run, not external errors. The 04-CONTEXT D-12 entry says "-32019 stays reserved" — Phase 5 honors that reservation.

**Data shape (extends existing `strategy_runtime_error` factory):**

```json
// Policy denial
{
  "code": "strategy_runtime_error",
  "kind": "policy_violation",
  "rule": "contract_not_allowed",
  "action_index": 1,
  "detail": "contract 0xdead... not allowed on chain 31337",
  "run_id": "01ARZ123..."
}

// Simulation failure
{
  "code": "strategy_runtime_error",
  "kind": "simulation_failure",
  "fail_reason": "revert",
  "action_index": 0,
  "decoded_revert": "ERC20: insufficient balance",
  "detail": "evm revert: ERC20: insufficient balance",
  "run_id": "01ARZ123..."
}
```

The `detail` field stays a stable wire-safe string (MR-01 carry-forward). `decoded_revert` reuses Phase 4's `sanitize_revert_reason`. The `rule` and `action_index` fields are NEW — agents key off them for retry/repair logic.

**Implementation:** add `policy_violation(action_index, rule, detail, run_id)` and `simulation_failure(action_index, fail_reason, decoded, run_id)` factory functions in `crates/executor-mcp/src/errors.rs`, sibling to `strategy_runtime_error`. Both delegate to the existing `STRATEGY_RUNTIME_ERROR` constructor with the structured `data` payload.

## Gate Ordering (LOCKED — policy first)

**Decision:** **Policy → Simulation → (Phase 6 signer)**.

Rationale (cheapest-deny-first):
1. Policy is fully synchronous, in-memory, hashmap+set lookups. Sub-microsecond per action.
2. Simulation is a network round-trip plus contract execution. 10–500ms per action.
3. A policy-denied action MUST NOT consume an eth_call slot — both for performance and for correctness (if a strategy returns 32 actions and policy denies action[5], we shouldn't pay 32 RPCs to discover 31 valid simulations of actions we'll never sign).

**Pipeline order in `tools.rs::strategy_run` (Plan 05-04 owns the orchestration):**

```text
STEP A. Phase 4 validate_strategy_output       (unchanged — closes EXE-01)
STEP B. enforce MAX_ACTIONS_PER_RUN cap        (NEW Plan 05-01)
STEP C. for each action: normalize -> (TxRequest, NormalizedAction metadata)   (NEW Plan 05-01)
STEP D. for each (idx, normalized_action):
          policy.evaluate(action, chain_id) -> Decision
          journal_decisions row (gate=policy)
          if Deny: return -32017 policy_violation. Break.
STEP E. for each (idx, normalized_action):
          simulate_one(provider, cfg, tx, latest, simulation_from)
          journal_decisions row (gate=simulation)
          if Fail: return -32017 simulation_failure. Break.
STEP F. record journal_actions outcome=actions (unchanged Phase 3 path)
STEP G. (Phase 6 will splice in signer here)
STEP H. transition to Succeeded; return strategy_run response
```

**Why two passes (D then E) rather than interleaved (policy_action[i] then sim_action[i] then policy_action[i+1]):** in v1 with no inter-action state, both orderings produce the same observable outcome on the success path. The two-pass approach makes it trivial to journal "policy passed all 5 actions, simulation failed on action[2]" — the policy-pass rows are already written before the simulation loop starts. Recommend two-pass for journal clarity. **Note:** the alternative (interleaved) would short-circuit faster on real-world workloads where simulation is more likely to fail than policy. With `MAX_ACTIONS_PER_RUN = 32` and v1's single-agent runtime, the difference is negligible. Plan 05-04 picks final.

## strategy_run Response Shape Extension

**Today (Phase 4)** the `outcome` field of `StrategyRunResponse` is `StrategyOutcome::{Noop, Actions { actions }}`. Phase 5 extends `Actions` with a `decisions` array:

```rust
// crates/executor-core/src/schema/execution.rs — MODIFY
pub enum StrategyOutcome {
    Noop,
    Actions {
        actions: Vec<Action>,
        /// Phase 5 — per-action gate verdicts. Always present on success;
        /// length == actions.len(). Each entry has both gates evaluated.
        decisions: Vec<ActionDecision>,
    },
}

pub struct ActionDecision {
    pub action_index: u32,
    pub policy: GateVerdict,
    pub simulation: GateVerdict,
}

pub enum GateVerdict {
    Pass,
    Skipped,                                     // simulation skipped because policy denied
    Fail { rule: String, detail: String },       // wire-safe stable strings only
}
```

On the success path (all gates pass for all actions): `decisions` length = `actions` length, every entry `{ policy: Pass, simulation: Pass }`.

On the failure path: the response is NOT returned; an MCP error is raised. The journal carries the partial decisions (every gate row up to and including the failing one). Agents that need to inspect failures use `journal://{run_id}`.

**Note**: the `Action::Noop` case still wraps `StrategyOutcome::Noop` (no actions, no decisions). Noop never reaches the gate pipeline.

**Schema golden update (Plan 05-04):** regenerate `StrategyOutcome.json` and `StrategyRunResponse.json` goldens (add new fields). Mirror Phase 4 04-04's `UPDATE_SCHEMAS=1` opt-in pattern.

## Concurrency Plan (carry-forward from Phase 4)

The Phase 4 04-CONTEXT D-04 + 04-REVIEW-FIX WR-01 lock the pattern: alloy is async, rquickjs is sync, so EVM RPC happens inside a `spawn_blocking` closure via `Handle::current().block_on(provider.call(...))` — NEVER via `block_in_place`.

Phase 5 INHERITS this. The simulation loop runs in the SAME `spawn_blocking` block as Phase 3's `Sandbox::execute` and Phase 4's host-binding RPCs — extending the closure's tail. Concretely:

```rust
// In tools.rs::strategy_run, the existing spawn_blocking already wraps Sandbox::execute.
// Phase 5 changes: AFTER Sandbox::execute returns, BEFORE flush, run policy + simulation
// in the same closure. The mutex discipline (drop StateStore lock before block_on) carries
// forward identically.
```

There is a temptation to spawn a NEW spawn_blocking for the simulation loop (cleaner separation). **Don't.** Two reasons: (1) the journal writes from the simulation loop need the same Mutex<StateStore>, and crossing spawn_blocking boundaries imposes a re-acquire cost without benefit; (2) Phase 4's WR-01 fix specifically forbade nested blocking-in-blocking patterns. Stay in one closure.

The simulation loop's `block_on` parks the spawn_blocking thread (which is on tokio's blocking pool, NOT a worker), so async workers stay free to handle other concurrent MCP requests. This is the same property Phase 4 relies on.

**No parallel simulation in v1.** Sequential is simpler, deterministic, and matches the journaling order. v2 can revisit.

## Test Infrastructure

Phase 4 already shipped the anvil test harness behind `--features anvil-tests` (04-CONTEXT D-14):
- `crates/executor-evm/tests/common/anvil_fixture.rs` (gated by `test-fixtures` feature)
- `crates/executor-evm/tests/fixtures/counter.hex`, `erc20.hex`

**Phase 5 reuses these unchanged.** Add:
- `crates/executor-evm/tests/normalize.rs` — pure tests for Action -> TxRequest (no anvil needed; a pre-encoded calldata golden per variant suffices). 6 variants × 2-3 cases each = ~15 tests.
- `crates/executor-evm/tests/simulate_anvil.rs` — `anvil-tests` gated. Deploy Counter, simulate `increment()` -> Pass; simulate against missing-function selector -> Fail(Revert); kill anvil mid-run -> Fail(Transport). ~5 tests.
- `crates/executor-policy/tests/load_toml.rs` — fixture TOML files in `tests/fixtures/policy/` (good_policy.toml, bad_address.toml, deny_all.toml). ~10 tests.
- `crates/executor-policy/tests/eval_*.rs` — one file per dimension, `anvil`-FREE (pure evaluator). ~30 tests.
- `crates/executor-mcp/tests/stdio_handshake.rs` — extend with: `strategy_run_denies_disallowed_chain`, `strategy_run_denies_disallowed_contract`, `strategy_run_denies_oversized_native_value`, `strategy_run_denies_raw_call_without_explicit_allow`, `strategy_run_denies_simulation_revert` (anvil-gated), `strategy_run_journals_pass_pass_for_clean_action` (anvil-gated). ~10 tests (mix anvil-gated + pure).

**Mock policy fixtures:** ship `crates/executor-mcp/tests/fixtures/policy.permissive.toml` (allows everything on chain 31337 to anvil's pre-funded contracts) for the success-path tests. Failure tests inline the policy TOML.

**Negative grid (mirror Phase 4 04-04 commit `b63b2ab`):** at least one stdio rejection test per policy dimension. For each dimension, the test (a) registers a strategy that emits the corresponding action, (b) runs with a policy that denies that dimension, (c) asserts -32017 + the expected `data.kind=policy_violation` + `data.rule` + sanitized `data.detail`. MR-01 carry-forward: inline-grep that raw `alloy::*` / `serde_json::error` / `reqwest` strings DO NOT appear on the wire.

## Pitfalls & Gotchas

### P-1: alloy's `Provider::call` ignores `from` by default

`TransactionRequest::default().to(addr).input(data)` doesn't set `from`; alloy's HTTP provider serializes a JSON-RPC `eth_call` without the `from` field, which the node interprets as `from = 0x0`. For some contracts (`require(msg.sender != 0)`, ownership-gated functions, certain proxy patterns) this changes the simulation outcome vs. real execution. Mitigation: always set `from = simulation_from_address` in the simulator, defaulting to anvil account[0] in v1. Forward-compat to Phase 6's signer (which will set `from = signer_address` once it exists).

### P-2: revert-decoder spoofing

Phase 4's WR-04 fix already sanitizes revert reasons (`crates/executor-evm/src/read.rs:255-275` — strips control chars, caps at 256 bytes, truncates at UTF-8 boundary). Phase 5 reuses this unchanged for `simulation_failure.decoded_revert`. **Do NOT skip this** — a malicious contract can revert with `"\x1b[31mEVM RPC ERROR: TRANSPORT"` to spoof a different taxonomy in a poorly-rendering log viewer. The wire prefix `evm revert:` distinguishes it; sanitization is defense-in-depth.

### P-3: ERC20 spend = approve + transfer is a one-way conservative cap

POL-05 says "max ERC20 spend for helper-generated ERC20 actions". In ERC20 semantics, an `approve` doesn't actually spend — it AUTHORIZES future spends. But for policy purposes, an approve of N tokens IS a potential spend of N tokens by the spender, and a transfer IS an actual N-token outflow. Treating them additively (approve N + transfer M = N+M against the cap) is the conservative interpretation: it can REJECT runs that wouldn't actually overspend (e.g., approve N then transfer through that same approval), but it never UNDERESTIMATES spend. v1 uses the conservative model; document explicitly. **`[ASSUMED]` — A-7.**

### P-4: selector extraction on RawCall with empty data

`RawCallAction.data = "0x"` (4-char string after prefix-strip = 0 bytes) is legal Phase-4 input — represents a "send native value with no calldata", e.g., to a contract's `receive()` function. The policy evaluator's `selector` field is `Option<[u8;4]>`; for `data.len() < 4` it MUST be `None`. The selector check (POL-03) treats `None` as a separate case: **POL-06 raw_call gate ALWAYS applies** (a no-data RawCall must still pass `[raw_call]` allowlist), but the selector-allowlist (POL-03) is skipped for `None`. Document this: a 0-byte raw_call gates only on `(chain, contract)`, not selector. Same applies to `NativeTransfer` (always selector=None).

### P-5: `dry_run_abi_encode` discards bytes; Phase 5 must NOT re-do work

Phase 4's `dry_run_abi_encode` (`crates/executor-evm/src/action.rs:156-208`) parses the ABI, resolves the overload, encodes the args, then DROPS the bytes. Phase 5's normalizer must do the same encoding work (this time keeping the bytes). **Refactor** in Plan 05-01: extract `encode_call_input(abi, function, args) -> Result<Bytes, EvmError>` from the existing flow, have `dry_run_abi_encode` call it and discard the bytes, and have the new `normalize_contract_call` call it and keep the bytes. Avoid duplication; honor MR-03 (no swallowing of encode errors).

### P-6: `-32018` already does Action[] cap rejection — don't move semantics to -32017

Phase 4's `validate_strategy_output` is the JSON-output gate; Phase 5's `MAX_ACTIONS_PER_RUN` cap belongs there too (it's still a "shape problem"). Use `-32018 strategy_invalid_output` with detail `"Action[] length {n} exceeds {max}"`, NOT `-32017`. This keeps the wire taxonomy clean: -32018 = "your strategy returned a malformed shape", -32017 = "your strategy executed but the runtime / EVM / policy / simulator rejected it". Both action-cap and existing Phase-4 `deny_unknown_fields` belong on -32018.

### P-7: ERC20Transfer/Approve normalization needs the WRITE ABI

`crates/executor-evm/src/erc20.rs:23-30` ships READ-only ABI fragments. Phase 5 needs `transfer(address,uint256)` and `approve(address,uint256)` ABI fragments to encode those calldata. Plan 05-01 either (a) extends `ERC20_ABI` with the two write functions (selector-stable and OZ-compatible), or (b) adds a sibling `ERC20_WRITE_ABI` constant. **Recommend (b)** — keeps the read ABI immutable for Phase-4 callers (`erc20_balance_of` etc.) and avoids a goldens-churn ripple. Both are valid ERC20 standard fragments; selectors `0xa9059cbb` (transfer) and `0x095ea7b3` (approve) are universal.

### P-8: chainId source for policy

The policy evaluator needs the chain id to dispatch into per-chain tables. alloy's `Provider::get_chain_id()` is a network call. **Recommendation:** cache the chain id at provider lazy-init time (the first ctx.evm.* call OR the first simulation, whichever comes first). Store as `Arc<OnceCell<u64>>` field on `ExecutorServer` next to `evm_provider`. Subsequent calls return cached. Stale-cache risk is zero in v1 because chain id NEVER changes for a given RPC URL — chain forks take effect on server restart. **`[ASSUMED]` — A-8.**

### P-9: Phase 4 D-07 deliberately omitted `chainId` from ctx — DO NOT expose it now

Phase 4 04-CONTEXT D-07 ("`chainId` deliberately omitted from Phase 4. Chain identity is a Phase-5 policy concern (POL-01)") is the upstream commitment. Phase 5 **internalizes** chain id (used by the policy evaluator, NOT exposed to strategy code). Do NOT add `ctx.evm.chainId` in Phase 5; that surface stays deferred. The chain id reaches policy via the host-side `provider.get_chain_id()` cache — strategies remain unaware.

### P-10: TOML policy author confusion: per-chain contracts allowlist

`[contracts]` is a TABLE-OF-TABLES (`[contracts.31337]`, `[contracts.1]`, ...), NOT a single list. A user who writes `[contracts]\nallow = [...]` will silently fail because `allow` lands at the top-level `contracts` table, not at any chain. Mitigation: `executor-policy::load` validates that `[contracts]` has at least one chain-keyed sub-table, and that EVERY chain in `[chains.allow]` has a corresponding `[contracts.<id>]` entry (warn if missing → load fails with a stable error). This catches the common typo at boot, not at first-action runtime.

### P-11: address normalization in policy comparisons

Policy authors might write `0xDEAD...beef` (mixed case); strategies might emit `0xdead...beef` (lowercase). Both are the same address. Mitigation: store policy addresses as `Address` (alloy-primitives type) — comparison by `==` is byte-equal, case-insensitive. The TOML parse calls `Address::from_str`; the action's address goes through the same parse before policy lookup. Verified pattern: `executor-evm/src/action.rs:67` uses `Address::from_str` lenient, no checksum strictness on the input side.

### P-12: gas_estimate field NOT in payload yet

Phase 5's `SimulationOutcome::Pass { return_bytes, gas_estimate: None }` reserves the `gas_estimate` field for Phase 6 but doesn't populate it. Resist the urge to call `provider.estimate_gas(tx).await` in Phase 5 — it's a separate JSON-RPC roundtrip and gas estimation is a Phase 6 (signer) concern. The journal payload writes `"gas_estimate": null` so Phase 6 can populate without schema change.

### P-13: Same-ms ordering in journal_decisions (MR-04)

A strategy that emits 32 actions all simulated within the same RFC3339 second is plausible. The new `journal_decisions.seq` column (per-run monotonic) is the tie-break for `ORDER BY recorded_at, seq` — mirrors `journal_logs.seq` and `journal_source_reads.seq`. Plan 05-04 acceptance: regression test that asserts two same-ms decisions are observably ordered via `seq`.

### P-14: HR-01 / MR-01 / MR-03 / MR-04 carry-forward

Phase 4 04-CONTEXT D-15 carry-forwards remain in force:
- HR-01: forbidden-globals scrub still runs before host bindings (Phase 5 adds NO new ctx surface, so the scrub site isn't touched, but still must remain — regression test already exists in `crates/strategy-js/tests/sandbox_host_globals.rs`).
- MR-01: no raw alloy/reqwest text on wire — extends to simulation+policy detail strings.
- MR-03: no silent serde fallback — extends to `record_decision` payload writes.
- MR-04: per-run monotonic `seq` — already mandated for `journal_decisions`.

## Code Examples

### TransactionRequest construction (alloy 2.0 — verified pattern in `read.rs:115-117`)

```rust
// Source: crates/executor-evm/src/read.rs:115-117 (Phase 4)
let tx = TransactionRequest::default()
    .to(addr)
    .input(Bytes::from(calldata).into());
```

For Phase 5 with a value:
```rust
// Source: alloy-rpc-types-eth 2.0 docs.rs (TransactionRequest builder)
let tx = TransactionRequest::default()
    .to(addr)
    .input(Bytes::from(calldata).into())
    .value(value)        // U256
    .from(simulation_from);
```

### Per-action simulate loop (extension of Phase 4 RPC pattern)

```rust
// Inside the existing spawn_blocking closure in tools.rs::strategy_run
let handle = tokio::runtime::Handle::current();
let mut decisions: Vec<ActionDecision> = Vec::with_capacity(normalized.len());
for (idx, na) in normalized.iter().enumerate() {
    // ... (policy gate already passed for all actions in prior loop)
    let outcome: SimulationOutcome = handle
        .block_on(executor_evm::simulate::simulate_one(
            provider.clone(),
            &evm_cfg,
            &na.tx,
            BlockId::latest(),
            Some(simulation_from),
        ))
        .map_err(|e| /* transport — counts as Fail */ )?;
    record_decision_row(&state, &run_id, idx, "simulation", &outcome).await?;
    if let SimulationOutcome::Fail { reason, .. } = &outcome {
        return Err(simulation_failure(idx, reason, &run_id));
    }
}
```

### Policy evaluator skeleton

```rust
// crates/executor-policy/src/eval.rs
pub fn evaluate(
    policy: &Policy,
    chain_id: u64,
    action: &NormalizedAction,
) -> Decision {
    // 1. POL-01 — chain.
    if !policy.chains.allow.contains(&chain_id) {
        return Decision::Deny {
            rule: "chain_not_allowed",
            detail: format!("chain {chain_id} not in policy allowlist"),
        };
    }
    // 2. POL-02 — contract.
    let contracts = match policy.contracts.get(&chain_id) {
        Some(c) => c,
        None => return Decision::Deny { rule: "contract_not_allowed", detail: ... },
    };
    if !contracts.allow.contains(&action.to) {
        return Decision::Deny { rule: "contract_not_allowed", detail: ... };
    }
    // 3. POL-06 — raw_call gate (only for RawCall variant).
    if matches!(action.source, NormalizedActionKind::RawCall) {
        if !policy.raw_call.is_allowed(chain_id, &action.to, &action.selector) {
            return Decision::Deny { rule: "raw_call_denied", detail: ... };
        }
    }
    // 4. POL-03 — selector (skipped for None and for RawCall already gated).
    if let Some(sel) = action.selector {
        if !matches!(action.source, NormalizedActionKind::RawCall) {
            // ... allowlist lookup
        }
    }
    // 5. POL-04 — native value.
    if action.native_value > U256::ZERO {
        let cap = policy.native_value.get(&chain_id).map(|c| c.max_per_action)
            .unwrap_or(U256::ZERO);
        if action.native_value > cap {
            return Decision::Deny { rule: "native_value_exceeds", detail: ... };
        }
    }
    // 6. POL-05 — ERC20 spend (caller maintains running tally across actions).
    Decision::Allow
}
```

(POL-05 cumulative tally is maintained at the call-site in `tools.rs`, not inside `evaluate` — the evaluator is stateless. The orchestrator passes `&mut HashMap<(u64, Address), U256>` for running spend.)

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| Hand-rolled JSON-RPC client + manual ABI encoding | alloy 2.0 `Provider::call` + `Function::abi_encode_input` | Already adopted in Phase 4. Phase 5 inherits. |
| Allocate new MCP error code per failure mode | Reuse -32017 with `data.kind` taxonomy | Adopted Phase 4 D-12; Phase 5 extends with `policy_violation` / `simulation_failure`. |
| Single boolean policy ("blocked address list") | Multi-dimensional deny-by-default policy | Industry standard for transaction firewalls (Fireblocks, Gnosis Safe modules, Privy policy engine all use the same shape). |
| Per-strategy policy injection at register-time | Single global policy at server boot | v1 simplification. v2 can add per-strategy bindings without breaking the wire shape (add a `policy_id` column on strategies). |
| Bundled simulation (eth_callBundle / trace_callMany) | Per-action sequential eth_call | v1 simplification. Required for v2 if MEV ordering matters. |

**Deprecated/outdated:**
- ethers-rs (replaced workspace-wide by alloy in Phase 4 D-01).
- `Provider::estimate_gas` for simulation (it's a separate concern; gas estimation belongs in Phase 6 signer).

## Assumptions Log

These claims are tagged `[ASSUMED]` and warrant user confirmation in `/gsd-discuss-phase` before becoming locked decisions:

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| **A-1** | `MAX_ACTIONS_PER_RUN = 32` is the right v1 cap | Normalization | Too low → legitimate composite strategies rejected; too high → DoS surface from a runaway agent. 32 is reasonable for v1 but the user may want a stricter cap (e.g., 8) given v1's single-operator runtime. |
| **A-2** | Default `simulation_from` is anvil account[0] (`0xf39Fd...`) | Simulation | If the production user runs against a non-anvil RPC, account[0] won't be funded and simulations may revert with `OutOfFunds` for any value-attached call. Recommend the planner make `[evm.simulation_from]` REQUIRED (no default) for non-anvil RPCs. |
| **A-3** | Per-action sequential simulation is acceptable for v1 (vs bundled) | Simulation | If user expects "transfer X then transfer Y from new balance" to simulate as a sequence, v1 sims will incorrectly use head state for both. Document this in agent-facing docs. |
| **A-4** | `[evm.simulation_total_timeout_ms]` default 5_000ms with per-action `call_timeout` (1_000ms) is appropriate | Simulation | A 32-action strategy could exhaust the wall-clock budget. Smaller cap may be safer; larger cap may be needed for slow public RPCs. |
| **A-5** | Selector POL-03 does NOT apply to `RawCall` (POL-06 is the only gate) | Policy | If the user expects defense-in-depth (raw_call selector must ALSO be in `[selectors]`), v1 will under-reject. Make explicit in `/gsd-discuss-phase`. |
| **A-6** | Server boot FAILS when policy file is missing/unreadable | Policy | If user expects an "open" default-policy mode for development, v1 will refuse to start without a `policy.toml`. Aggressive fail-closed is correct for production but may frustrate first-run experience. |
| **A-7** | ERC20 spend cap (POL-05) sums approve + transfer cumulatively | Policy | Conservative (over-rejects) — but the alternative (separate caps per operation) doubles policy surface and is harder to reason about. |
| **A-8** | Cache `chain_id` at provider lazy-init via `OnceCell<u64>`; never refresh | Pitfalls | If the underlying RPC URL is repointed at runtime (e.g., a port-forward changes target), cached chain_id becomes stale. v1 single-operator runtime accepts this. |
| **A-9** | Reuse `-32017` with new `data.kind` rather than allocating `-32019/-32020` | MCP errors | If the user expects per-failure-mode error codes (typical of legacy JSON-RPC services), they'll be surprised. Phase 4 D-12 set the precedent; Phase 5 extends it. |
| **A-10** | Single global policy (no per-strategy policy in v1) | Crate Layout | If the user expects different strategies to have different policies (common in real ops — "rebalancer can touch DEX, liquidator can touch lending only"), v1 is too coarse. Acknowledged out-of-scope. |

## Open Questions for Planner (with proposed defaults)

| # | Question | Proposed Default |
|---|----------|------------------|
| Q-1 | Does `Action::Noop` produce a `journal_decisions` row? | **NO** — Noop is a no-op, both gates are vacuously satisfied. The journal_actions row (outcome=`noop`) already records the strategy's intent; adding a decision row would clutter the gate trace. |
| Q-2 | Should the simulation gate run if policy denied any action (i.e., evaluate ALL policy first, then ALL simulation, vs short-circuit at first policy deny)? | **Short-circuit at first policy deny.** The success criterion says "stop execution before signing", not "evaluate all gates before stopping". Cheaper and the journal still captures the partial gate trace via the per-action rows. |
| Q-3 | Should the `decisions` field appear in `StrategyOutcome::Actions` even on the failure path (e.g., via a streaming response)? | **NO** — failure path returns an MCP error, not a `StrategyOutcome::Actions`. Agents inspect `journal://{run_id}` for partial decisions. Streaming responses are a v2 transport concern (rmcp 1.5 supports them but the simpler request/response model fits v1). |
| Q-4 | Does the Phase-3 `phase3_emittable` gate on `JournalActionOutcome` rename to `phase5_emittable`, or do we leave it and add a parallel? | **Rename.** The gate's semantic role is "what can be emitted at the current phase boundary"; Phase 5 IS that boundary. Single rename in `crates/executor-core/src/schema/execution.rs:76` + call-site update at `crates/executor-state/src/journal.rs:141`. |
| Q-5 | Where does `simulation_from` config live? `[evm.simulation_from]` or `[policy.simulation_from]`? | **`[evm.simulation_from]`** — it's a property of the EVM context (which account does eth_call use), not a policy decision. Mirrors the existing `[evm]` section (`crates/executor-mcp/src/config.rs:72-94`). |
| Q-6 | Should `policy_get` MCP tool return the loaded policy verbatim or a redacted summary? | **Verbatim.** The agent that owns the runtime can read the policy file directly; redacting buys nothing. Phase 4 already locked `policy_get` as a placeholder; Phase 5 fills the body with `serde_json::to_value(&Arc<Policy>)`. |
| Q-7 | Should `policy_update` work in v1 (write through to disk + reload), or stay unimplemented? | **Stay unimplemented (-32010).** Writing policy at runtime invites a class of bugs (mid-run policy change, file write race). v1 is config-file-on-restart only. Defer to v2. Keep the existing -32010 envelope. |
| Q-8 | What happens when `MAX_ACTIONS_PER_RUN` is exceeded — `-32017` or `-32018`? | **`-32018 strategy_invalid_output`** — it's a shape/size violation of the strategy's return, same category as Phase 4's `abi_oversize`. Detail string `"actions length {n} exceeds {max}"`. |

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| anvil (foundry) | simulate_anvil tests | ✓ (installed in Phase 4) | check via `anvil --version` | `--features anvil-tests` gate; tests skip cleanly if absent (Phase 4 D-14 carry-forward). |
| alloy 2.0.x | normalize / simulate | ✓ (already in `executor-evm`) | 2.0.1+ verified Phase 4 | — |
| toml | policy load | ✓ (already in workspace deps `Cargo.toml:20`) | 0.8 | — |
| alloy-primitives | policy address/U256 parse | ✓ (transitive via alloy) | 1.x | — |
| sqlite | journal_decisions | ✓ (rusqlite already in `executor-state`) | — | — |

**No new external dependencies.** Plan 05-01 acceptance: `cargo build --workspace` succeeds without any new top-level Cargo dep additions.

## Validation Architecture

(Per `.planning/config.json` `workflow.nyquist_validation: true`.)

### Test Framework
| Property | Value |
|----------|-------|
| Framework | `cargo test` (built-in) + `cargo nextest` optional |
| Config file | none (Cargo.toml workspace defaults) |
| Quick run command | `cargo test --workspace --lib` |
| Full suite command | `cargo test --workspace --features anvil-tests` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | Wave 0 needed |
|--------|----------|-----------|-------------------|---------------|
| EXE-01 | Reuse Phase 4 validate_strategy_output as gate-precursor | unit | `cargo test -p executor-mcp validate_strategy_output -x` | NO (Phase 4 covered) |
| EXE-02 | Action -> TxRequest encoding round-trip | unit | `cargo test -p executor-evm --test normalize -x` | YES — `tests/normalize.rs` |
| EXE-03 | simulate_one returns Pass for valid eth_call | integration (anvil) | `cargo test -p executor-evm --test simulate_anvil --features anvil-tests -x` | YES — `tests/simulate_anvil.rs` |
| EXE-04 | Simulation revert -> -32017 simulation_failure on wire | stdio | `cargo test -p executor-mcp --test stdio_handshake strategy_run_denies_simulation_revert --features anvil-tests` | YES |
| EXE-05 | policy.evaluate runs before signer in pipeline | unit (orchestration) | `cargo test -p executor-mcp pipeline_order` | YES — `tests/pipeline_order.rs` |
| EXE-06 | Policy denial -> -32017 policy_violation on wire | stdio | `cargo test -p executor-mcp --test stdio_handshake strategy_run_denies_disallowed_chain` | YES |
| POL-01..06 | Per-dimension evaluator | unit | `cargo test -p executor-policy` | YES — 6 test files |
| STJ-05 | Decision rows persist with seq + payload | unit | `cargo test -p executor-state journal_decisions_*` | YES — extend `tests/journal.rs` |

### Sampling Rate
- **Per task commit:** `cargo test --workspace --lib` (~3s)
- **Per wave merge:** `cargo test --workspace --features anvil-tests` (~30s)
- **Phase gate:** Full suite green + `cargo clippy --workspace --all-targets -- -D warnings` clean before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/executor-policy/Cargo.toml` + `src/{lib,model,load,eval,decision,error}.rs`
- [ ] `crates/executor-policy/tests/load_toml.rs` + 6 `eval_*.rs` files
- [ ] `crates/executor-policy/tests/fixtures/policy/*.toml`
- [ ] `crates/executor-evm/src/{normalize,simulate}.rs`
- [ ] `crates/executor-evm/tests/normalize.rs` + `tests/simulate_anvil.rs`
- [ ] Workspace `members` extension in root `Cargo.toml`
- [ ] `crates/executor-state` schema migration (CREATE TABLE journal_decisions) + `journal::record_decision` + tests
- [ ] `crates/executor-mcp/src/config.rs` `[policy]` section
- [ ] `crates/executor-mcp/src/server.rs` `policy: Arc<Policy>` field + boot-time load
- [ ] `crates/executor-mcp/src/tools.rs` strategy_run pipeline extension
- [ ] `crates/executor-mcp/src/errors.rs` `policy_violation` + `simulation_failure` factories
- [ ] `crates/executor-mcp/tests/stdio_handshake.rs` 10+ new rejection/acceptance tests
- [ ] `crates/executor-core/tests/schemas/{StrategyOutcome,StrategyRunResponse}.json` regenerated

## Security Domain

(Per AGENTS.md and PROJECT.md — `security_enforcement` is implied for v1 production-grade runtime.)

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Single-operator local runtime; agent authenticates via stdio MCP transport boundary (out of scope here). |
| V3 Session Management | no | No sessions in v1. |
| V4 Access Control | **YES** | Policy DSL is the access-control surface. Deny-by-default per dimension. Policy load failure aborts boot. |
| V5 Input Validation | **YES** | Phase 4 already covers Action shape; Phase 5 adds policy-file TOML validation (`#[serde(deny_unknown_fields)]`, address/U256 parse on every entry). |
| V6 Cryptography | n/a Phase 5 | Signer + receipt-side integrity is Phase 6. |

### Known Threat Patterns for {Rust + alloy + sqlite}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Privilege escalation via misconfigured policy | E (Elevation) | Deny-by-default at every dimension; refuse boot when policy missing; mandatory presence of every chain-id sub-table that's listed in `[chains]`. |
| Address case-folding bypass | S (Spoofing) | Parse all addresses through `Address::from_str` before equality check (case-insensitive). Reject mixed-case-bad-checksum at policy LOAD (lenient at action input is fine because action goes through Phase 4 EIP-55 lenient validator first). |
| Selector collision via overload spoofing | T (Tampering) | Selectors are 4-byte canonical hashes of the function signature. Phase 5 extracts the selector from raw calldata bytes (NOT from the agent-supplied function name) — eliminates name-based confusion. |
| ABI oversize / DoS | D (Denial) | Phase 4 64 KiB ABI cap + Phase 5 `MAX_ACTIONS_PER_RUN` cap. Each is enforced at the wire-validation gate (EXE-01 / Plan 05-01). |
| Revert-reason injection (ANSI / fake taxonomy) | I (Information disclosure / spoofing) | Phase 4 `sanitize_revert_reason` carry-forward; control-char strip + 256 byte cap. Reused unchanged in Phase 5 for `decoded_revert` field. |
| Stale chain_id cache enabling cross-chain spoof | T | v1 caches once at boot; cache invalidates on server restart. Acceptable for single-operator runtime; documented assumption A-8. |
| Journal payload serde failure swallowed → audit gap | R (Repudiation) | MR-03 carry-forward: `?`-propagate via `StateError::SerializationError`; no silent "[]" fallback. |
| Same-ms decision rows out-of-order in audit trail | R | MR-04 carry-forward: per-run `seq` column on `journal_decisions`; `UNIQUE (run_id, seq)` schema-level invariant. |

## Sources

### Primary (HIGH confidence)
- `.planning/phases/04-evm-context-and-actions/04-CONTEXT.md` — D-01..D-16 locked decisions; alloy 2.0 stack, deny-by-default precedent, MR-01/03/04 carry-forward.
- `.planning/phases/04-evm-context-and-actions/04-04-SUMMARY.md` — Phase 4 final state (Action enum 6 variants, schema goldens, 349 test count, ERC20_ABI is read-only).
- `.planning/phases/04-evm-context-and-actions/04-REVIEW-FIX.md` — WR-01 (block_in_place forbidden), WR-04 (sanitize_revert_reason), BR-01 (data.kind taxonomy on the wire), BR-02 (size cap at JSON gate).
- `.planning/REQUIREMENTS.md` — EXE-01..06, POL-01..06, STJ-05 verbatim.
- `.planning/ROADMAP.md` — Phase 5 entry, 4-plan split, success criteria.
- `AGENTS.md` line 35 — `executor-policy/` named as target crate.
- `crates/executor-evm/src/{action,read,erc20,native,error,provider,config}.rs` — Phase 4 source code; verified types, function signatures, error categories.
- `crates/executor-core/src/schema/{action,execution,policy}.rs` — verified Action enum + JournalActionOutcome variants.
- `crates/executor-state/src/{schema,journal}.rs` — verified `seq` + MR-04 pattern; record_action_outcome already accepts SimulationFailure / PolicyDenied.
- `crates/executor-mcp/src/{tools,errors,config,server,validation}.rs` — verified pipeline integration points.

### Secondary (MEDIUM confidence — verified against official source)
- alloy 2.0.x docs.rs — `Provider::call`, `Provider::get_chain_id`, `TransactionRequest::default().to(_).input(_).value(_).from(_)` builder. Cross-verified against `executor-evm/src/read.rs:115` which already uses these in production.
- toml 0.8 — `#[serde(deny_unknown_fields)]` already used at `crates/executor-mcp/src/config.rs:17`. Same idiom for policy.
- alloy-primitives 1.x — `Address::from_str`, `U256::from_str_radix(s, 10)`. Already used Phase 4.

### Tertiary (LOW confidence — needs validation)
- (none — every claim in this research is grounded in either the verified Phase 4 codebase or REQUIREMENTS verbatim text.)

## Metadata

**Confidence breakdown:**
- Crate layout & responsibility map: **HIGH** — AGENTS.md line 35 + Phase 4 D-02 isolation pattern.
- Action -> TxRequest normalization: **HIGH** — alloy types verified in Phase 4 source; per-variant table is mechanical.
- Simulation adapter: **HIGH for shape; MEDIUM for `simulation_from` default** (assumption A-2).
- Policy DSL TOML schema: **HIGH for shape** (matches existing config idiom); **MEDIUM for evaluation order detail interactions** (POL-03 vs POL-06 interaction is ASSUMED A-5).
- Journal extension: **HIGH** — direct mirror of MR-04 / STJ-04 patterns.
- MCP error reuse: **HIGH** — Phase 4 D-12 set the precedent.
- Gate ordering: **HIGH** — cheapest-first deny is canonical pattern.
- Pitfalls: **HIGH** — every pitfall traces to a verified Phase 4 fix or a documented alloy behavior.

**Research date:** 2026-04-27
**Valid until:** 2026-05-27 (30 days; Phase 4 stack is stable). If alloy 2.1 ships in this window, re-verify `Provider::call` API surface.
