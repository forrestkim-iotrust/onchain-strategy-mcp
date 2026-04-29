# onchain-strategy-mcp

Agent-facing MCP runtime for writing, running, and auditing local EVM automation strategies.

## What this runtime does

`onchain-strategy-mcp` is a local-first MCP runtime. An agent registers sandboxed JavaScript strategy source, runs it through the runtime, and receives structured reports instead of prose-only logs.

The shipped v1 loop is:

```text
strategy_register
  -> strategy_run
  -> sandboxed JS returns Action[]
  -> action validation
  -> EVM simulation
  -> policy check
  -> local hot-wallet signing
  -> broadcast to configured RPC
  -> receipt wait
  -> execution_get or execution://{run_id}
```

Strategy JavaScript proposes actions through `ctx`; it does not receive private keys and cannot directly sign, broadcast, read files, access process APIs, or use arbitrary network clients. The runtime records strategy runs, source reads, policy/simulation decisions, execution action rows, receipts, and errors in local SQLite state.

This repository is not a hosted custody service, marketplace, or protocol recipe catalog. It is a local MCP runtime, not a product UI or long-running automation daemon.

## Local hot-wallet safety model

v1 uses a local hot-wallet private key only when a non-noop strategy reaches the approved execution path. Simulation and policy checks must pass before signing.

Signer configuration stores only the environment variable name:

```toml
[signer]
private_key_env = "EXECUTOR_PRIVATE_KEY"
receipt_timeout_ms = 120000
```

Raw private key values belong only in the operator environment variable named by `[signer].private_key_env`. Never commit raw private keys in `config.example.toml`, runtime config, strategy JavaScript, README snippets, logs, prompts, or issue reports. Strategy files may include public addresses and placeholders, but not signer secrets.

Policy is deny-by-default. Keep local policies narrow: exact chain IDs, exact contract addresses, allowed selectors, native-value limits, ERC20 spend limits, and `raw_call` disabled unless explicitly needed.

## Local Anvil examples

Checked-in examples demonstrate the runtime loop against local Anvil-style fixtures:

- `examples/strategies/erc20-approve.js` builds an ERC20 approve action.
- `examples/strategies/generic-counter-call.js` builds a generic ABI `increment()` contract call.
- `examples/policies/local-anvil.toml` shows a chain `31337` policy with exact selector allowlists.
- `config.example.toml` shows local state, EVM RPC, policy, and signer sections without secrets.

Typical agent/operator flow:

1. Start Anvil and deploy or substitute local contracts for the placeholder addresses in the strategy and policy examples.
2. Copy `config.example.toml` to a local untracked config file if needed.
3. Set `EXECUTOR_PRIVATE_KEY` in the operator shell only; do not write the raw key into committed files.
4. Use the MCP prompt `write_evm_strategy` when drafting strategy JS and `review_evm_strategy` before registration.
5. Register the strategy with `strategy_register` using one of the checked-in sources, such as `examples/strategies/erc20-approve.js`.
6. Run it with `strategy_run`.
7. Inspect the returned run ID with `execution_get` or the resource `execution://{run_id}` to verify receipt-backed action reports.
8. Review journal resources for source reads, validation, simulation, policy, and action outcomes.

The example verification test consumes `examples/strategies/erc20-approve.js` and `examples/strategies/generic-counter-call.js` directly, proving they are runtime inputs rather than disconnected snippets.

## Verification

Run these safety and regression checks before trusting changes to examples, policy, simulation, sandboxing, or execution reporting:

```bash
cargo test -p executor-mcp --features anvil-tests --test verification_examples -- --nocapture
cargo test -p executor-mcp --test verification_safety
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The verification suites cover local examples, `verification_examples`, policy/simulation/sandbox boundaries in `verification_safety`, workspace regression coverage, and lint cleanliness.

## Document map

- [README_ko.md](./README_ko.md) — Korean overview and usage notes.
- [AGENTS.md](./AGENTS.md) — agent/operator workflow and command checklist.
- [FOUNDATIONS_ko.md](./FOUNDATIONS_ko.md) — project foundation notes.
- [USE_CASES_ko.md](./USE_CASES_ko.md) — use-case notes.
