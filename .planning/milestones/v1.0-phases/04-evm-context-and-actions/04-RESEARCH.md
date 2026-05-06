---
phase: 04
artifact: RESEARCH
status: complete
researched: 2026-04-27
domain: alloy EVM client + rquickjs ↔ Rust bridge for ctx.evm.* / ctx.actions.*
confidence: HIGH (alloy 2.0, rquickjs 0.11 BigInt API, dyn-abi all verified against current docs)
requirements: [CTX-01, CTX-02, CTX-03, CTX-04, CTX-05, CTX-06, CTX-07, CTX-08, CTX-09]
upstream:
  - .planning/PROJECT.md
  - .planning/REQUIREMENTS.md
  - .planning/ROADMAP.md
  - .planning/phases/03-javascript-strategy-runner/03-CONTEXT.md
  - .planning/phases/03-javascript-strategy-runner/03-01-SUMMARY.md
  - .planning/phases/03-javascript-strategy-runner/03-02-SUMMARY.md
  - .planning/phases/03-javascript-strategy-runner/03-03-SUMMARY.md
  - AGENTS.md
  - crates/strategy-js/src/{lib.rs,sandbox.rs,runtime.rs}
  - crates/executor-mcp/src/tools.rs
  - crates/executor-core/src/schema/{action.rs,execution.rs}
---

# Phase 4: EVM Context and Actions — Research

## Summary

Phase 4 wires the EVM half of `ctx` — read paths through `ctx.evm.*` and write-action builders through `ctx.actions.*` — onto the Phase-3 sandbox without breaking the locked Phase-3 contract. Reads are real RPC calls against a local devnet (anvil), writes are pure JSON builders that the existing `validate_strategy_output` already rejects today (D-08a `strategy_run_rejects_phase4_action_kind` test) and that Phase 4 must accept.

The only credible Rust EVM client today is **alloy** (`alloy-rs/alloy` v2.0.1, released 2026-04-22) — AGENTS.md line 17 already locked this choice. The runtime-ABI capability lives in **alloy-dyn-abi**: strategies hand us a JSON ABI string at run time (not compile time), so the `sol!` macro is a non-fit for `ctx.evm.readContract` and `ctx.actions.contractCall`; `alloy-dyn-abi`'s `DynSolType`/`DynSolValue` is the right surface and is what e.g. cast/foundry uses internally.

**Primary recommendation:** Pin `alloy = "2.0"` with features `["provider-http", "contract", "rpc-types-eth", "json-rpc", "node-bindings"]` plus `alloy-dyn-abi = "1"` and `alloy-json-abi = "1"`. Build a single `Arc<DynProvider>` (boxed-trait variant) at `ExecutorServer` startup, share it via `RuntimeContext`, and bridge async alloy into the sync rquickjs sandbox via `tokio::runtime::Handle::current().block_on(...)` *inside* the existing `spawn_blocking` closure. Lock `uint256` as **decimal-string** at the JS boundary (rquickjs `BigInt::to_i64` cannot represent values above i64; building a custom u256↔BigInt is out of scope for v1). Action[] gets five new `kind` variants on `executor_core::schema::action::Action` and a new `phase4_emittable()` gate mirroring Phase-3 `phase3_emittable()` lockdown.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| RPC transport (HTTP to anvil) | `executor-evm` (NEW crate) | — | Per AGENTS.md line 36 the target crate is `executor-evm`; Phase 4 creates it. Transport state (Provider) must NOT live in `strategy-js` (would leak alloy types into the sandbox crate) and NOT in `executor-mcp` (mixes RPC concerns with MCP wiring). |
| ABI parse + calldata encode | `executor-evm` | — | `alloy-dyn-abi` lives here; strategy-js never sees ABI bytes. |
| `ctx.evm.*` host bindings | `strategy-js` | `executor-evm` | strategy-js wraps the function shapes; the body delegates to `executor-evm` traits. The Phase-3 `CtxHost` trait extends, NOT replaces. |
| `ctx.actions.*` builders | `strategy-js` | — | Pure value validation + JSON construction — no RPC, no ABI encoding (that happens in Phase 5 normalization). The builder just shapes a JSON object the validator can deserialize into `Action`. |
| Action JSON validation | `executor-mcp` (extends Phase-3 `validate_strategy_output`) | `executor-core` (Action enum extension) | Already the gate; just needs new variants accepted. |
| Local devnet test fixture | `executor-evm/tests/common` | — | One reusable helper; Phase 5/6 reuse via dev-dep cross-crate. |

## User Constraints (from CONTEXT.md)

No `04-CONTEXT.md` exists yet — this research feeds `/gsd-discuss-phase` or planner-locked `04-CONTEXT.md` next. Constraints inherited from PROJECT.md and Phase 3 lockdown:

- **PROJECT.md §Constraints** — strategy JS must not access "private key, filesystem, arbitrary network, **direct RPC client**". `ctx.evm.*` is the only legal RPC surface; the alloy `Provider` is owned by the host and is NEVER exposed to JS as a value.
- **PROJECT.md §Constraints** — "EVM generality: ERC20 demo에 갇히지 않도록 generic ABI contract read/call과 raw calldata call을 지원해야 한다." → `readContract` and `rawCall` are first-class, not derived.
- **AGENTS.md line 17** — "alloy for EVM ABI/RPC/transaction primitives". No alternatives in scope.
- **AGENTS.md line 36** — `executor-evm/` is the target crate. Phase 4 creates it.
- **Phase-3 D-04 lock** — Phase-3 `ctx` surface is `strategy / run / now / log / actions.noop` only. Phase 4 ADDS to this surface; it does not remove or rename.
- **Phase-3 D-08a stdio test `strategy_run_rejects_phase4_action_kind`** — this test currently asserts that `kind:"contract_call"` returns -32018. Phase 4 must update this test (NOT delete — flip its assertion to "accepts when valid, rejects when malformed").
- **Phase-3 D-06 future-lock pattern** — `Action` enum follows the `phase3_emittable` / `phase4_emittable` gate pattern; declare ALL variants now even if v1 doesn't broadcast yet.
- **No broadcast in Phase 4.** "writes are pure Action[] builders" — Phase 5 owns `simulate`/`policy`/`encode`, Phase 6 owns sign/broadcast.

## Phase Goal Recap (from ROADMAP.md)

> Phase 4: EVM Context and Actions
> **Goal**: Strategy code can express broad EVM reads and write actions through `ctx`.
> **Depends on**: Phase 3
> **Requirements**: CTX-01, CTX-02, CTX-03, CTX-04, CTX-05, CTX-06, CTX-07, CTX-08, CTX-09
> **Success Criteria**:
> 1. `ctx.evm.readContract` reads arbitrary ABI-compatible contract methods.
> 2. ERC20/native read helpers work against a local EVM.
> 3. `contractCall`, `rawCall`, ERC20, and native actions produce validated `Action[]`.
> 4. Unit/address helpers reduce common amount/address errors.
> Plans: 04-01 Alloy provider and ABI read adapter, 04-02 ERC20/native read helpers, 04-03 Contract call, raw call, ERC20, and native action builders, 04-04 Units/address helpers and action validation fixtures.

## Phase Requirements

| ID | Description (verbatim from REQUIREMENTS.md) | Research Support |
|----|---------------------------------------------|------------------|
| CTX-01 | `ctx.evm.readContract` can perform ABI-based generic contract reads. | `alloy-dyn-abi::DynSolType` parses ABI fragments at runtime; `alloy::providers::Provider::call(&TransactionRequest)` performs `eth_call`; result decoded back to `DynSolValue` and converted to JSON. See "ctx.evm.readContract Design" section. |
| CTX-02 | `ctx.evm.erc20Balance` can read ERC20 balances. | Wraps `readContract` with hard-coded `balanceOf(address)` ABI fragment. Returns decimal-string per BigInt-bridge convention. |
| CTX-03 | `ctx.evm.erc20Allowance` can read ERC20 allowances. | Wraps `readContract` with hard-coded `allowance(address,address)` ABI fragment. |
| CTX-04 | `ctx.evm.nativeBalance` can read native token balance. | `Provider::get_balance(addr, BlockId::Latest)`. No ABI involved. |
| CTX-05 | `ctx.actions.contractCall` can create ABI-based contract call actions. | Pure builder — validates address checksum, parses ABI fragment, validates arg count, returns `{kind:"contract_call", target, abi, function, args, value?}` JSON. NO RPC, NO encoding (Phase 5 normalization owns ABI→bytes). |
| CTX-06 | `ctx.actions.rawCall` can create explicit raw calldata actions. | Builder validates address + hex-shape calldata. Returns `{kind:"raw_call", target, data, value?}`. |
| CTX-07 | `ctx.actions.erc20Approve` and `ctx.actions.erc20Transfer` can create ERC20 actions. | Builders return `{kind:"erc20_approve", token, spender, amount}` and `{kind:"erc20_transfer", token, to, amount}`. Amount is decimal-string. |
| CTX-08 | `ctx.actions.nativeTransfer` can create native transfer actions. | Builder returns `{kind:"native_transfer", to, value}`. |
| CTX-09 | `ctx.units` and address helpers reduce common EVM value/address mistakes. | `ctx.units.parseUnits(amount, decimals) → string`, `ctx.units.formatUnits(value, decimals) → string`, `ctx.address.isAddress(s) → bool`, `ctx.address.checksum(s) → string`, `ctx.address.zeroAddress: string`. All operate on decimal-strings and rely on `alloy_primitives::U256` + `Address::parse_checksummed`. |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `alloy` | `2.0` (latest stable, released 2026-04-22) [VERIFIED: github.com/alloy-rs/alloy] | Provider, contract, RPC types, transport, node-bindings | AGENTS.md line 17 locks alloy. v2.0 is the current stable line; ethers-rs is deprecated. |
| `alloy-dyn-abi` | `1` (workspace will resolve compatible) [VERIFIED: docs.rs/alloy-dyn-abi] | Runtime ABI parse + encode + decode (DynSolType/DynSolValue) | Strategy supplies ABI as JSON at run time — `sol!` macro is compile-time only and does not fit. |
| `alloy-json-abi` | `1` [VERIFIED: docs.rs/alloy-json-abi v1.5.7] | Parse JSON ABI string → `JsonAbi`/`Function`/`Param` | Companion to `alloy-dyn-abi`; `JsonAbi: Deserialize` from `serde_json::from_str`. |
| `alloy-primitives` | (transitive via alloy) | `Address`, `U256`, `Bytes`, `B256`, EIP-55 helpers | `Address::parse_checksummed`, `Address::to_checksum`, `U256::from_str_radix(_, 10)`. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio` | already-pinned `1` | `Handle::current().block_on(...)` inside `spawn_blocking` | Bridging async alloy into sync rquickjs. |
| `serde_json` | already-pinned `1` | JSON marshalling between Rust ↔ rquickjs ↔ executor-core schema | Already the lingua franca (Sandbox returns `serde_json::Value`). |
| `hex` | (transitive via alloy-primitives) | `0x`-prefixed bytes parsing for `rawCall` | Used only in builder validation (`Bytes::from_str`). |
| `regex` | already in `executor-mcp/validation.rs` | Address shape pre-check (cheap reject before alloy parse) | Mirrors `validate_strategy_id_format` pattern. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| alloy 2.0 | ethers-rs 2.x | DEPRECATED upstream; AGENTS.md forbids; ecosystem is migrating off. |
| alloy-dyn-abi | sol! macro for hard-coded ERC20 only + reject-everything-else | Loses CTX-01 / CTX-05 (generic readContract/contractCall) → blocks PROJECT.md "EVM generality" constraint. Non-starter. |
| alloy-dyn-abi | `ethabi` crate | Smaller, but pre-alloy and not maintained alongside alloy-primitives — would force a second `Address`/`U256` type. |
| `alloy-node-bindings` for tests | Manual `Command::new("anvil")` + port discovery | `Anvil::new().try_spawn()` returns `AnvilInstance` with `endpoint_url()` and Drop kills the process. Saves ~80 LOC of test infra. [VERIFIED: alloy.rs/examples/node-bindings/anvil_local_instance] |
| Block-on inside spawn_blocking | Pre-fetch all reads before JS execution | Forces strategies to declare reads upfront, breaking the imperative authoring model. Defeats CTX-01 ergonomics. |

**Installation:**

```toml
# crates/executor-evm/Cargo.toml (NEW crate)
[dependencies]
alloy = { version = "2.0", default-features = false, features = [
    "provider-http",      # HTTP transport (devnet over http://127.0.0.1:8545)
    "contract",           # CallBuilder — but we use dyn-abi path mostly
    "rpc-types-eth",      # TransactionRequest, BlockId, BlockNumberOrTag
    "json-rpc",
    "reqwest-rustls-tls", # avoid native-tls / OpenSSL dep on dev machines
] }
alloy-dyn-abi = "1"
alloy-json-abi = "1"
alloy-primitives = "1"
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
alloy = { version = "2.0", features = ["node-bindings"] }
tokio = { workspace = true, features = ["rt", "macros"] }
```

**Version verification:** Confirm before locking — `cargo add alloy@2.0 --dry-run` then `cargo tree -p executor-evm | grep alloy` must show 2.0.x not 1.x. The 2026-04-22 `2.0.1` release is what github.com/alloy-rs/alloy lists as current; if a 2.0.2+ exists at planning time, take it (semver-compatible).

**Per-crate vs. workspace dep pinning:** alloy is consumed only by `executor-evm` initially. Phase 5 simulation may pull it into `executor-mcp` indirectly (TransactionRequest construction); when it does, promote `alloy` to `[workspace.dependencies]`. Phase 4 keeps it per-crate (mirrors Phase-2 D-03 / Phase-3 D-02 rule: promote only when ≥2 crates consume).

## Provider Strategy

**Devnet only for Phase 4.** No mainnet RPC keys, no fork tests, no chain-id allow-list (that's Phase 5 POL-01). Default RPC URL is `http://127.0.0.1:8545`; configurable via `ExecutorConfig::evm_rpc_url`.

**Single shared `Arc<DynProvider>` per `ExecutorServer`.** [VERIFIED: alloy.rs guides]

```rust
// crates/executor-evm/src/provider.rs
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub struct EvmConfig {
    pub rpc_url: Url,
    pub call_timeout: Duration,    // per eth_call upper bound
}

pub async fn build_provider(cfg: &EvmConfig) -> Result<Arc<DynProvider>, EvmError> {
    let provider = ProviderBuilder::new()
        .connect_http(cfg.rpc_url.clone())
        .erased();              // → DynProvider (boxed trait, Send + Sync + Clone)
    Ok(Arc::new(provider))
}
```

**Why `DynProvider` (erased), not `impl Provider`:**
- Stored on `ExecutorServer` as a struct field — needs a concrete `Send + Sync` type.
- Reqwest's HTTP client is already pooled internally; one `Provider` per server is enough for v1 (single agent, low concurrency).
- `Arc<DynProvider>` clones cheaply; can be moved into each `spawn_blocking` closure without lifetime gymnastics.

**Send + Sync verification:** alloy's `RootProvider<Http<Client>>` is `Send + Sync + Clone` because `reqwest::Client` is. The boxed `DynProvider` preserves these bounds. We need this because `RuntimeContext` (D-04 trait extension) lives across an `await` and gets moved into `spawn_blocking`.

**Timeout policy:**
- Per-call: 5 s default (anvil is local; production fork RPCs can be slower but Phase 4 is devnet-only).
- Implemented via `tokio::time::timeout(cfg.call_timeout, provider.call(...))` wrapper, NOT alloy's transport-level timeout (gives uniform error mapping).

**Retry policy:** None in Phase 4. Local anvil either responds or it's wedged; retries hide bugs. Phase 7 (or whenever a flaky-network case arises) revisits.

**Provider lifecycle:**
- Constructed once in `ExecutorServer::new()` *before* the rmcp service loop starts.
- If construction fails (anvil not running): server still boots — `ctx.evm.*` calls then return `RuntimeError::EvmTransport` which maps to a NEW MCP error code (see "Open Questions" Q1). Server boot must NOT depend on devnet liveness; agents may register strategies without a chain.

**No connection pooling code from us.** Reqwest pools at the HTTP layer; alloy reuses the client across calls. We do not maintain our own pool.

## ctx.evm.readContract Design

**JS-facing signature (locked):**

```typescript
// Phase 4 host binding
ctx.evm.readContract({
  address: string,            // EIP-55 checksummed or lowercase 0x-prefixed
  abi: string | object,       // JSON ABI string OR parsed array of fragments
  function: string,           // function name (no argument types — dyn-abi resolves overloads via abi)
  args: any[],                // JSON-encodable per Solidity types
  blockTag?: "latest" | "pending" | number | string  // defaults "latest"
}) → JsonValue
```

**Host-side flow** (in `executor-evm`):

```
 1. Parse JSON ABI string                  ← serde_json::from_str::<JsonAbi>
 2. Find Function by name + arity          ← JsonAbi::function(name) → Vec<&Function>
                                              (dyn-abi handles overload resolution)
 3. Encode each JS-side arg → DynSolValue  ← matches Function.inputs[i].selector_type_name()
 4. Build calldata                         ← Function::abi_encode_input(&values)
 5. Construct TransactionRequest           ← .with_to(addr).with_input(calldata)
 6. Provider::call(&tx).await              ← inside Handle::current().block_on
 7. Decode return Bytes                    ← Function::abi_decode_output(&bytes)
 8. DynSolValue → serde_json::Value        ← see "BigInt Bridge Convention"
 9. Inject into JS as a normal JSON value  ← rquickjs Object/Array round-trip
```

**Key reference:** `alloy-dyn-abi`'s [docs](https://docs.rs/alloy-dyn-abi/latest/alloy_dyn_abi/) shows the `DynSolType::abi_decode` flow; `alloy-json-abi::Function` provides `abi_encode_input` / `abi_decode_output` directly. [VERIFIED: docs.rs/alloy-json-abi]

**Revert handling:** alloy returns `Err(TransportError::ErrorResp)` on revert with the error data. We surface this as `RuntimeError::EvmRevert { selector, data, decoded_reason: Option<String> }`. The strategy sees this as a thrown JS Error from `ctx.evm.readContract` — the existing `caught_to_runtime_error` path in sandbox.rs already maps thrown JS errors to `RuntimeError::Exception`; we extend it with a typed `EvmRevert` variant whose JS-visible message starts with `EVM revert:`.

**Decoding errors are NOT reverts** (host bug or wrong ABI). They surface as `RuntimeError::EvmDecode { detail }` and become a runtime error (-32017) at the MCP boundary.

**Block tag:** v1 supports `"latest"`, `"pending"`, and integer block numbers. No safe/finalized aliases until a strategy actually requests them.

## BigInt Bridge Convention (LOCKED)

**Decision: decimal-string for all uint/int wider than i32.**

### Why not rquickjs `BigInt`

[VERIFIED: docs.rs BigInt struct] rquickjs 0.11 `BigInt` exposes `from_i64`, `from_u64`, and `to_i64` only. There is **no** `from_str`, `from_u256`, or `to_string` API surface for arbitrary precision. ERC20 balances and Wei values routinely exceed `u64` (1 ETH = 10^18 wei > u64::MAX / 18). Building our own u256-aware BigInt host wrapper means writing big-decimal arithmetic *inside* the sandbox or doing many round-trips for every comparison. Both are out of scope for v1.

The Phase-3 `qjs_value_to_json` already returns `Err("BigInt is not supported in strategy returns (Pitfall 8)")` when JS code returns a BigInt. We keep that. Strategies must NOT use JS `BigInt` literals for value math.

### Why not hex strings

Hex is fine for opaque bytes (calldata, hashes, addresses) but bad for amounts: `"0x16345785d8a0000"` is 0.1 ETH and an agent reading it cannot eyeball the magnitude. Decimal strings round-trip through human review trivially.

### The convention

| Type at the JS boundary | Wire form | Validator |
|-------------------------|-----------|-----------|
| `uint8`..`uint32`, `int8`..`int32` | JS `Number` | Existing serde_json::Number |
| `uint64`+, `int64`+, `uint256`, `int256` | JS string of base-10 digits, no `0x` prefix, `-` allowed for signed | `U256::from_str_radix(s, 10)` / `I256::from_str` |
| `address` | JS string `0x` + 40 hex (any case; checksummed accepted, lowercase accepted) | `Address::parse_checksummed` if mixed case, else `Address::from_str` |
| `bytes`, `bytes32`, `bytesN` | JS string `0x` + even-length hex | `alloy_primitives::Bytes::from_str` / `B256::from_str` |
| `bool` | JS boolean | direct |
| `string` | JS string | direct |
| Tuples, fixed/dynamic arrays | Recursive — JS array of above | recursive |

**On return:** alloy returns `DynSolValue::Uint(U256, _bits)`. We convert via `value.to_string_radix(10)` (alloy primitives `U256: Display` is base 10) and emit a JSON string. Strategy reads `const balance = ctx.evm.readErc20.balanceOf(token, account); // "1234500000000000000"` and uses `ctx.units.formatUnits(balance, 18)` to get a human form.

**`ctx.units` is the bridge.** Strategies do not do arithmetic on these strings directly in JS (would lose precision via JS Number). `ctx.units.parseUnits("1.5", 18) → "1500000000000000000"` and `ctx.units.formatUnits("1500000000000000000", 18) → "1.5"` keep precision in the host.

**JS Number safety floor:** Strategies that *do* coerce a u32-fits balance to Number (e.g., `decimals` returns a u8) get Number normally. Anything `> 2^53 - 1` is locked to string. Pitfall: if a strategy calls `Number(balance)` on a 1e18 wei string, it silently loses precision. We document this in Phase 7 prompts.

**Future direction (v2):** A custom `ctx.bigint(s)` host class wrapping U256 with `+/-/*/cmp` host methods. Out of scope for v1.

## ctx.evm.readErc20 Surface

**JS-facing (locked):**

```typescript
ctx.evm.readErc20 = {
  balanceOf(token: string, account: string, blockTag?): string,        // wei string
  allowance(token: string, owner: string, spender: string, blockTag?): string,
  decimals(token: string, blockTag?): number,                           // 0..255 fits Number
  symbol(token: string, blockTag?): string,
  totalSupply(token: string, blockTag?): string,
}
```

**Implementation:** Each helper is a thin wrapper around `readContract` with a hard-coded ABI fragment. We bundle the canonical ERC20 ABI fragments as a static `&str` JSON in `executor-evm/src/erc20.rs`; the helper names map 1:1 to function selectors.

**Caching:** None in v1. Reads are per-call, no per-run memo, no cross-run cache. Per-block memoization is tempting but would couple Phase 4 to a block-tag tracking model; defer to v2 if measurements show RPC pressure. Phase-3 `RuntimeContext` is per-run anyway.

**CTX-02/03 mapping:** REQUIREMENTS.md only mandates `erc20Balance` and `erc20Allowance`. `decimals`, `symbol`, `totalSupply` are added because (a) they are trivial extensions of the same machinery, (b) `formatUnits` is useless without `decimals()`, (c) Phase 7 example strategies need them. They are NOT separately gated requirements — listed as in-scope-but-not-mandated in PLAN 04-02 acceptance.

## ctx.evm.readNative Surface

**JS-facing (minimal, per CTX-04 only):**

```typescript
ctx.evm.readNative = {
  balance(account: string, blockTag?): string,            // wei string — REQUIRED by CTX-04
  blockNumber(): number,                                  // OPTIONAL — useful for time-window logic
  // chainId(): number — DEFER to Phase 5 (policy boundary owns chain identity)
}
```

**Implementation:** `Provider::get_balance(addr, BlockId)` and `Provider::get_block_number()` are direct alloy calls — no ABI involved. Returns are converted via the BigInt convention above.

**`chainId` is deliberately omitted from Phase 4.** The chain identity is a Phase-5 policy concern (POL-01: "Policy can restrict allowed chain IDs"). Exposing it here lets strategies hard-code chain-conditional logic that bypasses policy. Phase 5 may inject `ctx.chain.id` separately.

**`getBlock(tag)` and `getTransactionReceipt(hash)` deferred.** Neither is in CTX-04. Phase 6 (broadcast/receipt) will revisit when receipt-conditional strategies become a real use case.

## ctx.actions.* Builders Surface

All builders are **pure synchronous functions**. No RPC. No alloy `Provider` access from inside the builder. Their job: validate inputs cheaply (address checksum, amount shape, hex shape) and construct a JSON object that round-trips through `validate_strategy_output` into a strongly-typed `Action::*`. The actual ABI encoding (calldata bytes) happens in **Phase 5 normalization**, not here.

**Why builders, not direct JS-side object construction?**
- Validation gate: bad inputs fail at builder call time with an actionable message, not at `validate_strategy_output` time.
- API discoverability: `ctx.actions.contractCall({...})` is greppable; a free-form `{kind:"contract_call", ...}` object literal is not.
- Future-proofing: when v2 adds policy hints (e.g., `ctx.actions.erc20Transfer({...}).withGasHint(50000)`), the builder is the natural extension point.

**JS-facing (locked):**

```typescript
ctx.actions = {
  // Phase-3 carry-over (D-04)
  noop(): "noop",

  // CTX-05 — Phase 4 NEW
  contractCall({
    address: string,         // target contract (EIP-55 or lowercase)
    abi: string | object,    // JSON ABI containing the function
    function: string,        // function name
    args: any[],
    value?: string,          // wei decimal-string; default "0"
  }): { kind: "contract_call", ... },

  // CTX-06 — Phase 4 NEW
  rawCall({
    address: string,
    data: string,            // 0x-prefixed even-length hex
    value?: string,
  }): { kind: "raw_call", ... },

  // CTX-07 — Phase 4 NEW
  erc20Transfer({
    token: string,           // ERC20 contract address
    to: string,              // recipient EOA / contract
    amount: string,          // wei decimal-string
  }): { kind: "erc20_transfer", ... },

  erc20Approve({
    token: string,
    spender: string,
    amount: string,          // wei decimal-string; "0" allowed (revoke)
  }): { kind: "erc20_approve", ... },

  // CTX-08 — Phase 4 NEW
  nativeTransfer({
    to: string,
    value: string,           // wei decimal-string; non-negative
  }): { kind: "native_transfer", ... },
}
```

**Validation rules per builder** (sketch — Plan 04-03 nails per-field detail):

| Field | Validator | Failure mode |
|-------|-----------|--------------|
| `address` / `to` / `token` / `spender` | `Address::parse_checksummed(s, None)` if mixed case, else `Address::from_str` | Throw JS Error `"invalid address: <reason>"` |
| `data` | regex `^0x([0-9a-fA-F]{2})*$`, then `Bytes::from_str` | Throw JS Error `"invalid calldata hex"` |
| `value` / `amount` (decimal-string) | `U256::from_str_radix(s, 10)` (rejects negatives + non-digits) | Throw JS Error `"invalid amount: <reason>"` |
| `abi` (string variant) | `serde_json::from_str::<JsonAbi>(s)` AND `abi.function(name)` non-empty | Throw JS Error `"abi does not contain function <name>"` |
| `function` + `args` | `Function::abi_encode_input(&values)` actually succeeds (early ABI dry-run encode to surface arg-count/type mismatches at builder time, NOT at Phase-5 normalization time) | Throw JS Error `"abi encode failed: <reason>"` |

**Critical point:** the builder does ABI encoding *as validation*, then **discards the bytes** and returns the original `{abi, function, args}`. Phase 5 re-encodes. This is intentional — Phase 5 is where canonical encoding lives, and we want the journal to record the structured args (auditable) not pre-encoded bytes (opaque).

## Action[] Wire Schema

Extension to `crates/executor-core/src/schema/action.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Noop,                                                              // Phase 3
    ContractCall(ContractCallAction),                                  // Phase 4 — CTX-05
    RawCall(RawCallAction),                                            // Phase 4 — CTX-06
    Erc20Transfer(Erc20TransferAction),                                // Phase 4 — CTX-07
    Erc20Approve(Erc20ApproveAction),                                  // Phase 4 — CTX-07
    NativeTransfer(NativeTransferAction),                              // Phase 4 — CTX-08
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContractCallAction {
    /// Target contract address (lowercase 0x… 40 hex on the wire after canonicalization).
    pub address: String,
    /// JSON ABI string — the full ABI, not just the called function. Phase 5
    /// normalization re-parses; serializing the parsed Function would lose
    /// overloads and complicate goldens.
    pub abi: String,
    pub function: String,
    /// Arguments as serde_json::Value array — preserves nested tuples / arrays.
    /// Phase 5 maps to DynSolValue using the same logic as ctx.evm.readContract.
    pub args: Vec<serde_json::Value>,
    /// Wei decimal-string. Default "0" if absent on input.
    #[serde(default = "default_zero_value")]
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawCallAction {
    pub address: String,
    /// 0x-prefixed even-length hex.
    pub data: String,
    #[serde(default = "default_zero_value")]
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Erc20TransferAction {
    pub token: String, pub to: String, pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Erc20ApproveAction {
    pub token: String, pub spender: String, pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeTransferAction {
    pub to: String, pub value: String,
}

fn default_zero_value() -> String { "0".to_string() }

impl Action {
    /// Phase-4 emission gate (mirrors RunStatus::phase2_emittable / JournalActionOutcome::phase3_emittable).
    /// All five new variants emit; Phase 5+ may add e.g. `MultiCall` and gate it here.
    pub fn phase4_emittable(&self) -> bool { true }
}
```

**Phase-3 → Phase-4 validator change:** The Phase-3 D-08a test `strategy_run_rejects_phase4_action_kind` flips: `kind: "contract_call"` with valid fields succeeds; `kind: "contract_call"` with bad fields returns -32018. We update the test name to `strategy_run_accepts_contract_call` and add per-variant rejection cases.

**`deny_unknown_fields`:** every variant gets it. Forward-compat is via NEW variants, not field expansion. This matches the Phase-3 invariant.

**Schema goldens (Plan 04-04):** `Action.json` is regenerated to include all six variants. New per-variant goldens: `ContractCallAction.json`, `RawCallAction.json`, `Erc20TransferAction.json`, `Erc20ApproveAction.json`, `NativeTransferAction.json`.

## ctx.units / ctx.address Surfaces

**`ctx.units` — pure host-side decimal arithmetic on strings.**

```typescript
ctx.units = {
  parseUnits(amount: string, decimals: number): string,
  formatUnits(value: string, decimals: number): string,
}
```

Implementation lives in `executor-evm/src/units.rs` (NOT `strategy-js`, because `strategy-js` already chose to be alloy-free). `parseUnits` rejects negative amounts, non-decimal strings, and `decimals > 77` (U256 max precision is 78 digits). `formatUnits` operates on `U256::from_str_radix` then divmod by `10^decimals`, formats trailing-zero-trimmed fractional part.

**`ctx.address` — EIP-55 helpers.**

```typescript
ctx.address = {
  isAddress(s: any): boolean,           // returns false for non-strings (no throw)
  checksum(s: string): string,          // throws on invalid; returns EIP-55 mixed-case form
  zeroAddress: "0x0000000000000000000000000000000000000000",  // constant
}
```

`isAddress` accepts both lowercase and EIP-55 checksummed forms (returns true if either parse succeeds); rejects mixed-case with wrong checksum (returns false — does NOT throw). `checksum` is strict: input must be a valid address (any case); output is EIP-55. Mixed-case input with bad checksum throws `"invalid checksum"`.

**Why `zeroAddress` as a constant, not a function:** zero address is a sentinel, not a value to be computed. Constant catches typos at strategy-write time.

**EIP-1191 chain-prefixed checksums** (RSK, etc.): out of scope for v1. `to_checksum` and `parse_checksummed` both accept `Option<u64>` for chain id; we always pass `None`. Phase 5 may revisit if a multi-chain policy needs it.

## Concurrency Plan

**The bridge problem:** alloy is `async` (futures-based, tokio-pinned). rquickjs `Runtime` is synchronous and `!Sync` (D-01 `parallel` feature is forbidden). The Phase-3 handler already wraps `Sandbox::execute` in `tokio::task::spawn_blocking`. Phase 4 must call `Provider::call(...).await` from inside synchronous JS execution.

**Recommendation: `tokio::runtime::Handle::current().block_on(future)` inside the existing `spawn_blocking` closure.**

```rust
// Inside the ctx.evm.readContract host function (rquickjs Function::new closure):
let provider = provider.clone();             // Arc<DynProvider>
let tx = TransactionRequest::default()
    .with_to(addr)
    .with_input(calldata);

// We are inside spawn_blocking — Handle::current() returns the multi-threaded
// tokio runtime that owns this blocking thread. block_on parks the blocking
// thread until the future completes. The async runtime's worker threads are
// untouched.
let result_bytes: Bytes = tokio::runtime::Handle::current()
    .block_on(async {
        tokio::time::timeout(cfg.call_timeout, provider.call(&tx)).await
    })
    .map_err(EvmError::Timeout)?
    .map_err(EvmError::Transport)?;
```

**Why this is safe (and the alternatives are worse):**

1. **`block_on` is legal inside `spawn_blocking`.** [VERIFIED: tokio docs] `spawn_blocking` runs on a dedicated blocking thread that is not a tokio worker. `Handle::current()` returns the multi-threaded runtime handle; `block_on` on a blocking thread parks *that* thread until the future completes. The async workers run unobstructed. Calling `block_on` on an *async worker thread* is what panics — not what we're doing.
2. **Pre-fetching reads** (the alternative) breaks `ctx.evm.readContract`'s imperative model: the strategy can't use a balance to decide whether to make a second read. CTX-01 implicitly requires sync semantics from the strategy's POV.
3. **Spawning a fresh runtime** per call (`tokio::runtime::Builder::new_current_thread()`) costs ~50µs and prevents reqwest from reusing its connection pool — defeats the shared-Provider goal.

**Wall-clock budget interaction:** D-03 wall-clock 2s is enforced via `set_interrupt_handler`. The interrupt handler runs only between QuickJS bytecode opcodes — it does NOT preempt a `block_on` already in progress. A slow RPC call therefore can blow the 2s budget. Mitigation: `tokio::time::timeout(cfg.call_timeout, provider.call(...))` with `call_timeout < WALL_CLOCK_MS / max_calls_per_strategy`. Phase 4 sets `call_timeout = 1s` so a strategy can do up to ~2 sequential reads before the wall clock fires. Phase 5 may make this configurable.

**Pitfall (Phase-3 carry-over): `state.blocking_lock()` and provider calls in the SAME closure.** The Phase-3 handler holds `tokio::sync::Mutex` via `blocking_lock()` while DOING DB IO. If we `block_on` an alloy call while still holding `blocking_lock()`, no deadlock occurs (the mutex is for state-store, not the provider) — but contention against a concurrent run grows. Plan 04-01 acceptance includes a unit test that two sequential `strategy_run` invocations both hitting `ctx.evm.readContract` complete without a deadlock and without holding `blocking_lock` across the RPC call (drop the guard before `block_on`).

## Local EVM Test Infrastructure

**Recommended: `alloy::node_bindings::Anvil` (the Rust binding to the anvil CLI).** [VERIFIED: alloy.rs/examples]

```rust
// crates/executor-evm/tests/common/anvil_fixture.rs
use alloy::node_bindings::{Anvil, AnvilInstance};

pub struct AnvilFixture {
    pub instance: AnvilInstance,        // Drop kills the process
    pub rpc_url: url::Url,
    pub chain_id: u64,
    pub funded_accounts: Vec<alloy_primitives::Address>,  // anvil pre-funded
}

impl AnvilFixture {
    pub fn spawn() -> Self {
        let instance = Anvil::new()
            .chain_id(31337)
            .try_spawn()
            .expect("anvil binary must be on PATH for evm tests");
        let rpc_url = instance.endpoint_url();
        let chain_id = instance.chain_id();
        let funded_accounts = instance.addresses().to_vec();
        Self { instance, rpc_url, chain_id, funded_accounts }
    }
}
```

**Pre-deployed fixtures (counter contract, mock ERC20):** Compile once, ship the bytecode hex as a `&str` constant, deploy in test setup via `provider.send_transaction(deploy_tx).await`. We do NOT call `forge` from tests (no toolchain dep). The bytecode is committed to the repo under `crates/executor-evm/tests/fixtures/{counter.hex,erc20.hex}` with a comment pointing to the Solidity source.

**Env var fallback:** If `ANVIL_RPC_URL` is set in the environment (e.g., a developer already has anvil running), tests skip the spawn step and use that URL. This makes `cargo test -p executor-evm` runnable without anvil installed *iff* the env var points at a live devnet. CI sets `ANVIL_RPC_URL` only after explicitly starting anvil; tests that need pre-funded accounts skip when the URL is external (different account set).

**`cfg(feature = "anvil-tests")` gating:** Tests that spawn anvil are gated behind a feature flag so default `cargo test` works on machines without anvil. CI runs `cargo test -p executor-evm --features anvil-tests`.

**Phase 5/6 reuse:** This fixture lives in `tests/common/` of `executor-evm`. Phase 5 simulation tests and Phase 6 broadcast tests import via `[dev-dependencies] executor-evm = { path = "...", features = ["test-fixtures"] }` — the `test-fixtures` feature exposes the helper module via `pub use`. Plan 04-01 acceptance writes the helper; Plan 04-04 stress-tests it under all four `ctx.actions` variants.

## Pitfalls and Gotchas

### Pitfall 1: `set_interrupt_handler` does not preempt `block_on`

(Detailed above in Concurrency Plan.) The 2s wall-clock budget is **per-bytecode-step** scope; `block_on` is opaque. Always wrap RPC calls in `tokio::time::timeout`.

### Pitfall 2: rquickjs `BigInt` cannot represent u256

Strategies that write `const x = 1000000000000000000n;` (BigInt literal) and return `x` already fail today (Phase-3 `qjs_value_to_json` rejects). But strategies that return `{ amount: 100n }` *inside* an action object also fail with the same error. Plan 04-04 acceptance includes a regression test: `(ctx) => ctx.actions.erc20Transfer({token, to, amount: 100n})` must produce a clear "amount must be a string, got BigInt" error from the builder, NOT a confused "BigInt is not supported in strategy returns" error from the JSON serializer.

### Pitfall 3: alloy 1.x → 2.0 breaking changes

[VERIFIED: alloy 2.0.1 release 2026-04-22] alloy 2.0 made `ProviderBuilder::on_http` → `connect_http` and a few RPC type renames. Pin `alloy = "2.0"` (NOT `"1"`). All Phase-4 examples in this RESEARCH use 2.0 names. If `cargo add alloy` resolves to 2.x.y, accept it; if 1.x, force `2.0` explicitly.

### Pitfall 4: `JsonAbi::function(name)` returns `Option<&[Function]>` (overloads)

Solidity allows function overloading by argument types. `JsonAbi::function("foo")` returns a slice; we must select by argument count + type compatibility. For ERC20 helpers (`balanceOf(address)` is unique), this is trivial; for `ctx.evm.readContract` with arbitrary user ABI, we walk the slice and pick the first whose `inputs.len() == args.len()` and all types match `DynSolType` parsed from `inputs[i].selector_type_name()`. Ambiguous match (>1 hit) → host-side error `"function <name> has overloads; cannot disambiguate"`.

### Pitfall 5: Address case sensitivity

`Address::from_str("0xAbC...")` accepts ANY case without checksum validation; `Address::parse_checksummed("0xAbC...", None)` REQUIRES correct EIP-55 mixed case. Strategies often paste copy-pasted lowercase addresses. Convention: builders accept either (try `parse_checksummed` first, fall back to `from_str` only if all-lowercase or all-uppercase 40 hex). Mixed-case-but-wrong-checksum is rejected with `"address looks checksummed but checksum is invalid"`.

### Pitfall 6: `0x` prefix on calldata

`Bytes::from_str` accepts `0x`-prefixed input; `hex::decode` does NOT (rejects the prefix). Use `Bytes::from_str` exclusively to avoid two code paths. The Phase-4 builder requires the `0x` prefix; explicitly reject bare-hex input with a hint.

### Pitfall 7: Reentrancy in `ctx.evm.readContract`

A strategy can call `readContract` repeatedly inside a loop. Each call is a synchronous `block_on(provider.call(...))`. There is no per-strategy RPC budget in v1 — wall-clock is the only ceiling. A pathological strategy can issue ~thousands of calls in 2s. Plan 04-04 includes a regression test: `(ctx) => { for(let i=0;i<10000;i++) ctx.evm.readNative.balance(ZERO); return "noop"; }` MUST complete with a `runtime_error: timeout` (the wall-clock fires) — NOT a panic, NOT a deadlock. Phase 5 policy may add a per-run RPC count cap.

### Pitfall 8: anvil pre-funded account ordering

`AnvilInstance::addresses()` returns a deterministic list (anvil's hardcoded mnemonic). Tests should never `addresses()[0]` blindly across versions; pin to a specific account in test fixtures and assert its balance.

### Pitfall 9: `rquickjs::Function::new` closure must be `'static` and `Send` (for shared providers)

Currently the Phase-3 sandbox uses `Rc<RefCell<...>>` for the log buffer because it's single-threaded. The `Provider` host binding for `ctx.evm.*` will need `Arc<DynProvider>` (Send + Sync); cloning into the closure is fine — it just consumes a clone. Verify the closure signature compiles BEFORE Phase 4 implementation: `Function::new(c.clone(), move |args: Rest<Value>| -> rquickjs::Result<Value> { ... })`.

### Pitfall 10: `serde_json::Value` is a poor `DynSolValue` carrier for tuples

`DynSolValue::Tuple` and `DynSolValue::Array` both serialize from `serde_json::Value::Array`. The host can't disambiguate a tuple from a fixed-size array purely from JSON. We MUST drive conversion from the ABI (`Function::inputs[i].selector_type_name()` says `(uint256,address)` vs. `uint256[2]`). Plan 04-01 acceptance includes a tuple-arg readContract test.

### Pitfall 11: Stale ABI string in journal

`ContractCallAction.abi` is a long JSON string. Storing it verbatim in `journal_actions.payload_json` (Phase-3 D-06) bloats journal rows. Decision: keep it (audit trail), but enforce a 64 KiB cap on the ABI field at builder time — typical ERC20 ABI is ~5 KiB, full DEX router ~20 KiB. Plan 04-03 includes a 64 KiB regression cap.

### Pitfall 12: anvil binary not on PATH

`Anvil::try_spawn()` returns `Err` with a clear message if `anvil` isn't on PATH. Tests that depend on it must check for this in setup and skip with a clear message — NOT panic in `Drop`. The `cfg(feature = "anvil-tests")` gate prevents accidental runs.

### Pitfall 13: `serde_json::Number::from_f64(f64::NAN)` returns None

The Phase-3 `qjs_value_to_json` already rejects non-finite floats. Phase 4 `DynSolValue` decode never produces non-finite values (Solidity has no float type), so this is not a new failure mode — but Plan 04-04 should still test that a strategy doing `Number(NaN)` math followed by `parseUnits(thatValue, 18)` fails at the *builder* with `"amount must be a non-negative integer string"`, NOT at JSON serialization.

### Pitfall 14: `Eval` intrinsic interaction with ABI strings

Strategies can construct ABI strings dynamically (`JSON.stringify(somefn())`). `Eval` is enabled (Phase-3 D-04 caveat). This is fine — the host validates the resulting ABI via `serde_json::from_str::<JsonAbi>`. No new attack surface.

## Runtime State Inventory

Phase 4 is greenfield — no rename / migration concerns. Skipping detailed inventory.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `anvil` (foundry) | `executor-evm` integration tests (--features anvil-tests) | unknown — check `which anvil` at planning time | foundry 0.2+ | `ANVIL_RPC_URL` env var pointing at external devnet; OR mark tests `#[ignore]` and run only on operator-side CI with foundry installed |
| `rustc` ≥ 1.91 | alloy 2.0 MSRV | likely yes (Phase 1-3 already on edition 2024) | stable | upgrade rustup |
| Network access during build | `cargo` to fetch alloy + dyn-abi | yes in dev | — | offline cargo cache populated once after `cargo fetch` |

**Missing dependencies with no fallback:** `anvil` is the only blocker — if not installed, `--features anvil-tests` fails. Plan 04-01 includes a "Wave 0 — install foundry" step OR documents `curl -L https://foundry.paradigm.xyz | bash && foundryup` in the per-developer setup. CI must install foundry before `--features anvil-tests` runs.

**Missing with fallback:** ANVIL_RPC_URL env var allows running tests against an externally-managed devnet (Hardhat node, Reth dev mode, Tenderly fork) — sufficient for read tests, may not work for tests that assume specific funded accounts.

## Validation Architecture

Phase 4 inherits Phase-3's nyquist_validation default (config.json key absent → enabled).

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (`#[test]` and `#[tokio::test]` from `tokio = { features = ["macros"] }`) — same as Phase 1-3 |
| Config file | none — workspace Cargo.toml |
| Quick run command | `cargo test -p executor-evm --lib` (unit tests only — fast, no anvil) |
| Full suite command | `cargo test --workspace --features anvil-tests` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CTX-01 | `readContract` decodes a counter contract's `number()` return | integration (anvil) | `cargo test -p executor-evm --features anvil-tests read_contract_decodes_counter -- --exact` | ❌ Wave 0 |
| CTX-02 | `erc20Balance` returns funded balance against mock ERC20 | integration (anvil) | `cargo test -p executor-evm --features anvil-tests erc20_balance_returns_funded` | ❌ Wave 0 |
| CTX-03 | `erc20Allowance` returns 0 for un-approved spender | integration (anvil) | `cargo test -p executor-evm --features anvil-tests erc20_allowance_zero_unapproved` | ❌ Wave 0 |
| CTX-04 | `nativeBalance` returns anvil pre-funded account balance | integration (anvil) | `cargo test -p executor-evm --features anvil-tests native_balance_funded_account` | ❌ Wave 0 |
| CTX-05 | `ctx.actions.contractCall(valid)` produces wire-correct JSON; bad ABI rejected | unit (no anvil) | `cargo test -p strategy-js contract_call_builder_valid_and_invalid` | ❌ Wave 0 |
| CTX-06 | `ctx.actions.rawCall(valid)` produces wire-correct JSON; bad hex rejected | unit | `cargo test -p strategy-js raw_call_builder_valid_and_invalid` | ❌ Wave 0 |
| CTX-07 | `erc20Transfer` and `erc20Approve` builders validate amount + addresses | unit | `cargo test -p strategy-js erc20_action_builders` | ❌ Wave 0 |
| CTX-08 | `nativeTransfer` builder validates non-negative amount | unit | `cargo test -p strategy-js native_transfer_builder` | ❌ Wave 0 |
| CTX-09 | `parseUnits`/`formatUnits` roundtrip; `isAddress` accepts EIP-55; `checksum` rejects bad mix | unit | `cargo test -p executor-evm units_and_address_helpers` | ❌ Wave 0 |
| CTX-01..09 | End-to-end: stdio strategy_run that uses ctx.evm reads + ctx.actions returns valid Action[] | integration stdio (anvil) | `cargo test -p executor-mcp --features anvil-tests strategy_run_phase4_end_to_end` | ❌ Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test -p <crate-touched> --lib` (fast unit tests, no anvil) + `cargo clippy --workspace --all-targets`.
- **Per wave merge:** `cargo test --workspace --features anvil-tests` — includes anvil-spawning integration tests.
- **Phase gate:** Full suite green before `/gsd-verify-work`.

### Wave 0 Gaps

- [ ] `crates/executor-evm/Cargo.toml` — new crate manifest with alloy + dyn-abi deps
- [ ] `crates/executor-evm/src/lib.rs` — module skeleton
- [ ] `crates/executor-evm/src/provider.rs` — `EvmConfig` + `build_provider`
- [ ] `crates/executor-evm/src/dyn_abi.rs` — JS-arg ↔ DynSolValue conversion
- [ ] `crates/executor-evm/src/erc20.rs` — bundled ERC20 ABI + helper trait
- [ ] `crates/executor-evm/src/units.rs` — `parseUnits` / `formatUnits`
- [ ] `crates/executor-evm/src/address.rs` — `isAddress` / `checksum`
- [ ] `crates/executor-evm/tests/common/anvil_fixture.rs` — reusable fixture for Phase 5/6
- [ ] `crates/executor-evm/tests/fixtures/{counter.hex,erc20.hex}` — pre-compiled test bytecode
- [ ] Workspace `Cargo.toml` adds `crates/executor-evm` to members
- [ ] Foundry / `anvil` install step (operator setup or CI step)

## Security Domain

Phase 4 expands the runtime's attack surface to include outbound RPC and ABI parsing. Applicable concerns:

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V5 Input Validation | yes | All builder inputs validated via alloy primitives (Address, U256, Bytes) — no hand-rolled parsers; `serde(deny_unknown_fields)` on all action variants. |
| V6 Cryptography | partial | NO new key material in Phase 4. EIP-55 checksum is integrity-only, not cryptographic auth. |
| V8 Data Protection | yes | ABI strings stored in `journal_actions.payload_json` may be large but contain no secrets (public contract metadata). 64 KiB cap (Pitfall 11) prevents accidental DoS-via-huge-ABI. |
| V12 API and Web Service | yes | Outbound HTTP RPC to anvil — bound to `127.0.0.1` by default. SSRF risk if `EvmConfig::rpc_url` is operator-supplied; v1 trusts the operator. Phase 5 policy may add an allowlist of RPC URLs. |
| V14 Configuration | yes | `EvmConfig::rpc_url` is operator-controlled; not strategy-controlled. Strategy CANNOT override the provider URL — that would be a sandbox escape. |

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Strategy returns crafted ABI to read arbitrary host memory | Information Disclosure | `alloy-dyn-abi`'s decoder is bounds-checked and returns `Result`; no unsafe code in alloy. The 64 KiB ABI cap also bounds parsing cost. |
| Strategy floods provider with `readContract` calls | Denial of Service | Wall-clock 2s + per-call timeout 1s = ~2 RPC calls before kill. Phase 5 policy may add per-run RPC count cap. |
| Strategy returns Action[] with bogus calldata to drain ETH | (Phase 5/6 concern) | Phase 4 only builds actions; signing/broadcasting is Phase 6 behind policy/sim gates. |
| ABI parser panic on adversarial input | Tampering | `JsonAbi: Deserialize` is fully fallible; no panics expected. `unsafe_code = "forbid"` workspace lint prevents any unsafe path. |

**Critical security invariant:** The alloy `Provider` (and the `reqwest::Client` it wraps) is NEVER exposed to JS as a value. The closure captures an `Arc<DynProvider>`; JS sees only the `ctx.evm.*` function shapes. PROJECT.md "no direct RPC client" is preserved.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | alloy 2.0.1 (released 2026-04-22) is the current stable. | Standard Stack | If 2.1+ exists at planning time with breaking changes, examples need version bumps. Mitigation: re-verify `cargo add alloy --dry-run` before locking. |
| A2 | rquickjs 0.11 has no built-in u256 BigInt support. | BigInt Bridge | If 0.12+ adds `BigInt::from_str`, decimal-string convention is still valid (more conservative); upgrade path is additive. |
| A3 | `Handle::current().block_on(...)` inside `spawn_blocking` does not deadlock on the multi-threaded tokio runtime. | Concurrency Plan | [VERIFIED: tokio docs state spawn_blocking runs on a separate thread pool; block_on on a non-worker thread is supported.] Phase-4 Wave-0 task includes a smoke test that hits `block_on` from `spawn_blocking` to confirm. |
| A4 | anvil's `--chain-id 31337` is stable across foundry releases. | Local EVM Test Infra | If foundry changes the default, hard-coded `chain_id(31337)` in fixture still works (we set it explicitly). |
| A5 | `Action::phase4_emittable()` returning `true` for all Phase-4 variants is the right gate. | Action Wire Schema | Phase 5 may want some variants gated (e.g., `RawCall` denied unless policy whitelists). The gate stays at the *Action enum* level for variant additions; emission policy stays in `executor-policy`. |
| A6 | Per-call timeout of 1s is enough for local anvil RPC. | Concurrency Plan | If a strategy does ~10 reads, total budget is 10s vs. 2s wall clock — wall clock will fire first. The 1s per-call is a safety net for anvil hangs. Tunable. |
| A7 | Strategies will not need EIP-1191 chain-prefixed checksums in v1. | ctx.address | If a v1 user is on RSK or similar, `parse_checksummed(s, Some(chain_id))` would be needed. Adding it later is non-breaking. |
| A8 | 64 KiB cap on ABI strings in `ContractCallAction.abi` is generous. | Pitfall 11 | If a real DEX aggregator ABI exceeds 64 KiB, raise to 128 KiB; cap shape is a constant in `executor-core`. |
| A9 | `executor-evm` crate is the right place; no need to split into `executor-evm-read` + `executor-evm-action`. | Architectural Map | If Phase 5 simulation crate balloons, can split later. v1 keeps it monolithic. |
| A10 | Each `ctx.evm.*` call gets a fresh borrow of `Arc<DynProvider>`; no per-strategy provider isolation. | Provider Strategy | Multi-tenant scenarios (V2-07) may need per-tenant providers. Out of scope. |

## Open Questions for Planner

### Q1. Should EVM transport errors map to a NEW MCP error code, or reuse `-32017 STRATEGY_RUNTIME_ERROR`?

**Proposed default:** Reuse `-32017` with `data.kind = "evm_transport"` (anvil down, RPC timeout) and `data.kind = "evm_revert"` (contract reverted with reason). This matches Phase-3 D-07 pattern (`data.kind ∈ {"exception","oom","timeout","stack_overflow"}`) — just two more kinds. Avoids burning a new error code on a sub-classification.

**Alternative:** New `-32019 STRATEGY_EVM_ERROR`. Reserved per Phase-3 D-07. Rejected because the agent's recovery action is the same as for any runtime error: read the journal, retry or rewrite.

### Q2. Should `ctx.evm.readContract` accept a parsed ABI (JS array of fragments) OR only a JSON string?

**Proposed default:** Accept BOTH. JS-side: if `abi` is a string, treat as JSON and parse with `serde_json::from_str`; if it's an array, `JSON.stringify` it inside the host then parse. The "string" path is ~2x faster (no double-encoding); the "array" path is ergonomic for agents constructing ABIs programmatically. No correctness difference.

### Q3. Should ERC20 helpers reuse `readContract` or have their own optimized path?

**Proposed default:** Reuse `readContract`. The bundled ABI is a `&'static str` JSON string; the helper just calls `readContract({ address, abi: ERC20_ABI, function: "balanceOf", args: [account] })`. Code reuse > micro-optimization. If Phase 7 benchmarks show meaningful overhead, drop to a direct `Function` call (skip `JsonAbi` parse).

### Q4. Should `RawCall` go through Phase-4 builder validation at all, or be pass-through?

**Proposed default:** Validate address + hex shape only; do NOT validate calldata against any ABI. RawCall is the escape hatch for situations where the strategy author knows what they're doing (proxies, custom selectors). Policy in Phase 5 (POL-06) deny-by-default takes care of the safety story. Phase 4 just shapes the JSON.

### Q5. Should `ctx.evm.*` calls be journaled to `journal_source_reads` like Phase-3's `strategy_source` marker?

**Proposed default:** YES, one row per `ctx.evm.*` call. Phase-3 D-06 already locked the schema with `kind` and `payload_json` fields. Phase 4 emits e.g. `kind="evm_call"`, `target=<contract>`, `payload_json={"function":"balanceOf","args":[...]}`. STJ-03 says "Runtime records source reads performed during each run" — this is the realization for EVM reads. Plan 04-02 wires this; tests assert one row per ctx.evm call.

### Q6. Where should `parseUnits` / `formatUnits` actually live — `strategy-js` or `executor-evm`?

**Proposed default:** `executor-evm`. Reason: the implementation needs `alloy_primitives::U256` for correctness (78-digit precision), and `strategy-js` deliberately stays alloy-free (D-01 sandbox-engine isolation). The host binding lives in `strategy-js::sandbox` (where all `ctx.*` injections live), but the body delegates to `executor_evm::units::{parse_units, format_units}`.

### Q7. Should `phase4_emittable` actually exist, given it returns `true` for everything?

**Proposed default:** YES, declare it explicitly. Phase-5 may add `MultiCall` / `Permit2Approve` action variants and need to gate them via `phase5_emittable`. Following the pattern set by Phase-2 `phase2_emittable` and Phase-3 `phase3_emittable` keeps the codebase predictable. Token cost is one method body.

### Q8. Should this phase add a Phase-1-style schema golden for the new Action variants?

**Proposed default:** YES, mirror Phase-3 D-08 schema-golden discipline. New goldens: `Action.json` (regenerated), plus per-variant `ContractCallAction.json` etc. Plan 04-04 task. Critical because Action shape is part of the agent-visible MCP schema.

### Q9. Should we promote `alloy` to `[workspace.dependencies]` in Phase 4?

**Proposed default:** NO. Phase-2 D-03 / Phase-3 D-02 rule: promote only when ≥2 crates consume the same dep. Phase 4 has only `executor-evm` consuming alloy. Phase 5 will add `executor-mcp` (transaction normalization needs `TransactionRequest`) — at that point promote.

### Q10. Strategy-side ergonomics: should we expose a helper that loads a popular contract ABI by address (proxy resolution + Etherscan lookup)?

**Proposed default:** NO. Out of scope for v1. Strategy authors paste ABIs into the source. ABI registry / on-chain ABI resolution is a v2 feature (V2-05 capability registry). Mention in `Deferred Ideas`.

## Sources

### Primary (HIGH confidence)
- [alloy v2.0.1 release on github.com/alloy-rs/alloy](https://github.com/alloy-rs/alloy) — released 2026-04-22, current stable
- [docs.rs/alloy/latest/alloy/](https://docs.rs/alloy/latest/alloy/) — feature flags, ProviderBuilder API
- [docs.rs/alloy-dyn-abi/latest](https://docs.rs/alloy-dyn-abi/latest/alloy_dyn_abi/) — DynSolType / DynSolValue runtime ABI surface
- [docs.rs/alloy-json-abi v1.5.7](https://docs.rs/alloy-json-abi/) — JsonAbi, Function, abi_encode_input/abi_decode_output
- [docs.rs/alloy-primitives Address](https://docs.rs/alloy-primitives/latest/alloy_primitives/struct.Address.html) — parse_checksummed, to_checksum, EIP-55
- [docs.rs/rquickjs/latest/rquickjs/](https://docs.rs/rquickjs/latest/rquickjs/) — 0.11 BigInt struct + Type variants, FromJs/IntoJs
- [alloy.rs/examples/node-bindings/anvil_local_instance/](https://alloy.rs/examples/node-bindings/anvil_local_instance/) — Anvil::new().try_spawn() pattern
- crates/strategy-js/src/sandbox.rs (in-tree) — Phase-3 ctx injection mechanism, qjs_value_to_json, FORBIDDEN_GLOBALS_SCRUB
- crates/executor-mcp/src/tools.rs:225-351 (in-tree) — strategy_run handler 8-step lifecycle and validate_strategy_output gate
- crates/executor-core/src/schema/{action.rs,execution.rs} (in-tree) — Action enum extension point, JournalActionOutcome future-lock pattern
- .planning/phases/03-javascript-strategy-runner/03-CONTEXT.md — D-04 ctx surface scope, D-06 journal schema, D-08 strategy_run contract
- .planning/REQUIREMENTS.md — CTX-01..09 verbatim text (Strategy Runtime, Context API sections)
- AGENTS.md — line 17 (alloy lock), line 36 (executor-evm/ target crate)

### Secondary (MEDIUM confidence)
- [alloy.rs/contract-interactions/using-sol!/](https://alloy.rs/contract-interactions/using-sol!/) — sol! macro patterns (referenced for contrast — we use dyn-abi, not sol!)
- [github.com/alloy-rs/examples/contracts/interact_with_abi.rs](https://github.com/alloy-rs/examples/blob/main/examples/contracts/examples/interact_with_abi.rs) — interact_with_abi pattern (sol!-based; dyn-abi alternative inferred from docs.rs/alloy-dyn-abi)

### Tertiary (LOW confidence)
- WebSearch hits on "alloy-node-bindings AnvilInstance endpoint_url" — corroborated by docs.rs but exact method name (`endpoint_url()` vs `endpoint()`) should be re-verified at implementation time. The `Anvil::new().try_spawn()` pattern itself is HIGH confidence.

## Metadata

**Confidence breakdown:**
- alloy 2.0 selection + version: HIGH — verified via crates.io / github releases
- dyn-abi runtime path: HIGH — verified via docs.rs
- BigInt convention (decimal-string): HIGH — rquickjs BigInt API limits verified; convention is opinionated but well-grounded
- Concurrency plan (`block_on` inside `spawn_blocking`): HIGH — established tokio pattern, Phase-3 already uses spawn_blocking shape
- Action wire schema: HIGH — direct extension of Phase-3 locked patterns
- Test infra (Anvil binding): HIGH — verified via alloy.rs examples; minor MEDIUM on exact `endpoint_url()` method name
- Address/units helpers: HIGH — alloy-primitives surface verified

**Research date:** 2026-04-27
**Valid until:** 2026-05-27 (30 days; alloy is mature enough that 2.0.x patches are non-breaking; rquickjs 0.11 is stable)
