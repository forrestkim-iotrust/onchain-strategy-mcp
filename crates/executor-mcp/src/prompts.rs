//! Prompt surface — guided authoring/review pair plus four self-documenting
//! prompts (`getting_started`, `trigger_patterns`, `example_strategies`,
//! `common_pitfalls`) so a fresh agent can discover the runtime end-to-end
//! without prior context.
//!
//! Argument schemas come from `executor_core::schema::prompt_args::*` via
//! `Parameters<T>` (so `prompts/list` publishes them automatically). The four
//! self-doc prompts take no arguments — represented by [`EmptyPromptArgs`].

use executor_core::schema::prompt_args::{ReviewEvmStrategyArgs, WriteEvmStrategyArgs};
use rmcp::{
    ErrorData as McpError, RoleServer,
    handler::server::wrapper::Parameters,
    model::{GetPromptResult, PromptMessage, PromptMessageRole},
    prompt, prompt_router,
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ExecutorServer;

/// Argless prompt payload. rmcp's `#[prompt]` macro requires a `Parameters<T>`
/// even for prompts that take no input — `EmptyPromptArgs` keeps the schema
/// surface honest (`{}` with no required fields).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "No arguments.")]
pub struct EmptyPromptArgs {}

const GETTING_STARTED_BODY: &str = r#"You are connected to the Onchain Strategy MCP runtime. Orient yourself in this order:

1. Read the server `instructions` you received at `initialize` — it covers the strategy/trigger/policy model, the `ctx.actions` and `ctx.evm` API surface, the tool list, and all resource templates.

2. Inspect the live state:
   - `strategy_list` — what is already registered?
   - `trigger_list` — what is firing automatically?
   - `policy_get` — what is the agent allowed to do (chains, contracts, selectors, caps)?

3. If `strategy_list` is empty, register the bundled `yield-snapshot` example to prove the loop end-to-end:
   - Read the source via `examples://strategies/yield-snapshot`.
   - Call `strategy_register({ name: "yield-snapshot", source: <that source> })`.
   - Call `strategy_run({ strategy_id: <returned id> })`.
   - Read `execution://{run_id}` and `journal://{run_id}` to show the user the result.

4. Then turn to the user's intent. Strategies are short JS functions returning `Action[] | "noop"`. WRITE THE STRATEGY YOURSELF from the user's description — do not ask the user for code. The canonical use cases on this runtime:

   - **Funnel** — auto-convert/supply every inbound balance (see `examples://strategies/eth-funnel`).
   - **Yield comparator** — read multiple markets at any block, return a snapshot (see `examples://strategies/yield-snapshot`).
   - **Instant reactor** — log/mempool trigger fires the strategy within seconds of an onchain event.
   - **Atomic multi-step** — `[approve, supply]` etc. lands as one tx via auto-7702 batching.

5. Pair every strategy with the right trigger if it needs to run autonomously. Load the `trigger_patterns` prompt for the decision table.

6. Before broadcasting unfamiliar shapes, dry-run with `strategy_run` (it simulates each action through policy gating before signing).

If anything reverts, fails, or returns a -32017/-32018, load the `common_pitfalls` prompt before iterating."#;

const TRIGGER_PATTERNS_BODY: &str = r#"Pick the trigger kind that matches the *source of change*, not the cadence:

| user intent                                      | trigger kind | typical config                              |
|--------------------------------------------------|--------------|---------------------------------------------|
| "run this once now"                              | manual       | none — just call `strategy_run`             |
| "every N minutes / hourly snapshot"              | interval     | `{ interval_ms: 60_000 }`                   |
| "react to an oracle / Transfer / state event"    | log          | `{ address, topics[] }` filter              |
| "front-run / detect a pending tx / mempool sig"  | mempool      | predicate over `{ to, input, value, from }` |

Concrete examples:

- **Incoming-fund detection (funnel pattern):** use `log` on the ERC20 contract with `topics = [Transfer, *, burner]` — catches confirmed deposits. Use `mempool` only when you need to *front-run* a pending tx; for "supply when funds arrive" the confirmed log is correct and avoids reorg races.
- **Oracle reaction:** `log` on the oracle aggregator address filtered by the price-update event topic. The strategy reads the new price via `ctx.evm.readContract` and decides.
- **Periodic snapshot / yield comparator:** `interval` with `interval_ms` matching the rate of change you care about (hourly = `3_600_000`).
- **One-shot or human-in-the-loop:** `manual`. No trigger registered; agent invokes `strategy_run` on demand.

Concurrency: a trigger that fires while a previous run of the same strategy is still in flight is skipped and journaled as a `dedup_rejected` event. Inspect via `trigger-events://{trigger_id}`.

Mempool requires `[trigger].mempool_wss_url` in `.local/config.toml` (an Alchemy or equivalent WSS endpoint). Without it, mempool workers warn-log and stay idle."#;

const EXAMPLE_STRATEGIES_BODY: &str = r#"Embedded reference strategies live at `examples://strategies/{name}`. Always read the source via that resource before adapting — the embedded copy matches the binary, the on-disk repo may not.

- **`yield-snapshot`** — reads supply APR/utilization for a Compound v3 (Comet) market across blocks. Pure-read strategy returning `"noop"`. Best first example: no signing, no policy gates, exercises `ctx.evm.readContract` with `blockTag`.

- **`eth-funnel`** — when ETH or USDC lands at the burner, swap to USDC and supply to Aave. Demonstrates the multi-step `[erc20Approve, contractCall]` pattern that auto-bundles via EIP-7702.

- **`erc20-approve`** — minimal one-action strategy showing `ctx.actions.erc20Approve` standalone. Useful as a template when you just need to grant or revoke an allowance.

- **`generic-counter-call`** — minimal one-action `ctx.actions.contractCall` against a counter contract. Use as the bare-minimum template for any single-call automation.

Reference contracts at `examples://contracts/{name}`:

- **`BatchExec`** — the EIP-7702 delegate contract. Deployed deterministically via CREATE2 at `0x821fd81668823A3c5a65E95CeD5F050Ee54a4f53`. Run `npx onchain-strategy-mcp deploy-delegate` once per chain to put bytecode at that address.

When adapting an example: copy the source, edit addresses/ABIs for the target chain, register it under a new name. Do NOT mutate the embedded source in place — register fresh."#;

const COMMON_PITFALLS_BODY: &str = r#"Mistakes the runtime forgives poorly:

1. **Trailing semicolon at EOF in strategy source.** The JS host evaluates the source as a single expression. A trailing `;` after the last expression flips the program value to `undefined` and surfaces as `-32018 strategy_invalid_output`. Drop the trailing semicolon.

2. **ETH sent TO a 7702-delegated burner reverts** when the delegate has no `receive()`. The bundled `BatchExec` ships with `receive()` — but if you point `[aa].delegate` at a custom contract without one, every native transfer to the EOA reverts. Verify with `evm_code` on the burner.

3. **`ctx.evm.readContract` requires the full ABI fragment**, not a name. Include the matching function entry (with inputs + outputs) in the `abi` array. The runtime selects by `function` name.

4. **`simulation_from` defaults to zero address.** State-dependent calls (price reads on certain oracles, balance-gated views) may revert from `0x0`. Pass `simulation_from: <burner>` explicitly in `evm_view` / `evm_read` when simulating state the burner would see.

5. **Don't manually call a batch tool — there isn't one.** Returning `[a, b, c]` from a strategy auto-bundles via EIP-7702 when `[aa].delegate` resolves and code exists at it. If batching silently downgrades to sequential, run `executor-mcp deploy-delegate`.

6. **No `await` inside a strategy.** The JS sandbox is synchronous. All `ctx.evm.*` calls return the resolved value directly.

7. **Policy is deny-by-default.** Adding a new contract or selector requires editing `.local/policy.toml` and restarting the server. `policy_update` returns `-32010 unimplemented` by design in this version.

8. **Strategy ids are 64-char lowercase hex.** Run ids are 26-char Crockford ULIDs. Resource templates reject malformed ids with `-32002 resource_not_found`.

9. **Trigger dedup window:** a trigger that fires while its strategy is still executing is rejected, not queued. Build idempotent strategies; check `trigger-events://{id}` to see suppressed fires."#;

#[prompt_router(vis = "pub(crate)")]
impl ExecutorServer {
    #[prompt(
        name = "write_evm_strategy",
        description = "Author a new EVM automation strategy from a free-form intent."
    )]
    async fn write_evm_strategy(
        &self,
        Parameters(args): Parameters<WriteEvmStrategyArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let chain = args.chain_hint.as_deref().unwrap_or("base");
        let body = format!(
            "Author a JS strategy for the Onchain Strategy MCP runtime.\n\
             Target chain hint: {chain}\n\
             Intent: {intent}\n\n\
             Requirements:\n\
             - Return an array of `ctx.actions.contractCall` / `ctx.actions.erc20Approve` items, or `\"noop\"`.\n\
             - Use `ctx.evm.*` for any read (supports `blockTag`).\n\
             - No `await`, no `module.exports`, no trailing semicolon on the final expression.\n\
             - Keep the body short and declarative; multi-step plans auto-bundle via EIP-7702.\n\
             - When unsure, read `examples://strategies/eth-funnel` and `examples://strategies/yield-snapshot` first.\n\n\
             Output: the strategy source ready for `strategy_register`, followed by a one-paragraph explanation.",
            chain = chain,
            intent = args.intent,
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("Guided strategy authoring"))
    }

    #[prompt(
        name = "review_evm_strategy",
        description = "Review an existing EVM automation strategy for safety, correctness, and policy fit."
    )]
    async fn review_evm_strategy(
        &self,
        Parameters(args): Parameters<ReviewEvmStrategyArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let body = format!(
            "Review strategy `{id}` registered on this runtime.\n\n\
             Steps:\n\
             1. Read the source via `strategy_get` or `strategy://{id}`.\n\
             2. Read the active policy via `policy_get` and confirm every contract/selector the strategy touches is allowed.\n\
             3. Re-read each `ctx.evm.*` call: is `blockTag` correct? Is `simulation_from` set when the read is state-dependent?\n\
             4. For each returned action: check that decimals / units match the token, that `value` is in wei, that multi-step ordering is safe (approve before use).\n\
             5. Re-check error envelopes via the last few `execution://{{run_id}}` reports for any prior runs.\n\n\
             Output: a structured review with findings flagged as BLOCKER / WARN / NIT, and a recommended patch if anything is BLOCKER.",
            id = args.strategy_id,
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("Guided strategy review"))
    }

    #[prompt(
        name = "getting_started",
        description = "Orient a fresh agent: inspect live state, run the bundled example, then author from user intent."
    )]
    async fn getting_started(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            GETTING_STARTED_BODY,
        )])
        .with_description("End-to-end orientation"))
    }

    #[prompt(
        name = "trigger_patterns",
        description = "Decision table for picking the right trigger kind (manual / interval / log / mempool)."
    )]
    async fn trigger_patterns(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            TRIGGER_PATTERNS_BODY,
        )])
        .with_description("Trigger selection guide"))
    }

    #[prompt(
        name = "example_strategies",
        description = "Menu of embedded reference strategies + contracts, with one-line descriptions."
    )]
    async fn example_strategies(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            EXAMPLE_STRATEGIES_BODY,
        )])
        .with_description("Reference strategies catalogue"))
    }

    #[prompt(
        name = "common_pitfalls",
        description = "Mistakes the runtime forgives poorly — read before iterating on a failing strategy."
    )]
    async fn common_pitfalls(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            COMMON_PITFALLS_BODY,
        )])
        .with_description("Top-N footguns"))
    }
}
