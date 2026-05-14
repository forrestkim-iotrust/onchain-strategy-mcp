//! Prompt surface — guided authoring/review pair plus the v1.4 prefetch-style
//! workflow prompts (`getting_started`, `safety_review`, `author_strategy`)
//! and the four self-documenting prompts (`trigger_patterns`,
//! `example_strategies`, `common_pitfalls`).
//!
//! v1.4 Track E1 shifts these from static text to **server-side prefetch +
//! structured cues**: the handler reads live state (strategies, policy) from
//! [`StateStore`] and the loaded [`LoadedPolicy`] and composes a context block
//! the agent can act on immediately. See `.planning/v1.4-AGENT-UX-DESIGN.md`
//! §9 ("Prompts as workflows").
//!
//! Argument schemas come from `executor_core::schema::prompt_args::*` via
//! `Parameters<T>` (so `prompts/list` publishes them automatically). The four
//! self-doc prompts take no arguments — represented by [`EmptyPromptArgs`].

use std::fmt::Write as _;

use executor_core::schema::prompt_args::{
    AuthorStrategyArgs, ReviewEvmStrategyArgs, SafetyReviewArgs, WriteEvmStrategyArgs,
};
use executor_policy::LoadedPolicy;
#[cfg(test)]
use executor_state::StrategySummary;
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

2. **ETH sent TO a 7702-delegated burner reverts** when the delegate has no `receive()`. The bundled `BatchExec` ships with `receive()` — but if you point `[aa].delegate` at a custom contract without one, every native transfer to the EOA reverts. Verify with `ctx.evm.code(burner)` inside `evm_view`.

3. **`ctx.evm.readContract` requires the full ABI fragment**, not a name. Include the matching function entry (with inputs + outputs) in the `abi` array. The runtime selects by `function` name.

4. **`simulation_from` defaults to zero address.** State-dependent calls (price reads on certain oracles, balance-gated views) may revert from `0x0`. Pass `simulation_from: <burner>` explicitly in `evm_view` when simulating state the burner would see.

5. **Don't manually call a batch tool — there isn't one.** Returning `[a, b, c]` from a strategy auto-bundles via EIP-7702 when `[aa].delegate` resolves and code exists at it. If batching silently downgrades to sequential, run `executor-mcp deploy-delegate`.

6. **No `await` inside a strategy.** The JS sandbox is synchronous. All `ctx.evm.*` calls return the resolved value directly.

7. **Policy is deny-by-default.** Adding a new contract or selector requires editing `.local/policy.toml` and restarting the server. `policy_update` returns `-32010 unimplemented` by design in this version.

8. **Strategy ids are 64-char lowercase hex.** Run ids are 26-char Crockford ULIDs. Resource templates reject malformed ids with `-32002 resource_not_found`.

9. **Trigger dedup window:** a trigger that fires while its strategy is still executing is rejected, not queued. Build idempotent strategies; check `trigger-events://{id}` to see suppressed fires."#;

/// v1.4 Track E1: bundle skeleton template for the `author_strategy` prompt.
/// Static for now — Track A wires up the real bundle records/view executor.
/// Kept under ~1KB so the composed prompt body stays under the 3KB budget.
const BUNDLE_SKELETON: &str = r#"```js
// v1.4 strategy bundle shape. Register via:
//   strategy_register({ name, source, records, view })
// where `source` is the legacy `Action[] | "noop"` strategy and
// `records` / `view` are optional bundle members (Track A).

({
  // 1. EXECUTE — the existing strategy function. Returns Action[] | "noop".
  //    Wired to `ctx.actions.*` + `ctx.evm.*`. No await.
  execute: (ctx) => {
    // TODO: read state via ctx.evm.*; decide.
    return "noop";
  },

  // 2. RECORDS — declarative capture of per-run facts. Each entry becomes a
  //    row in `strategy://{id}/records?since=...`. Use to seed view().
  //    Example shapes (Track A2 DSL):
  //      { field: "principal_usdc", from: "action_arg", action: 0, arg: "amount" }
  //      { field: "tx_hash",        from: "tx_hash",    action: 0 }
  records: [
    // TODO: per-run facts to journal.
  ],

  // 3. VIEW — pure JS that aggregates records into a snapshot. Read via
  //    `strategy://{id}/view`. Returns a JSON-shaped object.
  view: (records, ctx) => {
    // TODO: aggregate records into { principal, current_value, ... }.
    return { confidence: "missing", reason: "view not implemented" };
  },
})
```"#;

/// v1.11 Track H: collapse a `McpError` to a one-line reason for the
/// `getting_started` "⚠️ Partial: …" markers. The full structured error
/// envelope would dominate the briefing; the caller can re-read the URI
/// directly via `resources/read` for full forensics.
fn short_err(e: &McpError) -> String {
    let m: &str = e.message.as_ref();
    let line = m.lines().next().unwrap_or("error");
    let trimmed: String = line.chars().take(120).collect();
    if line.chars().count() > 120 {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

/// Build a compact markdown table of currently registered strategies. When the
/// list is empty, returns a one-line "no strategies registered" placeholder so
/// the agent knows it's an empty-state run.
///
/// v1.11 Track H: kept for backwards-compatible test coverage of the
/// empty-state placeholder contract. The live `getting_started` prompt now
/// derives its inventory line from the prefetched `strategy://list` JSON
/// instead of inlining a Markdown table.
#[cfg(test)]
fn format_strategy_table(list: &[StrategySummary]) -> String {
    if list.is_empty() {
        return "_(no strategies registered yet — empty state)_".to_string();
    }
    let mut s = String::new();
    s.push_str("| name | id (short) | description | tags | created_at |\n");
    s.push_str("|------|------------|-------------|------|------------|\n");
    for row in list.iter().take(20) {
        let short_id = if row.id.len() >= 8 {
            &row.id[..8]
        } else {
            row.id.as_str()
        };
        let desc = row.description.as_deref().unwrap_or("");
        let desc_trimmed: String = desc.chars().take(60).collect();
        let tags = row
            .tags
            .as_ref()
            .map(|t| t.join(","))
            .unwrap_or_else(|| "-".to_string());
        let _ = writeln!(
            s,
            "| {} | `{}` | {} | {} | {} |",
            row.name, short_id, desc_trimmed, tags, row.created_at
        );
    }
    if list.len() > 20 {
        let _ = writeln!(
            s,
            "_… {} more rows truncated; read `strategy://list`._",
            list.len() - 20
        );
    }
    s
}

/// Compact summary of the loaded policy (chains / contract count / selectors
/// count / raw_call gate). When the policy field is `None`, surface a
/// fail-closed note — `strategy_run` will refuse anything until it loads.
fn format_policy_summary(policy: Option<&LoadedPolicy>) -> String {
    let Some(p) = policy else {
        return "_(no policy loaded — `strategy_run` will fail-closed with `policy_not_loaded`. \
Set `[policy].path` in `.local/config.toml` and restart.)_"
            .to_string();
    };
    let chains: Vec<String> = p.chains_allow.iter().map(|c| c.to_string()).collect();
    let contracts: usize = p.contracts_by_chain.values().map(|v| v.len()).sum();
    let selectors: usize = p
        .selectors_by_chain_contract
        .values()
        .map(|v| v.len())
        .sum();
    let raw_call_state = if p.raw_call_allow_global {
        "GLOBAL ALLOW (dangerous)"
    } else if p.raw_call_allow.is_empty() {
        "deny (no overrides)"
    } else {
        "deny-by-default with per-contract overrides"
    };
    format!(
        "- chains allowed: {}\n- contracts allowed: {} across {} chain(s)\n- selectors allowed: {}\n- raw_call gate: {}\n",
        if chains.is_empty() {
            "_(none)_".into()
        } else {
            chains.join(", ")
        },
        contracts,
        p.contracts_by_chain.len(),
        selectors,
        raw_call_state,
    )
}

/// Embedded examples → short description, used for the `author_strategy`
/// intent-keyword router. Mirrors the table in
/// `crates/executor-mcp/src/resources.rs` so the prompt body can suggest the
/// most-relevant example without re-reading the include_str! sources.
const EXAMPLES_FOR_INTENT: &[(&str, &str)] = &[
    (
        "eth-funnel",
        "Funnel pattern: when ETH or USDC arrives at the burner, swap excess ETH to USDC and supply to Aave V3. Multi-action [erc20Approve, contractCall] auto-bundles via EIP-7702.",
    ),
    (
        "yield-snapshot",
        "Periodic read-only snapshot: reads supply APY/utilization for Aave/Compound/Moonwell on Base. Returns 'noop'. Pair with an `interval` trigger.",
    ),
    (
        "erc20-approve",
        "Minimal one-action ERC20 approve template. Useful when you just need to grant or revoke an allowance.",
    ),
    (
        "generic-counter-call",
        "Bare-minimum contractCall template against a counter contract on local anvil (chain 31337).",
    ),
];

/// Pick the most relevant example from the embedded set via case-insensitive
/// substring match on the user's intent. Falls back to `yield-snapshot` (the
/// best first example — pure read, no signing, exercises `ctx.evm.readContract`).
fn select_example_for_intent(intent: &str) -> (&'static str, &'static str) {
    let lower = intent.to_ascii_lowercase();
    if lower.contains("approve") || lower.contains("allowance") {
        return EXAMPLES_FOR_INTENT[2];
    }
    if lower.contains("funnel")
        || lower.contains("supply")
        || lower.contains("aave")
        || lower.contains("swap")
        || lower.contains("uniswap")
        || lower.contains("deposit")
    {
        return EXAMPLES_FOR_INTENT[0];
    }
    if lower.contains("yield")
        || lower.contains("apy")
        || lower.contains("apr")
        || lower.contains("snapshot")
        || lower.contains("rate")
        || lower.contains("read")
        || lower.contains("compound")
        || lower.contains("moonwell")
    {
        return EXAMPLES_FOR_INTENT[1];
    }
    if lower.contains("counter") || lower.contains("call") {
        return EXAMPLES_FOR_INTENT[3];
    }
    EXAMPLES_FOR_INTENT[1]
}

/// Extract every `ctx.actions.contractCall({...})` / `ctx.actions.erc20Approve({...})`
/// occurrence, returning `(call_kind, arg_block)` slices where `arg_block` is
/// the literal text of the object passed to the call (best-effort brace
/// matching). No new dependencies — this is a hand-rolled scan, not a real JS
/// parser. When the call's first argument is not an object literal, the entry
/// records `manual review needed` so the agent escalates rather than trusting
/// a partial extraction.
fn extract_action_calls(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let needles = [
        ("ctx.actions.contractCall", "contractCall"),
        ("ctx.actions.erc20Approve", "erc20Approve"),
    ];
    let mut idx = 0usize;
    while idx < bytes.len() {
        let mut next: Option<(usize, &str, &str)> = None;
        for (needle, kind) in &needles {
            if let Some(off) = source[idx..].find(needle) {
                let abs = idx + off;
                if next.map(|(prev, _, _)| abs < prev).unwrap_or(true) {
                    next = Some((abs, needle, kind));
                }
            }
        }
        let Some((abs, needle, kind)) = next else {
            break;
        };
        let mut p = abs + needle.len();
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        if p >= bytes.len() || bytes[p] != b'(' {
            idx = abs + needle.len();
            continue;
        }
        p += 1;
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        let arg_block = if p < bytes.len() && bytes[p] == b'{' {
            let start = p;
            let mut depth = 0i32;
            let mut end = p;
            while end < bytes.len() {
                match bytes[end] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            source[start..end.min(bytes.len())].to_string()
        } else {
            "(non-literal arg — manual review needed)".to_string()
        };
        out.push((kind.to_string(), arg_block));
        idx = abs + needle.len();
    }
    out
}

/// Pull a quoted string literal from `<field>: "<value>"` inside an action
/// arg block. Returns the first match only.
fn extract_field<'a>(block: &'a str, field: &str) -> Option<&'a str> {
    let needle = format!("{field}:");
    let i = block.find(&needle)?;
    let after = &block[i + needle.len()..];
    let trimmed = after.trim_start();
    let quote = trimmed.as_bytes().first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let q = quote as char;
    let inside = &trimmed[1..];
    let end = inside.find(q)?;
    Some(&inside[..end])
}

fn extract_function_name(block: &str) -> Option<&str> {
    extract_field(block, "function")
}

fn extract_address(block: &str) -> Option<&str> {
    extract_field(block, "address")
}

/// Static-analysis heuristics for `safety_review`. Each fires independently
/// and contributes a warning line — none gate the go/no-go alone (that's
/// the final aggregate verdict).
fn surface_common_pitfalls(source: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let trimmed = source.trim_end();
    if trimmed.ends_with(';') {
        findings.push(
            "Trailing `;` at EOF — JS sandbox evaluates the source as one expression; \
             trailing semicolons surface as `-32018 strategy_invalid_output`. Drop it."
                .to_string(),
        );
    }
    if source.contains("await ") || source.contains("await(") {
        findings.push(
            "`await` keyword present — the JS sandbox is synchronous. All `ctx.evm.*` calls return resolved values directly."
                .to_string(),
        );
    }
    if source.contains("module.exports") {
        findings.push(
            "`module.exports` present — strategies are evaluated as a single top-level expression. Remove the CommonJS wrapper.".to_string(),
        );
    }
    if source.contains("amountOutMinimum: \"0\"")
        || source.contains("amountOutMinimum:\"0\"")
        || source.contains("amountOutMinimum: 0")
    {
        findings.push(
            "`amountOutMinimum: 0` detected — unbounded slippage. Hard-code a floor or compute it from a recent oracle read."
                .to_string(),
        );
    }
    findings
}

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
             1. Read the source via `strategy://{id}`.\n\
             2. Read the active policy via `policy://current` and confirm every contract/selector the strategy touches is allowed.\n\
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

    /// v1.11 Track H: canonical first-screen briefing.
    ///
    /// Prefetches live state via `crate::resources::dispatch_uri_to_json` so a
    /// fresh agent has a single composed snapshot — chain/RPC posture, the
    /// strategy + trigger inventory, the policy fingerprint, and a 24h run
    /// rollup — without making N additional resource reads. Composes a
    /// state-conditional playbook (exactly ONE of empty/partial/active) and
    /// the namespace map. Graceful degradation: any prefetch that errors
    /// prepends a `⚠️ Partial: <reason>` note to the relevant section and the
    /// rest of the body renders normally.
    ///
    /// Note: `server.get_info().instructions` is intentionally slim and only
    /// points here — the namespace map and first-action playbook are the
    /// single source of truth in this prompt body.
    #[prompt(
        name = "getting_started",
        description = "Canonical first-screen briefing: current state + first-action playbook + namespace map (single source of truth)."
    )]
    async fn getting_started(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        use crate::resources::{ViewEvm, dispatch_uri_to_json};

        // Build a minimal ViewEvm — the URIs we prefetch here don't actually
        // exercise the JS view sandbox, so a None provider + default config is
        // sufficient. Pulling the real provider would force an RPC connect
        // before the agent has even seen the briefing, which is exactly the
        // posture problem this prompt is trying to surface (not paper over).
        let evm = ViewEvm::default();

        // ── Prefetch — each call independently degrades. ──
        let strategies_json = dispatch_uri_to_json(
            "strategy://list?summary=true".to_string(),
            self.state.clone(),
            evm.clone(),
        )
        .await;
        let triggers_json = dispatch_uri_to_json(
            "trigger://list".to_string(),
            self.state.clone(),
            evm.clone(),
        )
        .await;
        let policy_json = dispatch_uri_to_json(
            "policy://current".to_string(),
            self.state.clone(),
            evm.clone(),
        )
        .await;
        // Last 24h: list_runs with no status filter — the per-status counts
        // come from the same execution://list dispatch via simple status
        // groupings on the response array.
        let since_24h = chrono::Utc::now() - chrono::Duration::hours(24);
        let since_24h_rfc = since_24h.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let runs_json = dispatch_uri_to_json(
            format!("execution://list?since={since_24h_rfc}&limit=200"),
            self.state.clone(),
            evm,
        )
        .await;

        // ── Current state extraction (each block degrades independently). ──
        let chain_label = match self.chain_id().await {
            Ok(id) => format!("chain {id}"),
            Err(_) => "unconfigured".to_string(),
        };
        let rpc_state = {
            let url = self.evm_config.rpc_url.as_str();
            // Mask off the path/query — the host+scheme is enough for posture.
            match self.evm_config.rpc_url.host_str() {
                Some(host) => format!(
                    "{scheme}://{host}",
                    scheme = self.evm_config.rpc_url.scheme(),
                ),
                None => url.to_string(),
            }
        };

        // strategies + 24h rollup share the prefetch. Extract counts without
        // re-reading the DB.
        let mut strategies_warning: Option<String> = None;
        let active_strategy_count: usize = match &strategies_json {
            Ok(v) => v
                .get("count")
                .and_then(|c| c.as_u64())
                .unwrap_or(0) as usize,
            Err(e) => {
                strategies_warning =
                    Some(format!("strategy://list prefetch failed: {}", short_err(e)));
                0
            }
        };

        let mut triggers_warning: Option<String> = None;
        let (active_trigger_count, kind_counts) = match &triggers_json {
            Ok(v) => {
                let triggers = v.get("triggers").and_then(|t| t.as_array());
                let total = triggers.map(|t| t.len()).unwrap_or(0);
                // (mempool, log, interval, manual)
                let mut counts: [usize; 4] = [0; 4];
                if let Some(arr) = triggers {
                    for t in arr {
                        let enabled = t
                            .get("enabled")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(true);
                        if !enabled {
                            continue;
                        }
                        match t.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
                            "mempool" => counts[0] += 1,
                            "log" => counts[1] += 1,
                            "interval" => counts[2] += 1,
                            "manual" => counts[3] += 1,
                            _ => {}
                        }
                    }
                }
                let active_total: usize = counts.iter().sum();
                (
                    // If we filtered to enabled-only and that came out 0 but
                    // raw total was nonzero, prefer the raw total so the
                    // briefing surfaces ALL triggers (UI will filter).
                    if active_total == 0 { total } else { active_total },
                    counts,
                )
            }
            Err(e) => {
                triggers_warning =
                    Some(format!("trigger://list prefetch failed: {}", short_err(e)));
                (0, [0; 4])
            }
        };

        let mut policy_warning: Option<String> = None;
        let (policy_label, policy_rules_count): (String, usize) = match &policy_json {
            Ok(v) => {
                let loaded = v.get("loaded").and_then(|b| b.as_bool()).unwrap_or(false);
                if !loaded {
                    ("not loaded".to_string(), 0)
                } else {
                    let rev = v
                        .get("revision_id")
                        .and_then(|r| r.as_str())
                        .unwrap_or("");
                    let short = if rev.len() >= 8 { &rev[..8] } else { rev };
                    // Best-effort rules count: sum of allow lists across the
                    // policy body when present. Falls back to 0 on shape drift.
                    let rules = v
                        .get("policy")
                        .and_then(|p| p.as_object())
                        .map(|obj| {
                            let mut n = 0usize;
                            for (_k, val) in obj {
                                if let Some(arr) = val.as_array() {
                                    n += arr.len();
                                }
                            }
                            n
                        })
                        .unwrap_or(0);
                    let label = if short.is_empty() {
                        "active".to_string()
                    } else {
                        format!("rev {short}")
                    };
                    (label, rules)
                }
            }
            Err(e) => {
                policy_warning =
                    Some(format!("policy://current prefetch failed: {}", short_err(e)));
                ("unknown".to_string(), 0)
            }
        };

        let mut runs_warning: Option<String> = None;
        let (runs_total, runs_ok, runs_failed, runs_noop) = match &runs_json {
            Ok(v) => {
                let arr = v.get("runs").and_then(|r| r.as_array());
                let mut total = 0usize;
                let mut ok = 0usize;
                let mut failed = 0usize;
                let mut noop = 0usize;
                if let Some(rows) = arr {
                    for r in rows {
                        total += 1;
                        match r.get("status").and_then(|s| s.as_str()).unwrap_or("") {
                            "succeeded" => ok += 1,
                            "failed" => failed += 1,
                            "noop" => noop += 1,
                            _ => {}
                        }
                    }
                }
                (total, ok, failed, noop)
            }
            Err(e) => {
                runs_warning =
                    Some(format!("execution://list prefetch failed: {}", short_err(e)));
                (0, 0, 0, 0)
            }
        };

        // ── Compose "Current state" with prepended warnings on failure. ──
        let mut current_state = String::new();
        if let Some(w) = &strategies_warning {
            let _ = writeln!(current_state, "⚠️ Partial: {w}");
        }
        if let Some(w) = &triggers_warning {
            let _ = writeln!(current_state, "⚠️ Partial: {w}");
        }
        if let Some(w) = &policy_warning {
            let _ = writeln!(current_state, "⚠️ Partial: {w}");
        }
        if let Some(w) = &runs_warning {
            let _ = writeln!(current_state, "⚠️ Partial: {w}");
        }
        let _ = writeln!(current_state, "- Chain: {chain_label}");
        let _ = writeln!(current_state, "- Burner: see `policy://current` (signer field) or `.local/config.toml`");
        let _ = writeln!(current_state, "- RPC: {rpc_state}");
        let _ = writeln!(current_state, "- Active strategies: {active_strategy_count}");
        let _ = writeln!(
            current_state,
            "- Active triggers: {active_trigger_count} (mempool={}, log={}, interval={}, manual={})",
            kind_counts[0], kind_counts[1], kind_counts[2], kind_counts[3],
        );
        let _ = writeln!(
            current_state,
            "- Policy: {policy_label} · {policy_rules_count} allow-rules"
        );
        let _ = writeln!(
            current_state,
            "- Last 24h: {runs_total} runs ({runs_ok} succeeded · {runs_failed} failed · {runs_noop} noop)"
        );

        // ── First-action playbook (exactly ONE of empty/partial/active). ──
        let playbook = if active_strategy_count == 0 {
            // Empty state: zero strategies registered.
            "### Empty state (0 strategies):\n\
             1. Read `examples://strategies/eth-funnel` to see a starter pattern.\n\
             2. Use the `strategy_register` tool to register your first strategy.\n\
             3. (Optional) Attach a trigger via `trigger_register`."
        } else if active_trigger_count == 0 {
            // Partial: strategies but no active triggers.
            "### Partial state (strategies registered but 0 triggers active):\n\
             1. Inspect each strategy: `strategy://list?status=active&summary=true`.\n\
             2. Attach triggers via `trigger_register` — see `docs://trigger-model` for kinds."
        } else {
            // Active: strategies + triggers running.
            "### Active state (strategies + triggers running):\n\
             1. One-screen status: call prompt `inventory`.\n\
             2. Recent failures? call prompt `triage_run` with a run_id from \
                `execution://list?status=failed`.\n\
             3. Adjust thresholds: prompt `tune_thresholds` per-strategy."
        };

        let body = format!(
            "# osmcp — local onchain strategy runtime\n\n\
             A local MCP runtime that executes JavaScript strategies onchain. You author short\n\
             JS functions describing intent; the runtime simulates, signs with a local burner,\n\
             broadcasts, journals every decision, and (for multi-step plans) auto-bundles into\n\
             one atomic EIP-7702 transaction. Keys in the OS keychain; state in SQLite. No\n\
             remote services.\n\n\
             ## Current state\n\n\
             {current_state}\n\
             ## First-action playbook\n\n\
             {playbook}\n\n\
             ## Namespace map (use this to discover URIs)\n\n\
             - `runtime://*` — system state (status, signals, recent)\n\
             - `strategy://`, `trigger://`, `execution://`, `journal://`, `portfolio://`, `policy://` — domain objects\n\
             - `docs://*`, `examples://*` — reference\n\n\
             Call `resources/list` for the stable entrypoints; `resources/templates/list` for parameterized URIs.\n\n\
             ## Where to go next\n\n\
             - Author a new strategy → prompt `author_strategy`\n\
             - Review before registering → prompt `safety_review`\n\
             - Failed run forensics → prompt `triage_run`\n\
             - Threshold tuning → prompt `tune_thresholds`\n",
        );

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description(
            "Canonical first-screen briefing — composes runtime + strategy + policy in one call.",
        ))
    }

    /// v1.4 Track E1: vet a proposed strategy source before `strategy_register`.
    /// Body inlines a static-analysis-style checklist of the submitted source
    /// + per-action policy verdict + a go/no-go recommendation.
    #[prompt(
        name = "safety_review",
        description = "Vet a proposed strategy source — itemized static analysis + policy verdict + go/no-go."
    )]
    async fn safety_review(
        &self,
        Parameters(args): Parameters<SafetyReviewArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let actions = extract_action_calls(&args.source);
        let pitfalls = surface_common_pitfalls(&args.source);
        let (policy_loaded, policy_summary) = {
            let guard = self.policy.read().await;
            (guard.is_some(), format_policy_summary(guard.as_ref()))
        };

        let mut action_block = String::new();
        if actions.is_empty() {
            action_block.push_str(
                "_(no `ctx.actions.*` calls extracted — strategy may be pure-read (`noop`), or the literal call sites are non-trivial. Manual review needed.)_\n",
            );
        } else {
            for (i, (kind, block)) in actions.iter().enumerate() {
                let _ = writeln!(action_block, "### action {} — `ctx.actions.{}`", i + 1, kind);
                let addr_field = if kind == "erc20Approve" {
                    "token"
                } else {
                    "address"
                };
                let address_lit = if kind == "erc20Approve" {
                    extract_field(block, "token")
                } else {
                    extract_address(block)
                };
                let function_lit = extract_function_name(block);
                let _ = writeln!(
                    action_block,
                    "- target ({addr_field}): {}",
                    address_lit
                        .map(|a| format!("`{a}`"))
                        .unwrap_or_else(|| "_manual review needed_".into())
                );
                if let Some(f) = function_lit {
                    let _ = writeln!(action_block, "- function: `{f}`");
                }
                let policy_note = if !policy_loaded {
                    "_policy not loaded — `strategy_run` will refuse before broadcast._"
                        .to_string()
                } else if address_lit.is_none() {
                    "_address not extractable — re-check after rendering literal addresses._"
                        .to_string()
                } else {
                    "Cross-check the target above against the policy block below; if the address is absent from `contracts_by_chain`, this action will be refused.".to_string()
                };
                let _ = writeln!(action_block, "- policy verdict: {policy_note}");
                action_block.push('\n');
            }
        }

        let mut pitfall_block = String::new();
        if pitfalls.is_empty() {
            pitfall_block.push_str("_(no common pitfalls detected — see the `common_pitfalls` prompt for the full list.)_\n");
        } else {
            for f in &pitfalls {
                let _ = writeln!(pitfall_block, "- {f}");
            }
        }

        let preview: String = args.source.chars().take(600).collect();
        let truncated = args.source.chars().count() > 600;

        let verdict = if !pitfalls.is_empty() {
            "**NO-GO** — at least one static-analysis finding above must be addressed before register."
        } else if !policy_loaded {
            "**NO-GO** — policy not loaded; `strategy_run` will fail-closed regardless of source quality."
        } else if actions.is_empty() {
            "**GO (read-only)** — no signing actions detected. Safe to register; runs will journal reads without policy gating."
        } else {
            "**CAUTION** — actions extracted but no obvious pitfalls. Verify each target/selector against the policy block above before registering."
        };

        let body = format!(
            "Safety review of a *proposed* strategy source (pre-register).\n\n\
             ## Submitted source ({len} chars{trunc})\n\n```js\n{preview}\n```\n\n\
             ## Extracted `ctx.actions.*` calls\n\n{actions}\n\
             ## Static-analysis findings\n\n{pitfalls}\n\
             ## Active policy (inline)\n\n{policy}\n\n\
             ## Verdict\n\n{verdict}\n\n\
             ## Next step\n\n\
             Once Track G ships, prefer `strategy_register({{ dry_run: true, name, source }})` to run the source through the sandbox + policy before persisting. That flag may not yet be available — until then, register, then immediately call `strategy_run` and inspect `execution://{{run_id}}` for the policy verdict.",
            len = args.source.chars().count(),
            trunc = if truncated {
                ", truncated above to 600 chars"
            } else {
                ""
            },
            preview = preview,
            actions = action_block,
            pitfalls = pitfall_block,
            policy = policy_summary,
            verdict = verdict,
        );

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("Pre-register safety review: static analysis + policy cross-check"))
    }

    /// v1.4 Track E1: bundle-shaped authoring guide. Inlines the
    /// `{ name, execute, records, view }` skeleton + the most relevant
    /// example for the declared intent.
    #[prompt(
        name = "author_strategy",
        description = "Bundle skeleton template + intent-relevant example + live policy constraints."
    )]
    async fn author_strategy(
        &self,
        Parameters(args): Parameters<AuthorStrategyArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let (example_name, example_desc) = select_example_for_intent(&args.intent);
        let policy_summary = {
            let guard = self.policy.read().await;
            format_policy_summary(guard.as_ref())
        };

        let body = format!(
            "Author a v1.4 strategy bundle for the following intent.\n\n\
             ## Intent\n\n> {intent}\n\n\
             ## Bundle skeleton (v1.4 shape)\n\n{skeleton}\n\n\
             ## Most relevant embedded example\n\n\
             **`examples://strategies/{example_name}`** — {example_desc}\n\n\
             Read the full source via that resource before adapting. The embedded copy matches the binary; the on-disk repo may not.\n\n\
             ## Policy constraints (the burner's allow list)\n\n{policy}\n\n\
             Author actions that target only the chains/contracts/selectors listed above. Anything else is refused before broadcast (`-32017 policy_*`).\n\n\
             ## Authoring rules\n\n\
             - The `execute` function returns `Action[] | \"noop\"` synchronously. No `await`.\n\
             - Use `ctx.evm.*` for reads (supports `blockTag`).\n\
             - Use `ctx.actions.contractCall` / `ctx.actions.erc20Approve` for state-changing actions.\n\
             - Multi-action returns (`[approve, contractCall]`) auto-bundle via EIP-7702 — no manual batch call.\n\
             - Drop the trailing `;` at EOF; the source is one expression.\n\n\
             ## Bundle docs\n\n\
             See `docs://strategy-bundle` for the full bundle contract (may not yet be available pre-A1; until then this skeleton plus the example above are the source of truth).\n\n\
             Once authored, run the `safety_review` prompt against the source before calling `strategy_register`.",
            intent = args.intent,
            skeleton = BUNDLE_SKELETON,
            example_name = example_name,
            example_desc = example_desc,
            policy = policy_summary,
        );

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("Bundle skeleton + intent-relevant example + policy constraints"))
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

// ────────────────────────── unit tests ──────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_action_calls_finds_contract_calls() {
        let src = r#"
        ((A) => (ctx) => [
            ctx.actions.contractCall({
                address: "0xabc",
                abi: [],
                function: "supply",
                args: [],
            }),
            ctx.actions.erc20Approve({
                token: "0xdef",
                spender: "0xfeed",
                amount: "100",
            }),
        ])("foo")
        "#;
        let calls = extract_action_calls(src);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "contractCall");
        assert_eq!(calls[1].0, "erc20Approve");
        assert!(calls[0].1.contains("\"0xabc\""));
        assert!(calls[1].1.contains("\"0xdef\""));
    }

    #[test]
    fn extract_function_name_works() {
        let block = r#"{ address: "0x1", function: "supply", args: [] }"#;
        assert_eq!(extract_function_name(block), Some("supply"));
    }

    #[test]
    fn extract_address_works() {
        let block = r#"{ address: "0xabc", function: "x" }"#;
        assert_eq!(extract_address(block), Some("0xabc"));
    }

    #[test]
    fn surface_common_pitfalls_flags_trailing_semicolon() {
        let src = "((ctx) => \"noop\");";
        let pitfalls = surface_common_pitfalls(src);
        assert!(pitfalls.iter().any(|p| p.contains("Trailing `;`")));
    }

    #[test]
    fn surface_common_pitfalls_flags_unbounded_slippage() {
        let src = r#"ctx.actions.contractCall({ amountOutMinimum: "0" })"#;
        let pitfalls = surface_common_pitfalls(src);
        assert!(pitfalls.iter().any(|p| p.contains("amountOutMinimum")));
    }

    #[test]
    fn select_example_for_intent_routes_known_keywords() {
        assert_eq!(
            select_example_for_intent("ETH to USDC funnel").0,
            "eth-funnel"
        );
        assert_eq!(
            select_example_for_intent("APY snapshot of lending markets").0,
            "yield-snapshot"
        );
        assert_eq!(
            select_example_for_intent("Approve USDC for router").0,
            "erc20-approve"
        );
        assert_eq!(
            select_example_for_intent("call my counter").0,
            "generic-counter-call"
        );
        assert_eq!(
            select_example_for_intent("something completely unrelated").0,
            "yield-snapshot"
        );
    }

    #[test]
    fn format_strategy_table_empty() {
        let s = format_strategy_table(&[]);
        assert!(s.contains("no strategies registered"));
    }

    #[test]
    fn format_policy_summary_no_policy() {
        let s = format_policy_summary(None);
        assert!(s.contains("no policy loaded"));
    }

    /// v1.4 E1 budget: prompt bodies must stay under ~3KB each so a fetch
    /// doesn't blow the agent's per-prompt token budget. The composed
    /// `safety_review` and `author_strategy` bodies vary with input — these
    /// guards exercise the typical-size paths (empty policy, modest source,
    /// short intent) and assert the result stays under 3500 bytes.
    #[test]
    fn bundle_skeleton_fits_under_budget() {
        // The skeleton alone is the largest static contribution to
        // author_strategy; verifying its size locks in the headroom.
        assert!(
            BUNDLE_SKELETON.len() < 1200,
            "bundle skeleton too large: {} bytes",
            BUNDLE_SKELETON.len()
        );
    }

    #[test]
    fn trigger_patterns_body_under_budget() {
        assert!(
            TRIGGER_PATTERNS_BODY.len() < 3072,
            "trigger_patterns body too large: {} bytes",
            TRIGGER_PATTERNS_BODY.len()
        );
    }
}
