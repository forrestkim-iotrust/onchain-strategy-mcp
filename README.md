<p align="right"><b>English</b> · <a href="./README_ko.md">한국어</a></p>

# onchain-strategy-mcp

A safe way to let an AI agent (like Claude) act on your crypto wallet.

---

## 1. What is this?

**A local runtime that lets an AI agent (like Claude) actually *do* things onchain — not just write code about it.**

Today, when you ask Claude something like *"swap some ETH for USDC and deposit it into Aave,"* the best it can do is write a Python script and tell you to run it. You set up the RPC, manage the key, handle gas, write logging, debug edge cases. Every iteration is friction. Every meaningful action requires you to be the operator.

This project removes that gap. The AI writes a short JavaScript strategy that describes the *intent*. A small program running quietly on your laptop handles everything else:

- Connects to the chain
- Simulates first
- Checks each action against the policy you wrote (which contracts, which functions, what limits)
- Signs locally — your private key never leaves your machine
- Broadcasts, waits for the receipt, writes every decision to a local journal

The AI can also subscribe to onchain events ("when this token transfer hits, run that strategy") for genuine live automation.

Put another way: **the AI becomes a developer that ships and operates code inside your machine**, and you hold the policy lock.

---

## 2. What can it do?

Anything you can express as a small JavaScript function — but only inside the limits you set. Things people have already built with it:

- **Auto-deposit.** "When ETH lands in this wallet, swap it to USDC and put it into Aave." Runs by itself.
- **Auto-rebalance.** "Move my USDC to whichever lending market pays highest, but only if the gain beats $0.10 in gas."
- **Watch and react.** "If somebody large is about to swap on Uniswap, do X." Or "if Aave's USDC supply rate jumps above 5%, deposit."
- **Compare yields across protocols.** Read Aave, Compound, Moonwell APYs every 30 minutes and log them. Two days later you have a dataset.
- **Historical analysis.** Read protocol state at any past block. Backfill a 30-day APY chart in 5 minutes.
- **Run multiple steps as one transaction.** Approve + supply in a single onchain tx (using EIP-7702, no smart wallet required).

Triggers — what makes things start:

- **You ask Claude to run it** (manual).
- **Every N minutes** (scheduled).
- **When something happens onchain** — a wallet receives money, a token is transferred, a price oracle updates, a specific contract emits an event.
- **A new transaction shows up in the mempool** (for chains where that's useful).

---

## 3. How to use it

You'll need: a Mac/Linux machine, [Rust](https://rustup.rs/), [Foundry](https://book.getfoundry.sh/), and [Claude Code](https://claude.ai/code).

### Step 1 — Get the code, build it

```bash
git clone https://github.com/forrestkim-iotrust/onchain-strategy-mcp.git
cd onchain-strategy-mcp
cargo build --release --bin executor-mcp
```

### Step 2 — Make yourself a small "burner" wallet

This is the wallet the agent will act on. Start with a few dollars of ETH on Base (or whatever chain you're using). Never put your savings here.

```bash
cast wallet new
# Save the address and private key. Send a small amount of ETH to the address.
export EXECUTOR_PRIVATE_KEY=0xyourkey
```

### Step 3 — Write your rules

Copy the example operator config and edit it:

```bash
cp -R .local.example .local
# Edit .local/config.toml — point it at your wallet and your RPC.
# Edit .local/policy.toml — list which contracts the agent is allowed to touch,
# and how much it can spend.
```

The policy is a short text file. By default it says "no". You explicitly add a line for each thing you want to allow: this contract, this function, up to this much.

### Step 4 — Connect to Claude Code

```bash
claude mcp add osmcp \
  -e EXECUTOR_CONFIG=$PWD/.local/config.toml \
  -e EXECUTOR_PRIVATE_KEY=$EXECUTOR_PRIVATE_KEY \
  -- $PWD/target/release/executor-mcp
```

### Step 5 — Ask Claude to do something

Inside Claude Code, say:

> Show me my wallet balance, then register the example strategy at `examples/strategies/yield-snapshot.js` and run it once.

Claude will use the tools this project exposes to read your wallet, check the strategy, and run it. You'll see the result in chat.

That's the basic loop. From there you write more strategies, attach triggers to them, and Claude can wire it all up by talking.

---

## 4. Real use cases (these are already working)

### A. The auto-deposit funnel

You set this up once: any ETH or USDC that lands in your burner wallet automatically gets converted to USDC and deposited into Aave (the lending market). You keep a tiny ETH reserve for gas. After that, you never touch it — every drop that comes in earns yield.

This entire flow runs without you. You just send money to the wallet.

### B. The yield comparator

You ask Claude: *"compare USDC supply yields across Aave, Compound, and Moonwell on Base — show me the last 30 days, hourly."* Claude writes a small JavaScript view, reads each protocol's rate at past blocks via archive RPC, and hands you back a table in seconds. No waiting, no setup — the data already exists onchain.

Then you say: *"now keep watching it forward and notify me if Moonwell jumps above 5%."* That's a separate command — Claude attaches a periodic check (or an onchain event trigger) so future moves don't get missed.

Two pieces, same runtime: **history is one call**, **live monitoring is a trigger**. You decide which you need.

### C. The instant reactor

You ask Claude: *"watch for the Aave oracle updating ETH price. If the price drops more than 2% in one update, repay my borrow."* Claude registers a log-event trigger. When that exact event fires onchain, your strategy runs within seconds.

### D. The multi-step atomic move

Some actions need to happen together or not at all (approve a token, then use it). Normally that's two transactions and a risk window in between. Using EIP-7702 (a 2025 Ethereum feature), this project bundles them into one transaction. Either both happen or neither happens.

---

## 5. FAQ

**Q. Is my money safe?**
The agent can only do what your policy file allows. If you only allow "deposit USDC to Aave," it can't sell your tokens, can't approve arbitrary contracts, and can't send ETH anywhere it likes. Your private key is in an environment variable on your computer — the agent never sees it.

The catch: the policy is only as good as you wrote it. Start narrow. Test with $5 before scaling up.

**Q. Can the AI lose me money through bad decisions?**
Yes — if you let it trade or interact with markets, normal market loss can happen. The protection is against *unauthorized* actions (sending money to a wrong address, hallucinating a malicious contract), not against bad market timing.

**Q. What chains does it work on?**
Built and tested on Base (an L2). Anything EVM-compatible should work with the right config — Ethereum mainnet, Arbitrum, Optimism, Polygon, etc. You change one URL.

**Q. How much does it cost to run?**
The software is free. Onchain transactions cost gas. On Base that's typically less than $0.10 per action. The bigger cost is the burner wallet itself — start with $5–10 of ETH and a small amount of whatever asset you're working with.

**Q. Do I need to be a programmer?**
For the basic version: a little. You need to copy commands and edit a config file once. Strategies themselves are short JavaScript files — Claude can write them for you if you describe what you want.

**Q. Do I need an API key for anything?**
- For basic use: no, you can use public RPC endpoints.
- For watching the mempool, listening for live events, or reading historical data older than a few days, you'll want an [Alchemy](https://www.alchemy.com/) key. Free tier is enough for hobbyist use.

**Q. What does "MCP" mean?**
Model Context Protocol — the way Claude Code (and similar AI clients) talk to outside programs. This project is one such program. When you "add it to Claude Code," you're telling Claude *here's a thing you can talk to.*

**Q. Why not just use a normal trading bot?**
Trading bots are written by you, in code. This lets the AI write and run small strategies on the fly, conversationally, while a hard policy prevents it from doing anything you didn't sanction. It's less about replacing bots and more about lowering the bar to "AI does something onchain for me."

**Q. Is this going to make me rich?**
No. It's a runtime, not an alpha generator. It does what you (or an AI you trust) tell it to. It will not find profitable trades on its own. Anyone who claims otherwise is selling something.

**Q. I found a bug or have a question.**
[Open an issue](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues). For anything security-sensitive, use GitHub's private advisory feature.

---

## Roadmap

What's already working sits in §2 above. What's coming:

- **External oracle triggers & data sourcing.** Fire strategies on Chainlink / Pyth / Redstone price updates, off-chain data feeds, or arbitrary HTTPS webhooks. Lets agents react to *real-world* signals — not just onchain state.
- **Autonomous integrations with non-AMM venues.** First-class support for Hyperliquid (perps), Polymarket (prediction markets), and similar orderbook / specialized venues. Agents place orders, manage positions, settle markets — all under the same policy gate.
- **Non-EVM chains.** Solana first, then a clean abstraction for other ecosystems (Move, CosmWasm, Stellar Soroban). Same strategy/policy/journal model, different signer + RPC backend.

These are big swings. Direction matters more than dates — discussion and PRs welcome via [issues](https://github.com/forrestkim-iotrust/onchain-strategy-mcp/issues).

---

## License & credits

Apache 2.0. Built on [rmcp](https://github.com/modelcontextprotocol/rust-sdk), [alloy](https://github.com/alloy-rs/alloy), [rquickjs](https://github.com/DelSkayn/rquickjs), and [foundry](https://github.com/foundry-rs/foundry). Everything is local-first — no servers we run, no accounts you create.

For the architecture details (crates, trigger pipeline, EIP-7702 specifics), see the source under `crates/` and the example contract in `examples/contracts/BatchExec.sol`.
