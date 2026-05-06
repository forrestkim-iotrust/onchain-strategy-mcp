---
phase: 04-evm-context-and-actions
artifact: CONTEXT
status: locked
gathered: 2026-04-27
mode: planner-locked   # /gsd-discuss-phase was skipped — researcher recommendations adopted as defaults
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md       # CTX-01..CTX-09
  - .planning/ROADMAP.md            # Phase 4 entry, Plans 04-01..04-04
  - .planning/phases/04-evm-context-and-actions/04-RESEARCH.md
  - .planning/phases/04-evm-context-and-actions/04-PATTERNS.md
  - .planning/phases/03-javascript-strategy-runner/03-CONTEXT.md
  - .planning/phases/03-javascript-strategy-runner/03-REVIEW-FIX.md
  - .planning/phases/03-javascript-strategy-runner/03-01-SUMMARY.md
  - .planning/phases/03-javascript-strategy-runner/03-02-SUMMARY.md
  - .planning/phases/03-javascript-strategy-runner/03-03-SUMMARY.md
  - AGENTS.md   # alloy (line 17), executor-evm/ target crate (line 36)
decisions:
  - D-01: alloy 2.0.x stack pinned to executor-evm crate (not workspace.dependencies)
  - D-02: New crate `crates/executor-evm/` (matches AGENTS.md target architecture). strategy-js stays alloy-free.
  - D-03: BigInt bridge — decimal-string for any value wider than i32. JS BigInt rejected in returns AND in builder inputs.
  - D-04: Provider strategy — single shared `Arc<DynProvider>` per ExecutorServer, lazy-init on first ctx.evm.* call, configurable RPC URL via `[evm]` config section, per-call 1s timeout under 2s wall-clock.
  - D-05: ctx.evm.readContract input shape — `{ address, abi, function, args, blockTag? }`. abi accepted as JSON string OR JS array (researcher Q2 — both, no correctness difference).
  - D-06: ctx.evm.readErc20 surface — balanceOf, decimals, symbol, name, allowance, totalSupply via thin readContract wrappers + bundled OZ-compatible ABI fragments.
  - D-07: ctx.evm.readNative surface — balance(address), blockNumber(). chainId deliberately omitted (Phase 5 policy boundary).
  - D-08: Action wire schema — extend `Action` enum with 5 new variants: ContractCall, RawCall, Erc20Transfer, Erc20Approve, NativeTransfer. All marked phase4_emittable; phase3_emittable boundary preserved. 64 KiB cap on `abi` JSON in ContractCallAction. `deny_unknown_fields` per variant.
  - D-09: Per-Action validator extension — executor-mcp/src/validation.rs widens allowlist to Phase 4 kinds; non-allowlisted kinds still get -32018 INVALID_OUTPUT. Per-kind input validation: address checksum (EIP-55 lenient), amount nonneg decimal, calldata `^0x[0-9a-fA-F]*$`.
  - D-10: ctx.units surface — parseUnits(amount, decimals) → string, formatUnits(value, decimals) → string. Lives in executor-evm; re-exposed via strategy-js host bindings.
  - D-11: ctx.address surface — isAddress(s) → bool, checksum(s) → string, zeroAddress constant. EIP-55 implementation via alloy_primitives::Address.
  - D-12: EVM error surfacing — REUSE -32017 STRATEGY_RUNTIME_ERROR with extended `data.kind` taxonomy: {evm_rpc_error, evm_decode_error, evm_revert} added alongside Phase-3 {exception, oom, timeout, stack_overflow}. Raw RPC text NEVER on wire (HR/MR-01 carry-forward) — `tracing::warn!` for raw, stable strings in `data.detail`.
  - D-13: ctx.evm.* journals one journal_source_reads row per call. Marker fields: kind="evm_read" (or kind="evm_call" for sub-classification — see decision body), target=`<address>:<function>`, payload_json={block_tag, block_number_resolved, args}.
  - D-14: Local EVM test infrastructure — `alloy::node_bindings::Anvil::new().try_spawn()` behind `--features anvil-tests` cargo gate. Tests skip cleanly when feature off OR anvil binary missing. `ANVIL_RPC_URL` env override. Counter + ERC20 bytecode shipped as `.hex` fixtures (committed).
  - D-15: Carry-forward anti-patterns from Phase 3 REVIEW-FIX (HR-01, MR-01, MR-03, MR-04).
  - D-16: Phase-3 stdio test `strategy_run_rejects_phase4_action_kind` FLIPS in 04-03 — rename to `strategy_run_accepts_contract_call` (or split into per-kind accept tests) and add per-kind rejection cases for malformed inputs.
---

# Phase 4: EVM Context and Actions — Context (Locked)

**Status:** locked. `/gsd-discuss-phase` was intentionally skipped per orchestrator brief; the researcher's recommendations in `04-RESEARCH.md` are adopted as **default-locked decisions**, deviating only where REQUIREMENTS.md, AGENTS.md, or PROJECT.md force a different choice.

This document is the agent-facing decision log for Phase 4 plans (04-01 / 04-02 / 04-03 / 04-04). Every plan's `decisions:` frontmatter MUST reference a subset of D-01..D-16 below; every implementation choice in those plans MUST be traceable here.

---

<domain>
## Phase Boundary

Strategy code can express broad EVM reads and write actions through `ctx`.

**This phase delivers:**
- New crate `executor-evm/` containing the alloy provider, dynamic-ABI read adapter, ERC20/native helpers, units helpers, address helpers, and reusable anvil test fixture.
- `ctx.evm.readContract`, `ctx.evm.readErc20.{balanceOf, allowance, decimals, symbol, name, totalSupply}`, `ctx.evm.readNative.{balance, blockNumber}` injected into the Phase-3 sandbox via the existing `CtxHost` trait extension (no replacement — purely additive).
- `ctx.actions.{contractCall, rawCall, erc20Transfer, erc20Approve, nativeTransfer}` builders. Pure synchronous validators that shape JSON the existing `validate_strategy_output` widens to accept.
- `ctx.units.{parseUnits, formatUnits}` and `ctx.address.{isAddress, checksum, zeroAddress}`.
- `Action` enum extended with five new variants (ContractCall, RawCall, Erc20Transfer, Erc20Approve, NativeTransfer) with `deny_unknown_fields` and a `phase4_emittable()` gate.
- `executor-mcp/src/validation.rs` widened to accept Phase-4 action kinds at the JSON-output gate; per-kind input validation surfaces `STRATEGY_INVALID_OUTPUT (-32018)` with stable strings.
- Extended `data.kind` taxonomy on `-32017 STRATEGY_RUNTIME_ERROR` for EVM-specific failure modes (transport, decode, revert).
- `journal_source_reads` rows per `ctx.evm.*` call (STJ-03 carry-forward — same table, new `kind` values).
- The Phase-3 `strategy_run_rejects_phase4_action_kind` test is updated (D-16): rename + add per-variant accept/reject coverage.

**This phase does NOT deliver:**
- Simulation, policy evaluation, signer integration (Phase 5/6).
- Action ABI encoding to transaction calldata (Phase 5 normalization owns this — Phase 4 builders only validate inputs by dry-run encoding then discard the bytes).
- Broadcast / receipt waiting / tx-hash recording (Phase 6).
- Fork / mainnet RPC support — devnet (anvil) only.
- Chain-id allowlists, per-run RPC budget, or any other policy gate (Phase 5).
- ABI registry / on-chain ABI resolution (deferred — V2 capability registry).
- Custom u256 BigInt JS host class with arithmetic methods (deferred — v2; Phase 4 strategies use ctx.units to keep precision in the host).

When Phase 4 ships: agent calls `strategy_run` with a strategy that reads via `ctx.evm.readContract` and returns `Action[]` containing any of the five new variants. Validation passes, the journal records the source reads + actions, and a Phase-5 simulator can pick those actions up unchanged.
</domain>

<requirements_text>
## Phase Requirements (verbatim from REQUIREMENTS.md)

| ID | Verbatim text | Source line |
|----|---------------|-------------|
| **CTX-01** | "`ctx.evm.readContract` can perform ABI-based generic contract reads." | REQUIREMENTS.md:26 |
| **CTX-02** | "`ctx.evm.erc20Balance` can read ERC20 balances." | REQUIREMENTS.md:27 |
| **CTX-03** | "`ctx.evm.erc20Allowance` can read ERC20 allowances." | REQUIREMENTS.md:28 |
| **CTX-04** | "`ctx.evm.nativeBalance` can read native token balance." | REQUIREMENTS.md:29 |
| **CTX-05** | "`ctx.actions.contractCall` can create ABI-based contract call actions." | REQUIREMENTS.md:30 |
| **CTX-06** | "`ctx.actions.rawCall` can create explicit raw calldata actions." | REQUIREMENTS.md:31 |
| **CTX-07** | "`ctx.actions.erc20Approve` and `ctx.actions.erc20Transfer` can create ERC20 actions." | REQUIREMENTS.md:32 |
| **CTX-08** | "`ctx.actions.nativeTransfer` can create native transfer actions." | REQUIREMENTS.md:33 |
| **CTX-09** | "`ctx.units` and address helpers reduce common EVM value/address mistakes." | REQUIREMENTS.md:34 |

Note on naming: REQUIREMENTS uses `ctx.evm.erc20Balance` / `ctx.evm.erc20Allowance` / `ctx.evm.nativeBalance` (flat namespace), while the locked surface in 04-RESEARCH organises them under `ctx.evm.readErc20.{balanceOf, allowance}` and `ctx.evm.readNative.{balance}`. **Both** spellings are exposed: the requirement-named flat aliases (`ctx.evm.erc20Balance`, `ctx.evm.erc20Allowance`, `ctx.evm.nativeBalance`) and the structured `readErc20` / `readNative` namespaces both resolve to the same host functions. See D-06 / D-07.
</requirements_text>

<decisions>
## Locked Decisions

### Crate stack and placement

- **D-01: alloy 2.0.x stack pinned to executor-evm only.**
  - **Why:** AGENTS.md line 17 ("alloy for EVM ABI/RPC/transaction primitives") locks the choice. ethers-rs is deprecated upstream. alloy 2.0.1 (released 2026-04-22) is the current stable line.
  - **Per-crate `[dependencies]` in `crates/executor-evm/Cargo.toml`** (NOT promoted to `[workspace.dependencies]` — Phase 2 D-03 / Phase 3 D-02 rule: promote only when ≥2 crates consume the same dep; Phase 5 will likely add `executor-mcp` as a second consumer for TransactionRequest construction, at which point the promotion happens):

    ```toml
    [dependencies]
    alloy = { version = "2.0", default-features = false, features = [
        "provider-http",
        "contract",
        "rpc-types-eth",
        "json-rpc",
        "reqwest-rustls-tls",
    ] }
    alloy-dyn-abi   = "1"
    alloy-json-abi  = "1"
    alloy-primitives = "1"
    serde = { workspace = true }
    serde_json = { workspace = true }
    thiserror = { workspace = true }
    tokio = { workspace = true }
    tracing = { workspace = true }
    url = "2"

    [features]
    default = []
    anvil-tests = []
    test-fixtures = []   # exposes tests/common/anvil_fixture.rs to Phase 5/6 via dev-dep

    [dev-dependencies]
    alloy = { version = "2.0", features = ["node-bindings"] }
    tokio = { workspace = true, features = ["rt", "macros"] }
    ```

  - **Version verification step (Plan 04-01 acceptance):** `cargo add alloy@2.0 --dry-run` then `cargo tree -p executor-evm | grep -E '^alloy v'` must show `2.0.x` (NOT 1.x). If a `2.0.2+` patch exists at planning time, take it (semver-compatible).

- **D-02: New workspace member `crates/executor-evm/`.**
  - **Why:** AGENTS.md line 36 lists `executor-evm/` as the target crate boundary. **Strategy-js source does not name `alloy` directly** (no `use alloy::*` in any strategy-js file) — Phase 3 D-02 isolation rationale carries forward. The dep graph DOES include alloy transitively via `executor-evm`, which re-exports `DynProvider`/`EvmError` for use in host-binding signatures; that re-export is zero-cost and is what D-02 actually means by "alloy stays out of strategy-js". Host bindings live in strategy-js but their bodies delegate every alloy interaction to executor-evm. (WR-02 review fix: prior wording "stays alloy-free" misread `cargo tree` as the contract; the contract is the source-level boundary.)
  - **Workspace `members` update:** root `Cargo.toml` adds `crates/executor-evm` to the existing list `["crates/executor-mcp", "crates/executor-core", "crates/executor-state", "crates/executor-signer", "crates/strategy-js"]`.
  - **Module layout** (Plan 04-01 lands the skeleton; later plans flesh out):
    ```
    crates/executor-evm/
      Cargo.toml
      src/
        lib.rs
        config.rs           # EvmConfig
        provider.rs         # build_provider, DynProvider sharing
        dyn_abi.rs          # JS-arg ↔ DynSolValue conversion (BigInt bridge)
        read.rs             # readContract entry point (used by strategy-js host binding)
        erc20.rs            # bundled ABI + readErc20 helpers (Plan 04-02)
        native.rs           # readNative helpers (Plan 04-02)
        action.rs           # builder validation helpers used by strategy-js (Plan 04-03)
        units.rs            # parseUnits / formatUnits (Plan 04-04)
        address.rs          # isAddress / checksum / zeroAddress (Plan 04-04)
        error.rs            # EvmError enum
      tests/
        common/
          mod.rs
          anvil_fixture.rs   # Plan 04-01; gated behind `anvil-tests`
        fixtures/
          counter.hex        # bytecode (committed, small)
          erc20.hex          # bytecode (committed, small)
        read_contract_anvil.rs    # Plan 04-01
        erc20_helpers_anvil.rs    # Plan 04-02
        native_helpers_anvil.rs   # Plan 04-02
        units_address.rs          # Plan 04-04
    ```

### BigInt bridge

- **D-03: Decimal-string for any value wider than i32. JS BigInt rejected in BOTH directions.**
  - **JS → Rust:** `uint8..uint32` / `int8..int32` cross as JS Number; `uint64+` / `int64+` / `uint256` cross as decimal strings (no `0x` prefix; `-` allowed for signed). Validator: `U256::from_str_radix(s, 10)` / `I256::from_str`.
  - **Rust → JS:** `DynSolValue::Uint(U256, _)` → `value.to_string()` (alloy `U256: Display` is base-10) → JSON string. Phase-3 `qjs_value_to_json` already rejects BigInt returns; Phase 4 keeps that behaviour.
  - **Why not rquickjs `BigInt`:** [VERIFIED docs.rs] rquickjs 0.11 `BigInt` exposes only `from_i64`/`from_u64`/`to_i64`. ERC20 wei routinely exceeds u64. Building a custom u256 BigInt host class is out of v1 scope.
  - **Builder error contract:** if a builder receives a JS BigInt where a string is required (e.g. `ctx.actions.erc20Transfer({ amount: 100n })`), the builder MUST throw `"amount must be a decimal string, got BigInt — use ctx.units.parseUnits(...) or pass a literal string"` (NOT a confused `qjs_value_to_json` error). Plan 04-03 includes this regression test.

### Provider strategy

- **D-04: Single shared `Arc<DynProvider>` per ExecutorServer; lazy-init on first ctx.evm.* call; configurable RPC URL.**
  - **Construction:** Lazy. `ExecutorServer` owns an `OnceCell<Arc<DynProvider>>` (or equivalent). First `ctx.evm.*` call constructs via `ProviderBuilder::new().connect_http(rpc_url).erased()`. If the agent never uses EVM, no provider is built.
  - **Why lazy:** server boot must NOT depend on devnet liveness — agents may register strategies (Phase 2) without a chain.
  - **Configuration — `[evm]` section in `ExecutorConfig`:**
    ```toml
    [evm]
    rpc_url = "http://127.0.0.1:8545"   # default
    call_timeout_ms = 1000              # per-eth_call upper bound; default 1s under 2s wall-clock
    ```
  - **Defaults if `[evm]` absent:** rpc_url=`http://127.0.0.1:8545`, call_timeout_ms=1000. Strategies that never call ctx.evm.* never trigger config validation.
  - **Per-call timeout:** `tokio::time::timeout(call_timeout, provider.call(...))`. Wall-clock 2s envelope still applies via Phase-3 `set_interrupt_handler` — but the interrupt does NOT preempt `block_on` (RESEARCH Pitfall 1), so the per-call timeout is the safety net.
  - **No retry policy.** Local anvil either responds or it's wedged.
  - **Concurrency model (RESEARCH Concurrency Plan):** alloy is async, rquickjs is sync. Inside the existing Phase-3 `spawn_blocking` closure, EVM host functions call `tokio::runtime::Handle::current().block_on(provider.call(...))`. This is safe because `spawn_blocking` runs on a non-worker thread; `block_on` parks that thread without blocking async workers.
  - **Mutex discipline carry-over:** Phase 3's `state.blocking_lock()` MUST be released BEFORE `block_on(provider.call(...))` to avoid serialising RPC calls behind the storage mutex. Plan 04-01 acceptance includes a unit-level guard.

### ctx.evm.readContract input shape

- **D-05: `{ address, abi, function, args, blockTag? }` — abi accepted as JSON string OR JS array.**
  - **Locked surface:**
    ```typescript
    ctx.evm.readContract({
      address: string,                 // EIP-55 or lowercase 0x + 40 hex
      abi: string | object[],          // JSON string OR array of fragments (researcher Q2 — both)
      function: string,                // function name; overload resolution by arg count + dyn-abi types
      args: any[],                     // JSON-encodable per Solidity types (D-03 BigInt bridge)
      blockTag?: "latest" | "pending" | number   // default "latest"
    }) → JsonValue
    ```
  - **Both abi forms:** if `abi` is a string, parse via `serde_json::from_str::<JsonAbi>`; if it's a JS array, JSON.stringify on the JS side then parse. The string path is the audit-stable form (journal records the verbatim string).
  - **Overload resolution** (RESEARCH Pitfall 4): `JsonAbi::function(name)` returns a slice; pick by argument count + type compatibility. Ambiguous (>1 hit) → host error `"function <name> has overloads; cannot disambiguate"`.
  - **Tuple vs fixed array** (RESEARCH Pitfall 10): drive conversion from the ABI's `selector_type_name()`, NOT from JSON shape. Plan 04-01 acceptance includes a tuple-arg integration test.
  - **Block tag:** Phase 4 supports `"latest"`, `"pending"`, integer block numbers. No `"safe"` / `"finalized"` until a strategy actually requests them.

### ERC20 read helpers

- **D-06: ctx.evm.readErc20 — balanceOf, allowance, decimals, symbol, name, totalSupply.**
  - **JS surface (locked):**
    ```typescript
    ctx.evm.readErc20 = {
      balanceOf(token: string, account: string, blockTag?): string,                  // wei decimal string
      allowance(token: string, owner: string, spender: string, blockTag?): string,
      decimals(token: string, blockTag?): number,                                    // u8 fits Number
      symbol(token: string, blockTag?): string,
      name(token: string, blockTag?): string,
      totalSupply(token: string, blockTag?): string,
    }
    ```
  - **Flat aliases per REQUIREMENTS naming** (CTX-02, CTX-03):
    ```typescript
    ctx.evm.erc20Balance(token, account, blockTag?)        // alias of readErc20.balanceOf
    ctx.evm.erc20Allowance(token, owner, spender, blockTag?)  // alias of readErc20.allowance
    ```
    Both routes resolve to the same host function. Plan 04-02 acceptance asserts both spellings work.
  - **Implementation:** thin wrappers around `readContract` with a hard-coded canonical ERC20 ABI fragment bundled as `&'static str` in `executor-evm/src/erc20.rs`. The ABI is OpenZeppelin-compatible (selector-stable across implementations).
  - **Caching:** none in v1. Reads are per-call.

### Native read helpers

- **D-07: ctx.evm.readNative — balance, blockNumber.**
  - **JS surface (locked):**
    ```typescript
    ctx.evm.readNative = {
      balance(account: string, blockTag?): string,    // wei decimal string — REQUIRED by CTX-04
      blockNumber(): number,                           // OPTIONAL but cheap; useful for time-window logic
    }
    ```
  - **Flat alias per CTX-04:** `ctx.evm.nativeBalance(account, blockTag?)` resolves to `readNative.balance`.
  - **Implementation:** `Provider::get_balance(addr, BlockId)` and `Provider::get_block_number()` — direct alloy calls, no ABI involved.
  - **`chainId` deliberately omitted from Phase 4.** Chain identity is a Phase-5 policy concern (POL-01). Exposing it now invites strategies to hard-code chain-conditional logic that bypasses policy.
  - **`getBlock(tag)` and `getTransactionReceipt(hash)` deferred** to Phase 6 (broadcast/receipt).

### Action wire schema

- **D-08: Extend `Action` with 5 new variants. `phase4_emittable()` gate. 64 KiB cap on ABI strings.**
  - **Definition (lands in `crates/executor-core/src/schema/action.rs`):**

    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Action {
        Noop,                                       // Phase 3
        ContractCall(ContractCallAction),           // Phase 4 — CTX-05
        RawCall(RawCallAction),                     // Phase 4 — CTX-06
        Erc20Transfer(Erc20TransferAction),         // Phase 4 — CTX-07
        Erc20Approve(Erc20ApproveAction),           // Phase 4 — CTX-07
        NativeTransfer(NativeTransferAction),       // Phase 4 — CTX-08
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub struct ContractCallAction {
        pub address: String,
        pub abi: String,                  // JSON ABI verbatim — Phase 5 re-parses
        pub function: String,
        pub args: Vec<serde_json::Value>,
        #[serde(default = "default_zero_value")]
        pub value: String,                // wei decimal string; default "0"
    }
    // RawCallAction { address, data, value=default "0" }
    // Erc20TransferAction { token, to, amount }
    // Erc20ApproveAction { token, spender, amount }
    // NativeTransferAction { to, value }

    fn default_zero_value() -> String { "0".into() }

    impl Action {
        /// Phase-4 emission gate (mirrors RunStatus::phase2_emittable / JournalActionOutcome::phase3_emittable).
        /// All five new variants emit; Phase 5+ may add e.g. MultiCall and gate it here.
        pub fn phase4_emittable(&self) -> bool { true }
    }
    ```

  - **`deny_unknown_fields` on every variant.** Forward-compat is via NEW variants, not field expansion (Phase-3 invariant).
  - **64 KiB cap on `ContractCallAction.abi`** (RESEARCH Pitfall 11). Typical ERC20 ABI ≈ 5 KiB; full DEX router ≈ 20 KiB. Constant `MAX_ABI_BYTES: usize = 64 * 1024` lives in `executor-core::schema::action` (or `executor-evm::action`). Builder enforces at construct time; serde `deserialize_with` enforces at validate-strategy-output time.
  - **Schema goldens (Plan 04-04):** `Action.json` regenerated to include all six variants. New per-variant goldens: `ContractCallAction.json`, `RawCallAction.json`, `Erc20TransferAction.json`, `Erc20ApproveAction.json`, `NativeTransferAction.json`.
  - **JournalActionOutcome wire shape unchanged** — Phase 4 still records `outcome ∈ {noop, actions, validation_error, runtime_error}` (Phase 3 D-06). The new Action variants just travel inside the `actions` payload.

### Per-Action validator widening

- **D-09: validate_strategy_output widened to accept Phase-4 kinds; per-kind input validation.**
  - **`crates/executor-mcp/src/validation.rs`:** the existing allowlist that drives `validate_strategy_output` widens to accept `kind ∈ {noop, contract_call, raw_call, erc20_transfer, erc20_approve, native_transfer}`. Non-allowlisted kinds still produce `STRATEGY_INVALID_OUTPUT (-32018)`.
  - **Per-kind input validation rules (executed by builders AND by serde deserialization on the JSON gate):**
    - **Address fields** (`address` / `to` / `token` / `spender`): try `Address::parse_checksummed(s, None)` first; if input is all-lowercase or all-uppercase 40 hex, fall back to `Address::from_str`; mixed-case-but-wrong-checksum is rejected with `"address looks checksummed but checksum is invalid"`. (RESEARCH Pitfall 5.)
    - **Calldata** (`data`): regex `^0x([0-9a-fA-F]{2})*$`, then `alloy_primitives::Bytes::from_str`. Bare-hex input (no `0x` prefix) is rejected with a hint. (RESEARCH Pitfall 6.)
    - **Amount/value** (decimal-string fields): `U256::from_str_radix(s, 10)`. Rejects negatives, non-digits, leading-zero anomalies, and JS BigInt (D-03).
    - **ABI string**: `serde_json::from_str::<JsonAbi>(s)` succeeds AND `abi.function(function_name)` non-empty AND length ≤ MAX_ABI_BYTES. Builder also dry-runs `Function::abi_encode_input(&values)` to surface arg-count / type mismatches at builder time, then **discards the encoded bytes** (Phase 5 owns canonical encoding).
  - **Failure path:** Builder errors throw a JS Error inside the sandbox → caught by `qjs_value_to_json` path or surfaced as `RuntimeError::Exception` → MCP boundary returns -32018 with stable `data.detail`.

### ctx.units / ctx.address surfaces

- **D-10: ctx.units lives in executor-evm; surface = parseUnits + formatUnits.**
  - **JS surface:**
    ```typescript
    ctx.units = {
      parseUnits(amount: string, decimals: number): string,
      formatUnits(value: string, decimals: number): string,
    }
    ```
  - **Implementation:** `parseUnits` rejects negative amounts, non-decimal strings, `decimals > 77` (U256 max precision is 78 digits). `formatUnits` operates on `U256::from_str_radix(s, 10)` then divmod by `10^decimals`, formats trailing-zero-trimmed fractional part.
  - **Why executor-evm not strategy-js:** correctness needs `alloy_primitives::U256` (78-digit precision); strategy-js stays alloy-free (D-02 isolation). Strategy-js owns the host binding; the body delegates to `executor_evm::units::{parse_units, format_units}`.

- **D-11: ctx.address — isAddress, checksum, zeroAddress.**
  - **JS surface:**
    ```typescript
    ctx.address = {
      isAddress(s: any): boolean,           // returns false for non-strings, no throw
      checksum(s: string): string,           // throws on invalid; returns EIP-55 mixed-case
      zeroAddress: "0x0000000000000000000000000000000000000000",  // constant
    }
    ```
  - `isAddress` accepts both lowercase 40-hex and EIP-55 forms; rejects mixed-case-with-bad-checksum (returns false, does NOT throw). `checksum` is strict.
  - **EIP-1191 chain-prefixed checksums:** out of scope for v1 (RESEARCH Assumption A7). `parse_checksummed(s, None)` always called.

### EVM error surfacing

- **D-12: Reuse -32017 STRATEGY_RUNTIME_ERROR with extended `data.kind` taxonomy.**
  - **No new MCP error code allocated.** -32019 stays reserved.
  - **Extended kinds** added on top of Phase-3's `{exception, oom, timeout, stack_overflow}`:
    - `evm_rpc_error` — transport-level failure (anvil down, HTTP 500, timeout firing on `tokio::time::timeout`).
    - `evm_decode_error` — host-side decode failure (wrong ABI for the data, malformed return bytes). Distinct from a contract revert — this is a host bug or strategy-supplied wrong ABI.
    - `evm_revert` — contract reverted with reason. Decoded reason (if available) appended to stable `data.detail` prefix; raw `Bytes` revert payload goes to `tracing::warn!` only.
  - **Wire discipline (HR/MR-01 carry-forward):** raw RPC text, raw revert bytes, raw alloy error strings NEVER appear in `error.message` or `data.detail`. The wire surface uses stable strings:
    - `data.detail = "evm rpc error: transport"` for transport
    - `data.detail = "evm decode error: <stable category>"` for decode
    - `data.detail = "evm revert: <decoded_reason | unknown>"` for revert
  - **`tracing::warn!`** carries the raw payload for operator debugging — same shape as Phase-3 MR-01 fix (`crates/executor-mcp/src/errors.rs:170` pattern).
  - **Implementation:** new `executor-evm::error::EvmError` enum with variants Transport / Decode / Revert / Timeout. `executor-mcp::errors::map_runtime_error` extends to recognise these (already takes a `RuntimeError` — strategy-js's `RuntimeError` gains a `EvmError` variant, OR EvmError flows through `RuntimeError::Exception` with a structured prefix; planner picks the cleaner option in 04-01).

### EVM read journaling

- **D-13: One journal_source_reads row per ctx.evm.* call.**
  - **Reuses STJ-03 table** (Phase-3 D-06). No schema change.
  - **Marker fields:**
    - `kind = "evm_read"` (covers both `readContract` and the `readErc20`/`readNative` helpers — sub-classification lives in `payload_json.helper`).
    - `target = "<address>:<function>"` for ABI-driven reads; `target = "<address>"` for `readNative.balance`; `target = "(block_number)"` for `readNative.blockNumber`.
    - `payload_json` includes: `helper` (one of `readContract`, `erc20Balance`, …), `args`, `block_tag` (raw input), `block_number_resolved` (the integer the provider actually queried, if available).
  - **Ordering (MR-04 carry-forward):** Phase 3 added a `seq INTEGER NOT NULL` column to `journal_logs` to disambiguate same-millisecond inserts. Multiple `ctx.evm.*` calls in the same ms is plausible (loop reads). Plan 04-01 (or 04-02) adds a `seq` column to `journal_source_reads` mirroring the `journal_logs` pattern: `UNIQUE (run_id, seq)`, derived via `SELECT COALESCE(MAX(seq), -1) + 1 FROM journal_source_reads WHERE run_id = ?`. Single-writer Phase-3 invariant (`Mutex<Connection>` + `spawn_blocking`) makes the SELECT-then-INSERT pair race-free.
  - **MR-03 carry-forward:** payload_json serialization MUST propagate serde failures (no silent `"[]"` fallback). `record_source_read` uses `?`-propagation through `StateError::SerializationError` (introduced in Phase 3 MR-03 fix).

### Local EVM test infrastructure

- **D-14: `alloy::node_bindings::Anvil` behind `--features anvil-tests`. Skip cleanly when missing.**
  - **`anvil-tests` cargo feature** on `executor-evm` (and on `executor-mcp` for end-to-end stdio). Default `cargo test` does NOT spawn anvil. CI runs `cargo test --workspace --features anvil-tests`.
  - **`AnvilFixture::spawn()` lives in `crates/executor-evm/tests/common/anvil_fixture.rs`.** Behaviour:
    - If `ANVIL_RPC_URL` env var is set, use that URL (no spawn). Tests that depend on specific anvil-pre-funded accounts skip in this mode.
    - Otherwise call `Anvil::new().chain_id(31337).try_spawn()`.
    - On `try_spawn` failure (anvil binary not on PATH): `eprintln!("[skip] anvil binary not on PATH; install foundry to run anvil-tests")` and return early. **Do NOT panic.** Tests detect the skip via a `#[test]` that returns `()` early.
  - **Pre-deployed contract bytecode shipped as `.hex` fixtures:**
    - `crates/executor-evm/tests/fixtures/counter.hex` — minimal Counter contract with `number()` getter and `increment()` setter (Solidity source link in a comment alongside the hex).
    - `crates/executor-evm/tests/fixtures/erc20.hex` — OpenZeppelin-compatible mock ERC20 with constructor minting to deployer.
    - Both bytecode files are committed; we do NOT call `forge` from tests.
  - **`test-fixtures` cargo feature** on `executor-evm` exposes `tests/common/anvil_fixture.rs` to Phase 5/6 via dev-dep `executor-evm = { path = "...", features = ["test-fixtures"] }`. Plan 04-01 wires the feature gate.
  - **Foundry / anvil install:** RESEARCH Environment Availability flagged anvil as the only blocker. Plan 04-01 documents `curl -L https://foundry.paradigm.xyz | bash && foundryup` in the per-developer setup; CI must install foundry before `--features anvil-tests` runs. NOT a Wave-0 install step in the plan itself (developer environment concern).

### Anti-pattern carry-forward from Phase 3 REVIEW-FIX

- **D-15: Five carry-forward rules. Every Phase 4 plan MUST honour these.**

  - **(a) HR-01: Forbidden-globals scrub runs BEFORE host bindings.** Phase 4 adds many new bindings (`ctx.evm.*`, `ctx.actions.contractCall/...`, `ctx.units.*`, `ctx.address.*`). The Phase-3 `FORBIDDEN_GLOBALS_SCRUB` eval (sandbox.rs:302-309) MUST still run BEFORE the new bindings are installed. A future intrinsic name overlapping a Phase-4 binding (hypothetical `__readContract`) cannot be allowed to silently delete a host binding. Plan 04-01 / 04-02 / 04-03 / 04-04 each end with a regression assertion that the scrub list still runs first; Plan 04-04 also asserts D-11 globals (`fs`, `process`, etc.) remain absent in the presence of the new bindings.

  - **(b) MR-01: Never echo raw error text in `error.message` / `data.detail`.** Stable taxonomy strings on the wire; raw alloy / rusqlite / quickjs error text routed through `tracing::warn!`. Phase 4 adds three new wire detail strings (D-12: `"evm rpc error: transport"`, `"evm decode error: <category>"`, `"evm revert: <decoded_reason | unknown>"`) — none of these contain raw transport text, raw HTTP body, or raw revert bytes.

  - **(c) MR-03: Never silent fallback in serde paths.** `record_source_read` payload_json serialization (D-13) and any new `record_action` payload paths (Phase 4 widens the variant set) MUST `?`-propagate serde failures via `StateError::SerializationError`. No `unwrap_or_else(|_| "[]".into())`-style swallowing.

  - **(d) MR-04: Same-ms ordering via monotonic `seq`.** D-13 adds `seq` to `journal_source_reads` mirroring `journal_logs`. Plan 04-01 acceptance includes a regression test that asserts two `ctx.evm.*` calls in the same millisecond are observably ordered via `seq`.

  - **(e) Forbidden-globals list extended for Phase 4 awareness.** The Phase-3 D-11 absent-globals list (console, fetch, setTimeout, ..., fs, Deno) MUST remain absent in Phase 4. The Phase-4 D-11 / D-10 / new bindings are ADDITIVE — they appear under `ctx.evm.*`, `ctx.units.*`, `ctx.address.*`, `ctx.actions.*` namespaces, not on `globalThis`. No Phase-4 plan introduces a top-level global.

### Phase-3 reject-test flip

- **D-16: `strategy_run_rejects_phase4_action_kind` flips in Plan 04-03.**
  - **Current behaviour** (Phase-3 stdio_handshake.rs:1276): asserts that `kind: "contract_call"` returns -32018.
  - **Phase-4 update:** Plan 04-03 renames or splits this test:
    - `strategy_run_accepts_contract_call` — well-formed `kind: "contract_call"` with valid abi/function/args → `outcome.kind == "actions"`, `outcome.actions[0].kind == "contract_call"`.
    - Per-variant rejection tests: `strategy_run_rejects_contract_call_bad_address`, `..._rejects_contract_call_oversize_abi`, `..._rejects_raw_call_bad_calldata`, `..._rejects_native_transfer_negative_value`, `..._rejects_erc20_transfer_bigint_amount`. (Exact split lives in 04-04 since 04-04 owns the comprehensive negative test fixtures; 04-03 must at minimum land the rename + one per-variant accept test.)
  - The Phase-3 test name MUST NOT be deleted without replacement — the flip is the proof that Phase 4 widened the gate.

</decisions>

<canonical_refs>
## Canonical References

### Project planning
- `.planning/PROJECT.md` §Constraints (sandbox isolation, EVM generality, observability)
- `.planning/REQUIREMENTS.md` CTX-01..09 (verbatim above)
- `.planning/ROADMAP.md` §"Phase 4: EVM Context and Actions" (4-plan split, success criteria)

### Phase 4 artefacts
- `04-RESEARCH.md` — alloy 2.0 surface, dyn-abi runtime path, BigInt convention, concurrency plan, Anvil binding, Pitfalls 1–14
- `04-PATTERNS.md` — file-level analogs (mirror Phase 3 conventions; rename research-side hints to executor-evm)
- `04-VALIDATION.md` — per-task automated verify map (sibling file)

### Prior phase artefacts (mirror conventions)
- `.planning/phases/03-javascript-strategy-runner/03-CONTEXT.md` — D-NN numbering style, error-code reservations, ctx host-injection mechanism
- `.planning/phases/03-javascript-strategy-runner/03-REVIEW-FIX.md` — HR-01 / MR-01 / MR-03 / MR-04 fixes that Phase 4 MUST NOT regress (D-15)
- `.planning/phases/03-javascript-strategy-runner/03-{01,02,03}-PLAN.md` — task / verification / acceptance shape
- `.planning/phases/03-javascript-strategy-runner/03-{01,02,03}-SUMMARY.md` — what Phase 3 actually delivered

### External
- alloy 2.0 docs.rs: ProviderBuilder, DynProvider, Provider::call, Provider::get_balance, Anvil node-bindings
- alloy-dyn-abi docs.rs: DynSolType, DynSolValue, abi_decode
- alloy-json-abi docs.rs: JsonAbi, Function::abi_encode_input / abi_decode_output
- alloy-primitives docs.rs: Address::parse_checksummed, Address::to_checksum, U256::from_str_radix
- AGENTS.md §Technology Stack (line 17 — alloy), §Architecture (line 36 — executor-evm/)
- EIP-55 mixed-case checksum spec
</canonical_refs>

<code_context>
## Existing Code Insights (verified in tree)

### Reusable assets (do not re-create)
- `crates/strategy-js/src/{lib.rs, sandbox.rs, runtime.rs, error.rs, limits.rs}` — Phase 3 sandbox + CtxHost trait. Phase 4 EXTENDS the trait (additive `evm_*`, `actions_*` etc.) or — preferred — adds a NEW `EvmHost` trait that `RuntimeContext` also implements; Plan 04-01 picks. The Phase-3 `FORBIDDEN_GLOBALS_SCRUB` mechanism (sandbox.rs:302-309) and the `block_on(...)` pattern inside `spawn_blocking` are the integration seams.
- `crates/executor-mcp/src/{tools.rs, validation.rs, errors.rs, server.rs, resources.rs, config.rs}` — Phase 1-3 wiring. Plan 04-03 widens `validation::validate_strategy_output` (currently rejects all non-Noop kinds via the Phase-3 `phase3_emittable` gate); Plan 04-01 extends `errors.rs` with the new `data.kind` taxonomy and `config.rs` with the `[evm]` section.
- `crates/executor-core/src/schema/{action.rs, execution.rs}` — Phase 1+3 schema. Plan 04-03 extends `action.rs` with the five new variants (current state: only `Noop` placeholder + comment "TODO(phase-4): ContractCall, RawCall, Erc20Approve, Erc20Transfer, NativeTransfer variants").
- `crates/executor-state/src/{schema.rs, journal.rs, store.rs}` — Phase 3 journal layer. Plan 04-01 (or 04-02) adds `seq` column to `journal_source_reads` (mirroring `journal_logs`); `record_source_read` extends to support new `kind` values without enum-gating (the table's `kind` is a plain TEXT field, future-proof by design).
- `crates/executor-mcp/tests/common/mod.rs` — `spawn_server_with_state`, `call_tool` helpers; Plan 04-03 / 04-04 stdio tests use these directly.

### Established patterns
- **Mutex placement (Phase-3 carry-over):** Tokio `Mutex` over `Connection`; never held across `await`. Every DB call goes through `spawn_blocking { state.blocking_lock(); store.<call>() }`. Phase 4 adds the alloy `block_on` inside the SAME `spawn_blocking`, but with the storage mutex DROPPED before `block_on` (D-04).
- **Future-reserved enum gates:** Phase 2 D-05 RunStatus, Phase 3 D-06 JournalActionOutcome both declared all variants at introduction with `phaseN_emittable()` gates. Plan 04-03's `Action::phase4_emittable()` follows the same pattern (D-08).
- **Per-crate dep pinning:** alloy declared only in `crates/executor-evm/Cargo.toml`; not promoted to `[workspace.dependencies]` (mirrors `executor-state/Cargo.toml:10-13` and `strategy-js/Cargo.toml` precedent).
- **Schema-golden discipline:** every new agent-facing struct/enum gets a golden test. Plan 04-04 regenerates `Action.json` and adds five per-variant goldens.
- **Stable wire taxonomy / `tracing::warn!` for raw text:** Phase-3 MR-01 fix established this. Plan 04-01's `errors.rs` extension MUST NOT regress.

### Integration points
- `ExecutorServer::new` (server.rs) — Plan 04-01 adds an `evm_provider: OnceCell<Arc<DynProvider>>` field next to the existing `state` and `runner` fields (or equivalent OnceCell-backed lazy init).
- `Sandbox::execute` (strategy-js/src/sandbox.rs) — Phase 3 injects `__ctx` after the forbidden-globals scrub. Phase 4 adds new sub-objects (`__ctx.evm`, `__ctx.units`, `__ctx.address`, plus expanded `__ctx.actions`) AT THE SAME INJECTION SITE, AFTER the scrub (D-15a).
- `validate_strategy_output` (executor-mcp/src/tools.rs:415-432) — currently maps `"noop"` and `Action[]` (only Noop variant) to `StrategyOutcome`. Plan 04-03 widens the deserialization to accept all six Action variants.
- `journal_source_reads` table (executor-state/src/schema.rs) — Phase-3 schema is forward-compatible: `kind TEXT NOT NULL`, `target TEXT NOT NULL`, `payload_json TEXT`. Phase 4 adds new `kind` values WITHOUT schema migration. The only schema change Phase 4 makes here is adding `seq INTEGER NOT NULL` + `UNIQUE (run_id, seq)` (D-15d).

</code_context>

<deferred>
## Deferred Ideas (DO NOT plan in Phase 4)

- Simulation, policy evaluation, signer integration (Phase 5/6).
- ABI encoding to transaction calldata for execution (Phase 5 normalization).
- Broadcast / receipt waiting / tx-hash recording (Phase 6).
- Multi-chain RPC support, fork tests, mainnet integration.
- Chain-id allowlists, per-run RPC budget, per-tenant providers (Phase 5+).
- Custom u256 BigInt JS host class with `+/-/*/cmp` methods (v2; v1 uses ctx.units to keep precision in host).
- ABI registry / on-chain ABI resolution (proxy resolution + Etherscan lookup) — V2 capability registry.
- `getBlock(tag)`, `getTransactionReceipt(hash)` — Phase 6 (broadcast/receipt).
- `chainId` ctx surface — Phase 5 (policy boundary owns chain identity).
- EIP-1191 chain-prefixed checksums — RSK and similar are out of v1 scope.
- Per-call retry policy / connection pooling at our layer (reqwest pools internally).
- Allocating `-32019 STRATEGY_EVM_ERROR` — D-12 reuses -32017 with extended kinds; -32019 stays reserved.
- Per-block memoization / per-run RPC cache — defer to v2 if measurements show pressure.
- Pooled providers across servers — single `Arc<DynProvider>` is enough for v1 single-agent runtime.
</deferred>

---

*Phase: 04-evm-context-and-actions*
*Context locked: 2026-04-27 (planner-locked from 04-RESEARCH.md after /gsd-discuss-phase was skipped)*
