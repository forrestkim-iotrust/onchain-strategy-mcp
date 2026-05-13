# onchain-strategy-mcp

> A local-first MCP runtime that lets AI agents execute EVM strategies safely:
> sandboxed JavaScript → policy-gated `Action[]` → EIP-7702 batched execution →
> auditable journal. No hosted custody, no marketplace, no off-chain orderflow.

**License:** Apache-2.0 · **Status:** v1.2 spike (trigger core + event subscriptions on Base mainnet)

```
   strategy JS (sandbox)
   └─ ctx.evm.* reads          ┐
   └─ ctx.actions.* builders   │ → Action[]   ┐
   └─ ctx.event (trigger pay)  ┘              │
                                              ▼
   ┌──────────────────────────────────────────────────┐
   │ policy gate (deny-by-default, 6 dimensions)      │
   │ simulation (eth_call)                            │
   │ EIP-7702 batch signer (single tx for N≥2 calls)  │
   │ broadcast + receipt wait                         │
   │ journal: every decision + tx + log               │
   └──────────────────────────────────────────────────┘
                                              ▼
                                         chain (Base/...)
```

---

## What this is — and isn't

**Is:** a local stdio MCP server that exposes a small surface for AI agents to
write, register, run, and observe onchain strategies. Strategy code is
sandboxed JavaScript; it cannot read your private key, hit arbitrary RPCs,
touch the filesystem, or bypass the policy gate. Every approved tx is signed
locally and journaled.

**Isn't:** a hosted custody service, a marketplace, an alpha-generation
agent, a router, a scheduler daemon you point at strangers' funds, or a UI.

---

## Why

Today, letting an AI agent touch onchain funds means picking between:

1. **Give the agent your private key** — one prompt-injection / hallucinated
   address and you're rugged.
2. **Hand-approve every tx** — not automation, just slower copy-paste.

This runtime fills the gap: agents emit *plans* (an `Action[]` from a JS
strategy), the runtime enforces *policy* before signing, and EIP-7702 lets
multi-step plans land atomically in a single transaction. Your key never
leaves your machine.

---

## Features (v1.0 → v1.2)

- **20 MCP tools** — strategy CRUD, run, execution lookup, read helpers
  (`evm_balance` / `evm_code` / `evm_read` / `evm_receipt` / `evm_view`),
  policy inspect, trigger CRUD.
- **Sandboxed JS strategy runner** (QuickJS) with a small `ctx` API:
  - `ctx.evm.readContract / nativeBalance / erc20Balance / readErc20.* / readNative.*`
  - `ctx.actions.{erc20Approve, erc20Transfer, contractCall, rawCall, nativeTransfer}`
  - `ctx.event` — current trigger's event payload
  - `ctx.log`, `ctx.units`, `ctx.address`
  - host access (FS / process / arbitrary fetch / private key) **denied by default**
- **Deny-by-default policy**, 6 dimensions per chain:
  chain · contract · selector · native-value cap · ERC20 spend cap · raw-call.
- **Pre-broadcast simulation** via `eth_call`.
- **EIP-7702 batching** — when a strategy returns ≥2 actions and `[aa].delegate`
  is set, the runtime authorizes burner → BatchExec for one tx, then forwards
  every inner call with `msg.sender == burner`.
- **Trigger core (v1.2)** — strategies fire on:
  - `manual` (via `strategy_run` MCP call)
  - `interval` (cron-style, ticks every N ms)
  - `mempool` (Alchemy `alchemy_pendingTransactions` with server-side filter)
  - `log` (`eth_subscribe` logs with address + 4-topic filter)
  - Reserved kinds: `block`, `webhook` (v1.3+)
- **Predicate evaluator** — optional JS function `(event) => bool` per trigger.
- **Dedup window** — per-trigger time-based key dedup.
- **Append-only journal** — strategies, runs, source reads, actions, decisions,
  executions, trigger events. Standard SQLite.
- **Historical reads** — pass numeric `blockTag` to `ctx.evm.readContract` for
  archive queries (Alchemy or any archive RPC).

---

## Quickstart (Base mainnet)

> v1.0/v1.2 ship as Rust source; prebuilt binaries land in v1.3. For now,
> you need `cargo` (Rust 2024 edition / 1.91+) and `foundry` (for the
> optional EIP-7702 delegate).

### 1. Build

```bash
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp
```

### 2. Operator config

```bash
cp -R .local.example .local
$EDITOR .local/config.toml      # paths, RPC, signer env name
$EDITOR .local/policy.toml      # which contracts/selectors strategies may touch
```

### 3. Generate a burner

```bash
cast wallet new                 # save the private key out-of-band
export EXECUTOR_PRIVATE_KEY=0x...
# Fund the burner with a few cents of ETH for gas + your operational asset.
```

### 4. (Optional) Deploy EIP-7702 BatchExec for atomic multi-action runs

```bash
cd examples/contracts
forge create --rpc-url <rpc> --private-key $EXECUTOR_PRIVATE_KEY --broadcast BatchExec.sol:BatchExec
# Put the deployed address into [aa].delegate in your config.
```

### 5. Register the MCP server with Claude Code

```bash
claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

### 6. Inside Claude Code

```
> Register the example strategy at examples/strategies/eth-funnel.js as
  "funnel-v1", list policies, then list registered strategies.
```

The agent calls `strategy_register` / `policy_get` / `strategy_list` via MCP.

---

## Use cases (validated on Base mainnet)

### 1. Auto-funnel — "any ETH/USDC arriving at burner → USDC → Aave"

`examples/strategies/eth-funnel.js` + two `log` triggers (one on
`NativeReceived` at burner address, one on `USDC.Transfer` where the
recipient is burner). The runtime detects incoming value, swaps the excess
ETH on Uniswap V3 keeping a gas reserve, and on the next fire bundles
`approve + supply` into a single EIP-7702 batch tx into Aave V3.

End-to-end validated on Base mainnet during development. Every policy/sim
verdict is in `journal_decisions`; the batched supply records a single
`tx_hash` against both actions in `journal_executions`.

### 2. Yield observer — passive APY data collection

`examples/strategies/yield-snapshot.js` reads USDC supply APY from Aave V3 +
Compound III + Moonwell on Base, logs the snapshot, returns `noop`. Pair
with an `interval` trigger to accumulate a free time series for any
allocator strategy you'd write later.

### 3. Historical analysis — block-tag backfill

`ctx.evm.readContract({..., blockTag: 45943991})` works against any archive
RPC. Loop over block numbers inside a single `evm_view` to assemble a
multi-protocol APY history (we collected 7d × hourly × 3 protocols = 504
samples this way).

### 4. EIP-7702 atomic multi-call

When a strategy returns N≥2 actions, the runtime constructs a single 7702
transaction:
- signs an Authorization (burner → BatchExec) at `nonce+1`
- ABI-encodes `executeBatch(Call[])` with each action as `(to, value, data)`
- broadcasts type-0x04 tx from burner to burner
- `msg.sender` inside every inner call is still the burner

Same `tx_hash` is recorded against every action in `journal_executions`.

---

## Architecture

7 Rust crates:

| Crate | Role |
|---|---|
| `executor-core` | Pure-domain types: `Action`, `Run`, `Outcome`, schemas. No alloy. |
| `executor-state` | SQLite store: strategies, runs, journal, triggers. |
| `executor-policy` | Deny-by-default policy DSL + evaluator. alloy-free. |
| `executor-evm` | alloy provider, normalize Action→TransactionRequest, simulator, read helpers. |
| `executor-signer` | Local private-key signing + EIP-7702 batch construction. |
| `strategy-js` | QuickJS sandbox, `ctx` API installer, predicate evaluator. |
| `executor-mcp` | rmcp 1.5 stdio server, tool handlers, trigger daemon, worker pool. |

Trigger flow:

```
worker (interval | mempool | log | webhook)
        └─→ TriggerEvent → mpsc → Dispatcher
                                 ├─ predicate JS eval (sandbox)
                                 ├─ dedup window check
                                 └─ run_strategy_with_event(event)
                                          └─ same pipeline as manual strategy_run
```

---

## Status & roadmap

| Milestone | Status |
|---|---|
| v1.0 — Strategy runtime + policy + sim + local signer | shipped |
| v1.1 — EIP-7702 batching, read tools, evm_view | shipped (this branch) |
| v1.2 — Trigger core (manual / interval / mempool / log) | shipped (spike) |
| v1.3 — block worker, webhook worker, hardened reconnect | planned |
| v1.4 — prebuilt binaries, `osmcp init/burner new`, distribution polish | planned |
| v2.0 — session-key / smart-account integration (EIP-7715) | exploratory |

---

## Security

- The signer **never** reads your private key from a config file. Only the
  *name* of an env var is configured; the runtime reads `std::env::var(...)`
  at the signing boundary.
- Strategies run in QuickJS with D-11 deny-by-default globals scrub:
  no `fetch`, no `process`, no `import`, no host APIs.
- Policy is fail-closed: if `[policy].path` is unset or the file fails to
  parse, every non-noop `strategy_run` returns `-32017 policy_not_loaded`.
- EIP-7702 delegate target MUST have a `receive() external payable` if you
  want the burner to accept plain ETH transfers post-delegation. The included
  `BatchExec.sol` does. Delegating to a contract without `receive()` silently
  bricks incoming ETH (wallets report failed sim).
- Treat the burner as a hot wallet. The runtime + delegate contract are part
  of your trusted compute base.

**Found a vulnerability?** Open a private security advisory on GitHub.

---

## Contributing

Early-stage. The cleanest contributions right now:

- Additional `examples/strategies/*.js`
- Additional trigger workers (`block`, `webhook` — scaffolds exist)
- Better integration tests against forked Base / anvil
- Documentation improvements

Run before opening a PR:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Lint policy is `-D warnings` workspace-wide.

---

## Acknowledgements

- [rmcp](https://github.com/modelcontextprotocol/rust-sdk) — MCP server framework
- [alloy](https://github.com/alloy-rs/alloy) — EVM types, signer, WSS pubsub
- [rquickjs](https://github.com/DelSkayn/rquickjs) — QuickJS bindings
- [foundry](https://github.com/foundry-rs/foundry) — Solidity tooling for the delegate

EIP-7702 (Pectra, May 2025) makes this shape practical without a bundler.
