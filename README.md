# onchain-strategy-mcp

Agent-facing MCP runtime for composing, running, and managing on-chain strategy programs.

## What It Is

`onchain-strategy-mcp` is a strategy runtime, not a product surface.

It exists to let an agent:

- create a strategy
- register it against an account boundary
- run it on a schedule or event loop
- produce auditable action graphs
- execute those actions safely or externalize them
- inspect state, logs, reports, and receipts later

This repository is not:

- a trading app
- a wallet UI
- a dashboard
- a cloud deployment system
- a strategy marketplace

## Current Design Stance

The current design is built around six ideas.

### 1. App-Agnostic Runtime

The runtime should not need to understand "apps" as product concepts.

Apps are capabilities. Execution is primitives.

That means a strategy may use Aave, Uniswap, Safe, Across, or an unknown contract, but the runtime still reasons in terms of:

- sources
- transforms
- conditions
- flow
- actions
- policy
- execution reports

### 2. JavaScript Is a Strategy DSL

Strategies may be authored as sandboxed JavaScript functions.

But JavaScript is not the execution authority. A strategy function reads through `ctx`, makes decisions, and returns a structured action graph. The runtime validates and executes that graph.

```js
export async function tick(ctx) {
  const usdc = await ctx.source.erc20Balance({
    chainId: 42161,
    token: "USDC",
    account: ctx.account.address,
  });

  if (usdc.gte(100)) {
    return ctx.sequence([
      ctx.action.erc20Approve({
        chainId: 42161,
        token: "USDC",
        spender: ctx.cap.aave.pool,
        amount: "50",
      }),
      ctx.action.contractCall({
        chainId: 42161,
        to: ctx.cap.aave.pool,
        abi: "supply(address,uint256,address,uint16)",
        args: ["USDC", "50", ctx.account.address, 0],
        reason: "supply idle USDC",
      }),
    ]);
  }

  return ctx.noop("conditions not met");
}
```

The important boundary is simple:

- strategy code may propose actions
- strategy code may not directly sign or broadcast transactions
- action graphs must be compiled into normalized actions before policy or execution

### 3. The Runtime Owns Execution Discipline

The runtime is responsible for:

- validating strategy code
- running ticks in a sandbox
- normalizing actions
- simulating execution
- evaluating policy
- managing execution state
- persisting journals and reports
- exposing runtime control such as pause and stop

The runtime does not need to own key custody.

### 4. Execution Transport Is Replaceable

The default philosophy is full runtime execution, but signing and broadcasting should remain adapter-driven.

Supported conceptual modes:

- `managed_execution`
- `detached_signing`
- `detached_execution`

So the runtime owns execution discipline, but not necessarily every transport boundary.

Execution mode is transport ownership. It is separate from execution phase.

Conceptual phases:

- `observe`
- `propose`
- `approve`
- `execute`
- `reconcile`
- `report`

### 5. Account Is the Execution Boundary

The core model is account-scoped.

```text
Account
  StrategyInstance
    Execution
      Action
```

An account is not just a wallet address. It is the boundary for:

- signer references
- policy
- budget
- budget reservations
- nonce lanes
- execution locks
- execution mode
- chain-specific addresses
- strategy-local state

### 6. Contracts Come Before Recipes

The runtime should expose durable contracts before higher-level protocol recipes.

Core contracts:

- `TickInputSnapshot`: what the strategy saw when it made a decision
- `NormalizedAction`: the policy-ready action compiled from a strategy graph
- `ExecutionEnvelope`: the externalizable execution request
- `ExternalExecutionResult`: the result reported by an outside executor

## Execution Lifecycle

Every action should move through a consistent pipeline:

```text
tick(ctx)
  -> source reads
  -> persist tick snapshot
  -> action graph
  -> normalize
  -> simulate
  -> policy check
  -> budget reservation
  -> approval request
  -> sign or externalize
  -> broadcast or externalize
  -> watch or ingest result
  -> persist report
```

The runtime should prefer structured reports over prose-only logs.

## Primitive Model

The runtime should stay small by building around composable primitives.

Conceptual primitive groups:

- `source.*`
- `transform.*`
- `condition.*`
- `flow.*`
- `action.*`
- `policy.*`
- `execution.*`

Capabilities such as `cap:erc20` or `cap:aave` are helpers that produce action graphs. They are not trusted execution authorities.

## Representative Use Cases

The current target is broader than "send a transaction once."

Representative categories:

- observe: balances, allowances, events, positions
- propose: execution plans, revoke proposals, Safe proposals
- execute: gas top-up, idle fund sweep, small managed actions
- reconcile: maintain thresholds, exposures, target balances
- workflow: bridge then act, approve then deposit, external sign then execute

## MCP Surface

The public MCP surface should stay narrow and stable.

Current conceptual groups:

- `account.*`
- `strategy.*`
- `execution.*`
- `policy.*`
- `opcode.*`

The runtime should expose durable contracts first, then higher-level recipes later.

## Non-Goals

This repository should not grow into:

- a landing page
- a dashboard
- wallet onboarding UX
- cloud orchestration
- analytics product surfaces
- exchange-specific product flows embedded into the core runtime

## Document Map

- [README_ko.md](./README_ko.md)
- [FOUNDATIONS_ko.md](./FOUNDATIONS_ko.md)
- [USE_CASES_ko.md](./USE_CASES_ko.md)

## Status

This repository is still in the design phase.

The current MVP direction is:

1. validate and register JavaScript strategies
2. run sandboxed `tick(ctx)` executions
3. persist `TickInputSnapshot`
4. return structured action graphs
5. compile `NormalizedAction`
6. simulate EVM actions
7. enforce policy and account budgets
8. support pluggable signing and broadcasting modes
9. persist execution journals and reports
