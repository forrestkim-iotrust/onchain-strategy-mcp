<p align="right"><b>English</b> · <a href="./README_ko.md">한국어</a></p>

# onchain-strategy-mcp

A local runtime that gives an AI agent (like Claude) **hands and eyes onchain**.

---

## 1. What is this?

When you ask Claude today to *"deposit my USDC into Aave"*, you get back a Python snippet and "now you run it." Everything operational falls on you: RPC, keys, gas, logging, simulation, receipts, error handling. The AI designs; you operate. Half a day to set up one piece of automation.

This runtime takes over the operator role. The AI describes *what it wants to happen* as a short JavaScript strategy — the runtime handles everything else:

- Connecting to the chain, simulating each step
- Signing locally and broadcasting
- Waiting for receipts, journaling every decision
- Subscribing to onchain events for live reaction
- Bundling multi-step actions into a single atomic transaction (EIP-7702)

The AI doesn't propose code anymore. It **operates** code, inside your machine, in a conversational loop.

---

## 2. What you can build

Anything you can describe in a short JavaScript function — inside whatever scope your policy allows. Things people have already built:

- **Auto-deposit funnel** — every drop of ETH/USDC that lands at a wallet gets converted and supplied to a lending market.
- **Yield rotator** — moves stablecoin liquidity to the highest-paying market, only when the gain beats gas.
- **Event reactors** — strategies that fire on log events, price-oracle updates, transfer hooks, or mempool signals.
- **Multi-protocol price/APY comparators** — read any number of contracts at any past block, build a dataset in seconds.
- **Atomic multi-step actions** — approve + supply, swap + LP, etc. land as one tx via EIP-7702.

Trigger modes available out of the box:

| Mode | Fires when |
|---|---|
| `manual` | You (or Claude) ask it to run |
| `interval` | Every N ms — cron-style |
| `log` | A matching onchain log appears in a confirmed block |
| `mempool` | A matching pending tx hits the watched node (Alchemy WSS) |
| Reserved | `block`, `webhook` — wired in v1.3 |

---

## 3. How to use it

You'll need: a Mac or Linux machine, [Node.js 18+](https://nodejs.org/), and [Claude Code](https://claude.ai/code). No Rust, no Foundry.

```bash
# 1. One-line install (downloads the prebuilt binary, generates a burner wallet
#    stored in your OS keychain, scaffolds .local/config.toml + .local/policy.toml)
npx onchain-strategy-mcp init

# 2. Register with Claude Code (init prints this exact line for you)
claude mcp add osmcp -- npx onchain-strategy-mcp serve
```

That's it. Open Claude Code and:

> Load the `getting_started` prompt and walk me through this MCP.

The server is self-documenting — its `instructions`, prompts, and embedded `examples://` / `docs://` resources tell the agent every feature it has. Or, to jump straight to a known example:

> Register the example strategy at `examples/strategies/yield-snapshot.js`, run it once, and show me the result.

Claude calls the MCP tools, the runtime executes, and you see the journaled outcome in chat. From there: write more strategies, attach triggers, build flows by talking.

<details>
<summary>Building from source (advanced)</summary>

If you'd rather build the Rust binary yourself instead of using the prebuilt one:

```bash
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp

cast wallet new                       # generate a burner; small amounts only
export EXECUTOR_PRIVATE_KEY=0xyourkey

cp -R .local.example .local
$EDITOR .local/config.toml            # RPC + signer env name
$EDITOR .local/policy.toml            # agent permissions

claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

Requires [Rust](https://rustup.rs/) and (optionally) [Foundry](https://book.getfoundry.sh/) for `cast`.
</details>

---

## 4. Real scenarios

### A. The auto-deposit funnel

Ask Claude: *"when ETH or USDC arrives at my burner, auto-convert to USDC and supply it to Aave — keep ~$0.10 worth of ETH for gas."* Claude registers the strategy plus two log triggers, and the funnel runs itself from there. Every drop hitting the wallet starts earning yield. Your only job is to send funds to the address.

### B. The yield comparator

Ask Claude: *"compare USDC supply APY across Aave, Compound, and Moonwell on Base, hourly for the past 30 days."* It writes a short view, walks past blocks via archive RPC, and hands back a table in seconds — the data already exists onchain.

### C. The instant reactor

Ask Claude: *"watch the Aave oracle for ETH price updates. If price drops more than 2% in one update, repay my borrow."* Claude attaches a log-event trigger. When the event fires onchain, the strategy runs within seconds.

### D. Atomic multi-step

Ask Claude: *"supply 0.1 USDC from my burner to Aave."* Claude returns `[approve, supply]` as two actions; the runtime detects the multi-step plan and bundles them into a single EIP-7702 transaction automatically. Both land together or neither does — no risk window between the approve and the use.

---

## 5. FAQ

**Q. What chains does it support?**
Built and tested on Base (an L2). Any EVM-compatible chain works with a config tweak — Ethereum mainnet, Arbitrum, Optimism, Polygon, etc. Solana and other non-EVM chains are on the roadmap.

**Q. What does it cost to run?**
The software is free. Onchain transactions cost gas — on Base, typically under $0.10 per action. The bigger cost is the wallet itself: start with $5–10 of ETH and a small amount of whatever asset you want to work with.

**Q. Do I need to be a programmer?**
A little. You'll copy commands and edit a config file once. Strategies themselves are short JavaScript functions, which Claude will write from your plain-language description.

**Q. Do I need any API keys?**
- Basic use: no, public RPC works fine.
- For mempool watching, live event subscriptions, or historical data older than a few days, you'll want an [Alchemy](https://www.alchemy.com/) key. Free tier is enough.

**Q. What is "MCP"?**
Model Context Protocol — the standard way Claude Code (and similar AI clients) talk to outside programs. This project is one of those programs. Adding it to Claude Code is essentially telling Claude *"here's a thing you can talk to."*

**Q. How does the policy/safety model work?**
The runtime ships with a deny-by-default policy DSL (allowed chains, contracts, function selectors, native-value caps, ERC20 spend caps). Anything outside the policy is refused before signing. This is the simple baseline; richer permissioning (session keys, agent wallets) lives in the roadmap below.

**Q. Where do I report bugs or ask questions?**
[Open an issue](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues). Use GitHub's private security advisory feature for anything security-sensitive.

---

## Roadmap

What's working sits in §2 above. What's shipped recently:

- ✅ **One-line install for Claude Code** *(v1.3)* — `npx onchain-strategy-mcp init` scaffolds the burner (OS keychain), config, and policy. Deterministic CREATE2 BatchExec address so 7702 batching works on any chain with one optional `deploy-delegate` call. No `cargo build`, no `cast`, no `claude mcp add` by hand.

What's next:

- **Self-documenting MCP session.** The server's `instructions`, prompts, and resource templates ship populated — examples, trigger patterns, action shapes, common pitfalls — so a fresh Claude Code session already *knows* what it can do. No feature should be unreachable just because the agent didn't know it existed.
- **Product homepage.** A simple landing site that explains what this is, shows the headline use cases, and walks newcomers through install in a browser — copy-pasteable commands, screenshots of a real Claude Code session, links to examples. Lowers the "wait, what does this actually do?" barrier before someone touches a terminal.
- **Out-of-band notifications.** A `ctx.notify({channel, message})` strategy API plus first-class adapters for Telegram, Discord, ntfy.sh, and generic webhooks. Triggers fire in the background regardless of whether you're at the keyboard; today the strategy can only journal locally. With this, a strategy can wake *you* up — not just write a log entry the agent surfaces next time you ask.
- **External oracle triggers & data sourcing.** Fire strategies on Chainlink / Pyth / Redstone price updates, off-chain data feeds, or arbitrary HTTPS webhooks. Lets agents react to *real-world* signals — not just onchain state.
- **WaaS (Wallet-as-a-Service) integration.** First-class adapters for Privy, Turnkey, Coinbase MPC, and similar agent-wallet providers. Pushes permissioning (session keys, account-level policies, rotation, recovery) into the wallet layer where it scales — burner + local policy stays for solo operators, WaaS for teams / production / multi-tenant.
- **Non-AMM venue integrations.** First-class support for Hyperliquid (perps), Polymarket (prediction markets), and similar orderbook / specialized venues. Agents place, manage, settle.
- **Cross-chain execution & bridge integration.** Adapters for Across and similar canonical bridges, plus a strategy-level multi-chain action model — a single strategy file can express flows that span chains (e.g., *withdraw from Aave on Base → bridge USDC to Arbitrum → supply to Aave there*). Because cross-chain isn't atomic the way an EIP-7702 batch is, the runtime treats each leg as a separately committed step with explicit fallback semantics at every boundary (retry, refund-path, abort).
- **Non-EVM chains.** Solana first, then a clean abstraction for other ecosystems (Move, CosmWasm, Stellar Soroban). Same strategy / policy / journal model, different signer + RPC backend.

Direction matters more than dates. Discussion and PRs welcome via [issues](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues).

---

## License & credits

Apache 2.0. Built on [rmcp](https://github.com/modelcontextprotocol/rust-sdk), [alloy](https://github.com/alloy-rs/alloy), [rquickjs](https://github.com/DelSkayn/rquickjs), and [foundry](https://github.com/foundry-rs/foundry). Local-first — no servers we run, no accounts you create.

For architecture details (crate layout, trigger pipeline, EIP-7702 internals), see the source under `crates/` and the example contract at `examples/contracts/BatchExec.sol`.
