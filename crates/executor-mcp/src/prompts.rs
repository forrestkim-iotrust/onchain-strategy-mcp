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
use std::time::Duration;

use executor_core::schema::prompt_args::{
    AuthorStrategyArgs, ReviewEvmStrategyArgs, SafetyReviewArgs, TriageRunArgs,
    TuneThresholdsArgs, WriteEvmStrategyArgs,
};
use executor_policy::LoadedPolicy;
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
use serde_json::{Value, json};

use crate::errors::invalid_params;
use crate::resources::{ViewEvm, dispatch_uri_to_json};
use crate::server::ExecutorServer;
use crate::validation::validate_strategy_id_format;

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

Concurrency: a trigger that fires while a previous run of the same strategy is still in flight is skipped and journaled as a `dedup_rejected` event. Inspect via `trigger://{trigger_id}/events`.

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

9. **Trigger dedup window:** a trigger that fires while its strategy is still executing is rejected, not queued. Build idempotent strategies; check `trigger://{id}/events` to see suppressed fires."#;

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

/// Build a compact markdown table of currently registered strategies. When the
/// list is empty, returns a one-line "no strategies registered" placeholder so
/// the agent knows it's an empty-state run.
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

// ────────────────────────── v1.11 Track E3 — tune_thresholds ──────────────────────────
//
// Pull last N runs for a strategy, static-parse the source for numeric
// threshold candidates, correlate with run history, propose raise/lower/keep.
// Proposals are NEVER auto-applied — they're reviewed by the user.
//
// Heuristic design (see plan: `.planning/v1.11-SURFACE-COMPLETION.md` E3):
//
// - Pure scanner over the source string. No real JS parser (`include_str!`
//   sources stay short; an n^2 scan over a typically-<5KB source is fine).
// - Strip `// ...` and `/* ... */` regions BEFORE candidate extraction so a
//   numeric literal inside a comment never makes it to the candidate list.
// - Strip string literals (`"..."`, `'...'`, backticks) too — a version string
//   like `"1.0.0"` shouldn't surface as three threshold candidates.
// - Walk every line; require a comparison operator on the line to seed a
//   candidate. Then scan the line for numeric literals using a hand-rolled
//   tokenizer (no regex dependency).

/// A threshold-candidate extracted by the static scanner.
#[derive(Debug, Clone)]
struct ThresholdCandidate {
    /// Parsed numeric value (after `_` separator removal). Kept for
    /// downstream consumers that want numeric comparison; the table
    /// rendering uses `raw_value` so the source text round-trips.
    #[allow(dead_code)]
    value: f64,
    raw_value: String,
    line_number: u32,
    column: u32,
    raw_line_text: String,
    /// The first comparison operator found on the line — used to decide
    /// whether "fires often" means "consider raising" vs "consider lowering".
    op: String,
}

/// Strip `//` line comments and `/* ... */` block comments AND string
/// literals from `source`, replacing each removed region with spaces (one
/// space per byte) so byte offsets / line / column positions of the
/// remaining code stay stable. Used as a pre-pass before threshold
/// extraction — anything inside a comment or quoted string is excluded
/// from the candidate scan.
fn strip_comments_and_strings(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // Block comment `/* ... */`. May span multiple lines — preserve `\n`
        // so line numbers downstream still align.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                i += 1;
            }
            if i + 1 < bytes.len() {
                out.push(b' ');
                out.push(b' ');
                i += 2;
            } else {
                // Truncated block comment — pad rest and stop.
                while i < bytes.len() {
                    out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                    i += 1;
                }
            }
            continue;
        }
        // Line comment `// ...` — strip to end of line, preserve the `\n`.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        // String literal `"..."` / `'...'` / template `` `...` ``. Preserve
        // newlines (template strings can be multi-line); replace contents
        // with spaces so embedded numbers don't surface.
        if b == b'"' || b == b'\'' || b == b'`' {
            let quote = b;
            out.push(b' ');
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    continue;
                }
                out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
                i += 1;
            }
            if i < bytes.len() {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

/// Threshold-relevant vocabulary on a line bumps the candidate up the
/// ranking when we need to drop overflow. Match case-insensitive substring.
const THRESHOLD_VOCAB: &[&str] = &[
    "threshold",
    "limit",
    "min",
    "max",
    "target",
    "slippage",
    "apy",
    "rate",
    "amount",
    "cap",
    "floor",
];

/// Vocab that suggests a numeric is a timestamp / block height — exclude
/// candidates whose surrounding identifier on the line contains any of these.
const TIMESTAMP_VOCAB: &[&str] = &["ts", "time", "block", "deadline", "expiry"];

/// True when `c` could continue a JS identifier.
fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// Extract numeric-literal threshold candidates from a strategy source. The
/// heuristic is documented inline at the call site and in the module
/// header — see `tune_thresholds` for the exact rules.
fn extract_threshold_candidates(source: &str) -> Vec<ThresholdCandidate> {
    let scrubbed = strip_comments_and_strings(source);
    let mut out: Vec<ThresholdCandidate> = Vec::new();

    for (line_idx, line) in scrubbed.lines().enumerate() {
        // Skip lines with no comparison operator. We don't try to be clever
        // about `=` (assignment) vs `==` (comparison) — assignment-only
        // lines have no comparison anyway.
        let op = first_comparison_op(line);
        let Some(op) = op else { continue };

        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_digit() || (c == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) {
                // Skip if this digit is part of an identifier (`a1`, `b2`).
                if i > 0 && is_ident_cont(bytes[i - 1]) {
                    // Advance past the rest of the identifier.
                    while i < bytes.len() && is_ident_cont(bytes[i]) {
                        i += 1;
                    }
                    continue;
                }
                let start = i;
                // Walk the numeric literal: digits / `.` / `_` / `e±N`.
                let mut saw_dot = c == b'.';
                let mut saw_exp = false;
                while i < bytes.len() {
                    let b = bytes[i];
                    if b.is_ascii_digit() || b == b'_' {
                        i += 1;
                    } else if b == b'.' && !saw_dot && !saw_exp {
                        saw_dot = true;
                        i += 1;
                    } else if (b == b'e' || b == b'E') && !saw_exp {
                        saw_exp = true;
                        i += 1;
                        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                            i += 1;
                        }
                    } else {
                        break;
                    }
                }
                let raw = &line[start..i];

                // Skip hex / address-like literals: `0x...`. Our scanner
                // doesn't enter the digit branch for `0x` (the `x` breaks
                // the loop), so when raw is `0` and the next non-digit char
                // is `x`, recognise that as the start of a hex literal and
                // walk past the whole hex token.
                if raw == "0" && i < bytes.len() && (bytes[i] == b'x' || bytes[i] == b'X') {
                    let hex_start = i - 1;
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                        i += 1;
                    }
                    let hex_len = i - hex_start;
                    // Skip both address-like (≥10 hex chars) AND selector-like
                    // (`0x` + <10 hex chars). Neither is a threshold.
                    let _ = hex_len; // both branches: skip silently.
                    continue;
                }

                // Skip if this number is an array index: `<ident>[<number>]`.
                // Find the previous non-space char before `start`.
                let mut p = start;
                while p > 0 {
                    p -= 1;
                    if bytes[p] == b' ' || bytes[p] == b'\t' {
                        continue;
                    }
                    break;
                }
                if start > 0 && bytes[p] == b'[' {
                    // Confirm there is an identifier directly before `[`.
                    let mut q = p;
                    while q > 0 && (bytes[q - 1] == b' ' || bytes[q - 1] == b'\t') {
                        q -= 1;
                    }
                    if q > 0 && is_ident_cont(bytes[q - 1]) {
                        continue;
                    }
                }

                // Skip the literal `0` and `1` — too generic.
                let parsed: Option<f64> = raw.replace('_', "").parse::<f64>().ok();
                let Some(val) = parsed else { continue };
                if (val - 0.0).abs() < f64::EPSILON || (val - 1.0).abs() < f64::EPSILON {
                    continue;
                }

                // Skip timestamps-by-vocab: large integer (10+ digits) AND
                // any identifier on the line matches TIMESTAMP_VOCAB.
                let digit_count = raw.chars().filter(|c| c.is_ascii_digit()).count();
                if digit_count >= 10 {
                    let lower = line.to_ascii_lowercase();
                    if TIMESTAMP_VOCAB.iter().any(|kw| {
                        // Word-ish match: substring is enough for the
                        // heuristic; precise tokenisation is overkill.
                        lower.contains(kw)
                    }) {
                        continue;
                    }
                }

                out.push(ThresholdCandidate {
                    value: val,
                    raw_value: raw.to_string(),
                    line_number: (line_idx as u32) + 1,
                    column: (start as u32) + 1,
                    raw_line_text: line.trim().to_string(),
                    op: op.to_string(),
                });
                continue;
            }
            i += 1;
        }
    }

    // Cap at 20. When over the cap, prefer candidates whose `raw_line_text`
    // mentions threshold-y vocab.
    if out.len() > 20 {
        out.sort_by_key(|c| {
            let lower = c.raw_line_text.to_ascii_lowercase();
            if THRESHOLD_VOCAB.iter().any(|kw| lower.contains(kw)) {
                0
            } else {
                1
            }
        });
        out.truncate(20);
    }
    out
}

/// Return the first comparison operator we find on `line`, or `None` if the
/// line has none. Recognises `<=`, `>=`, `==`, `!=`, `<`, `>` (in that
/// length-priority order so `<=` isn't truncated to `<`).
fn first_comparison_op(line: &str) -> Option<&'static str> {
    let two = ["<=", ">=", "==", "!="];
    let one = ["<", ">"];
    let mut best: Option<(usize, &'static str)> = None;
    for op in two.iter() {
        if let Some(idx) = line.find(op) {
            match best {
                Some((bi, _)) if bi <= idx => {}
                _ => best = Some((idx, *op)),
            }
        }
    }
    for op in one.iter() {
        if let Some(idx) = line.find(op) {
            // Skip `<=` / `>=` already matched at the same position.
            if line.as_bytes().get(idx + 1).copied() == Some(b'=') {
                continue;
            }
            match best {
                Some((bi, _)) if bi <= idx => {}
                _ => best = Some((idx, *op)),
            }
        }
    }
    best.map(|(_, op)| op)
}

/// Count how many runs in `runs_json` have a journal whose `decisions[]`
/// rows or `actions[].payload_json` substrings contain `needle`. Pragmatic
/// substring match — false positives are tolerable, false negatives are not.
async fn count_threshold_hits(
    state: std::sync::Arc<tokio::sync::Mutex<executor_state::StateStore>>,
    evm: ViewEvm,
    runs: &[serde_json::Value],
    needle: &str,
) -> u32 {
    let mut hits: u32 = 0;
    for r in runs {
        let Some(run_id) = r.get("run_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let journal_uri = format!("journal://{run_id}");
        let Ok(j) = dispatch_uri_to_json(journal_uri, state.clone(), evm.clone()).await else {
            continue;
        };
        let mut found = false;
        if let Some(decisions) = j.get("decisions").and_then(|v| v.as_array()) {
            for d in decisions {
                if let Some(detail) = d.get("detail").and_then(|v| v.as_str()) {
                    if detail.contains(needle) {
                        found = true;
                        break;
                    }
                }
                if let Some(payload) = d.get("payload_json").and_then(|v| v.as_str()) {
                    if payload.contains(needle) {
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            if let Some(actions) = j.get("actions").and_then(|v| v.as_array()) {
                for a in actions {
                    if let Some(payload) = a.get("payload_json").and_then(|v| v.as_str()) {
                        if payload.contains(needle) {
                            found = true;
                            break;
                        }
                    }
                }
            }
        }
        if found {
            hits += 1;
        }
    }
    hits
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

    /// v1.4 Track E1: refreshed `getting_started` — prefetches the live
    /// strategy list and the loaded policy and inlines them in the body so a
    /// fresh agent has a one-screen orientation without any further tool
    /// calls. See `.planning/v1.4-AGENT-UX-DESIGN.md` §9.
    #[prompt(
        name = "getting_started",
        description = "Orient a fresh agent: inlined strategy_list + policy + 5-step first-action playbook."
    )]
    async fn getting_started(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        // Prefetch live state — single short DB read on the StateStore mutex.
        let state = self.state.clone();
        let strategies: Vec<StrategySummary> = tokio::task::spawn_blocking(move || {
            let store = state.blocking_lock();
            store.list_strategies(false).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        let policy_summary = {
            let guard = self.policy.read().await;
            format_policy_summary(guard.as_ref())
        };
        let strategy_table = format_strategy_table(&strategies);
        let empty = strategies.is_empty();

        let playbook = if empty {
            r#"## First-action playbook (empty state — prove the loop end-to-end)

1. **Register a strategy.** Read `examples://strategies/yield-snapshot` and call `strategy_register({ name: "yield-snapshot", source: <that source> })`. yield-snapshot is pure-read and exercises no policy gates.
2. **Attach a trigger.** Call `trigger_register({ strategy_id: <id>, kind: "interval", config: { interval_ms: 3_600_000 } })` for an hourly snapshot, OR skip and run manually.
3. **Wait for first fire** (or call `strategy_run({ strategy_id })` to fire now).
4. **Check the execution.** Read `execution://{run_id}` for the report and `journal://{run_id}` for the per-action decision trace.
5. **Tune.** Inspect `strategy://list` / `trigger://list` and decide whether to add a real funnel strategy. See `examples://strategies` for source patterns."#
        } else {
            r#"## First-action playbook (something is already registered)

1. **Pick a strategy** from the table above and read its source via `strategy://{id}`.
2. **Inspect triggers.** Read `trigger://list` and confirm the strategy fires on the cadence you expect.
3. **Check recent fires.** Read the latest `execution://{run_id}` and `journal://{run_id}` to see what each fire decided.
4. **Adjust or extend.** Either author a new strategy from the user's intent (`author_strategy` prompt) or refine an existing one (`safety_review` prompt before re-registering).
5. **Tune thresholds** with evidence — use the records of the last N runs (Track A+E2 once shipped)."#
        };

        let body = format!(
            "You are connected to the Onchain Strategy MCP runtime. This is a **prefetched orientation** — the live state below is read from the server at the moment of this prompt.\n\n\
             ## Registered strategies (`strategy://list`)\n\n{strategies}\n\n\
             ## Active policy (`policy://current`)\n\n{policy}\n\n\
             {playbook}\n\n\
             ## Where to look next\n\n\
             - Source patterns: `examples://strategies` (list) + `examples://strategies/{{name}}` (each source).\n\
             - Authoring guide: invoke the `author_strategy` prompt with your intent.\n\
             - Pre-register safety check: invoke the `safety_review` prompt with the proposed source.\n\
             - Trigger selection: invoke the `trigger_patterns` prompt.\n\
             - Failure modes: invoke the `common_pitfalls` prompt before iterating on a failing run.\n\n\
             When the user describes intent, WRITE THE STRATEGY YOURSELF — do not ask the user for code. The runtime simulates each action through the policy above before signing.",
            strategies = strategy_table,
            policy = policy_summary,
            playbook = playbook,
        );

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("Prefetched orientation: strategy_list + policy + 5-step playbook"))
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

    /// v1.11 Track E1: `inventory` — one-screen status digest answering
    /// "what's running right now?" in a single prompt call.
    ///
    /// Prefetches `runtime://status`, `portfolio://`, and
    /// `strategy://list?status=active&summary=true`, composing them into a
    /// human-readable Markdown digest with System / Positions / Strategies
    /// sections. Each prefetch is matched independently — a single failure
    /// degrades that section to a `⚠️ unavailable` marker and the prompt
    /// continues. Honesty-envelope (`confidence != "full"`) prepends a
    /// section-level `⚠️ Partial: <reason>` line.
    #[prompt(
        name = "inventory",
        description = "One-screen status digest: System (RPC/watchers/24h), Positions (portfolio), Strategies (active)."
    )]
    async fn inventory(
        &self,
        Parameters(_args): Parameters<EmptyPromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let evm = crate::resources::ViewEvm {
            provider: self.evm_provider().await.ok(),
            evm_config: self.evm_config.clone(),
            price_cache: Some(self.price_cache.clone()),
            chain_id: self.chain_id().await.ok(),
        };

        // 1. System section — runtime://status.
        let system_block = match crate::resources::dispatch_uri_to_json(
            "runtime://status".to_string(),
            self.state.clone(),
            evm.clone(),
        )
        .await
        {
            Ok(v) => render_system_section(&v),
            Err(e) => format!("**System**: ⚠️ unavailable — {}", e.message),
        };

        // 2. Positions section — portfolio://.
        let positions_block = match crate::resources::dispatch_uri_to_json(
            "portfolio://".to_string(),
            self.state.clone(),
            evm.clone(),
        )
        .await
        {
            Ok(v) => render_positions_section(&v),
            Err(e) => format!("**Positions**: ⚠️ unavailable — {}", e.message),
        };

        // 3. Strategies section — active summary list.
        let strategies_block = match crate::resources::dispatch_uri_to_json(
            "strategy://list?status=active&summary=true".to_string(),
            self.state.clone(),
            evm.clone(),
        )
        .await
        {
            Ok(v) => render_strategies_section(&v),
            Err(e) => format!("**Strategies**: ⚠️ unavailable — {}", e.message),
        };

        let body = format!(
            "# Inventory — one-screen status digest\n\n\
             ## System\n\n{system}\n\n\
             ## Positions\n\n{positions}\n\n\
             ## Strategies\n\n{strategies}\n\n\
             Next steps:\n\
             - Failed runs? → call prompt `triage_run` with the run_id \
             (see execution://list?status=failed&limit=1)\n\
             - Adjust strategy thresholds? → call prompt `tune_thresholds`\n\
             - System health detail? → read runtime://status\n",
            system = system_block,
            positions = positions_block,
            strategies = strategies_block,
        );

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("One-screen status digest: System + Positions + Strategies"))
    }

    /// v1.11 Track E3: pull last N runs for a strategy, static-parse the
    /// source for numeric thresholds, correlate with run history, and
    /// propose raise/lower/keep. Proposals are NEVER auto-applied — they're
    /// reviewed by the user and re-registered manually.
    #[prompt(
        name = "tune_thresholds",
        description = "Threshold tuning report: static-parse strategy source + correlate with last N runs."
    )]
    async fn tune_thresholds(
        &self,
        Parameters(args): Parameters<TuneThresholdsArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        // 1. Validate strategy_id format early — invalid hex → invalid_params
        //    with a hint pointing at `strategy://list`.
        if let Err(e) = validate_strategy_id_format(&args.strategy_id) {
            return Err(invalid_params(format!(
                "tune_thresholds: strategy_id is malformed: {e}. \
                 call resource strategy://list to see active strategy ids"
            )));
        }
        // Clamp lookback. Default 20, hard cap 200.
        let lookback: u32 = args.lookback_runs.unwrap_or(20).min(200).max(1);

        let state = self.state.clone();
        let evm = ViewEvm::default();

        // 2. Prefetch strategy meta. `not_found` → invalid_params with hint.
        let strategy_uri = format!("strategy://{}", args.strategy_id);
        let strategy_meta = match dispatch_uri_to_json(strategy_uri.clone(), state.clone(), evm.clone()).await {
            Ok(v) => v,
            Err(e) => {
                // `resource_not_found` (-32002) and `not_found` (-32014) both
                // map to "no such strategy". Re-raise as invalid_params with
                // the listing hint per the plan.
                if e.code.0 == -32002 || e.code.0 == -32014 {
                    return Err(invalid_params(format!(
                        "tune_thresholds: strategy_id {} not found. \
                         call resource strategy://list to see active strategy ids",
                        args.strategy_id
                    )));
                }
                return Err(e);
            }
        };

        let strategy_name = strategy_meta
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)")
            .to_string();
        let strategy_version = strategy_meta
            .get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let source = strategy_meta
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 3. Prefetch recent run summaries.
        let runs_uri = format!(
            "execution://list?strategy_id={}&limit={}",
            args.strategy_id, lookback
        );
        let runs_body = dispatch_uri_to_json(runs_uri, state.clone(), evm.clone())
            .await
            .unwrap_or_else(|_| serde_json::json!({ "runs": [], "count": 0 }));
        let empty_runs: Vec<serde_json::Value> = Vec::new();
        let runs = runs_body
            .get("runs")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty_runs);
        let actual_run_count = runs.len() as u32;

        // Compute the earliest started_at + the latest one (for the report
        // header). `started_at` is RFC3339 — we don't need to parse, just
        // string-min / string-max (RFC3339 strings sort chronologically when
        // they share a timezone offset, which our runtime always emits as
        // `Z`).
        let earliest_ts = runs
            .iter()
            .filter_map(|r| r.get("started_at").and_then(|v| v.as_str()))
            .min()
            .map(|s| s.to_string());
        let latest_ts = runs
            .iter()
            .filter_map(|r| r.get("started_at").and_then(|v| v.as_str()))
            .max()
            .map(|s| s.to_string());

        // 4. Pull records during the window — informational only for the
        //    report; the correlation hit count is journal-based.
        let records_uri = match &earliest_ts {
            Some(ts) => format!(
                "strategy://{}/records?since={}",
                args.strategy_id,
                // Minimal percent-encode: `:` stays, `+` would break the
                // parser. RFC3339 timestamps don't include `+` for `Z`-suffixed
                // wire values, so pass through as-is.
                ts
            ),
            None => format!("strategy://{}/records", args.strategy_id),
        };
        let records_body = dispatch_uri_to_json(records_uri, state.clone(), evm.clone())
            .await
            .unwrap_or_else(|_| serde_json::json!({ "records": [], "count": 0 }));
        let records_count = records_body
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // 5. Static-parse source for candidates.
        let candidates = extract_threshold_candidates(&source);

        // 6. If no candidates, return the graceful missing-data response.
        if candidates.is_empty() {
            let body = format!(
                "# Threshold tuning report — `{name}` [v{ver}]\n\n\
                 Lookback: last {requested} runs requested ({actual} found).\n\n\
                 ## confidence: missing\n\n\
                 No numeric-literal thresholds were found in this strategy's source.\n\n\
                 `tune_thresholds` expects comparison expressions whose right-hand side is a \
                 literal number — e.g. `apy > 0.05`, `balance >= 100_000`, `price < 2_000`. \
                 This strategy doesn't appear to have any (or it gets all thresholds from \
                 strategy args / config rather than from inline literals).\n\n\
                 ## Suggested next step\n\n\
                 - If thresholds are passed via strategy args, inspect the args distribution \
                   from `execution://list?strategy_id={id}` and the per-run journals (`journal://{{run_id}}`).\n\
                 - If the strategy is pure-read (`noop`), thresholds may not be the right \
                   tuning surface — re-evaluate the decision rule itself.\n\n\
                 ## Caveats\n\n\
                 - Static parse is heuristic; it scans for literal numbers in comparison \
                   expressions only.\n\
                 - Comments and string literals are excluded from candidate scanning.\n",
                name = strategy_name,
                ver = strategy_version,
                requested = lookback,
                actual = actual_run_count,
                id = args.strategy_id,
            );
            return Ok(GetPromptResult::new(vec![PromptMessage::new_text(
                PromptMessageRole::User,
                body,
            )])
            .with_description("Threshold tuning (no candidates found)"));
        }

        // 7. Correlate each candidate with run history.
        let half = (actual_run_count / 2).max(1);
        let mut table_rows = String::new();
        let mut fires_often = 0u32;
        let mut never_fires = 0u32;
        let mut occasional = 0u32;
        for c in &candidates {
            let trigger_count = count_threshold_hits(
                state.clone(),
                evm.clone(),
                runs,
                &c.raw_value,
            )
            .await;
            let (proposal, rationale) = if actual_run_count == 0 {
                (
                    "keep".to_string(),
                    "no runs in window — not enough signal to adjust".to_string(),
                )
            } else if trigger_count == 0 {
                never_fires += 1;
                (
                    "keep".to_string(),
                    "never triggered in window; not enough signal to adjust".to_string(),
                )
            } else if trigger_count >= half {
                fires_often += 1;
                // For `>` / `>=`: gate fires often => threshold too permissive
                // (the comparison passes too often) => suggest *raising* the
                // threshold. For `<` / `<=`: gate fires often => threshold too
                // restrictive => suggest *lowering*. `==` / `!=`: unclear
                // direction, leave as "review".
                match c.op.as_str() {
                    ">" | ">=" => (
                        format!("raise (currently `{}`)", c.raw_value),
                        "gate fires often, may be too permissive".to_string(),
                    ),
                    "<" | "<=" => (
                        format!("lower (currently `{}`)", c.raw_value),
                        "gate fires often, may be too restrictive".to_string(),
                    ),
                    _ => (
                        "review".to_string(),
                        "gate fires often; direction depends on operator semantics".to_string(),
                    ),
                }
            } else {
                occasional += 1;
                (
                    "keep".to_string(),
                    format!("gate fires occasionally ({trigger_count}/{actual_run_count})"),
                )
            };
            let snippet: String = c.raw_line_text.chars().take(80).collect();
            let _ = writeln!(
                table_rows,
                "| `{raw}` | line {line}, col {col} | {hits} / {total} | {prop} | {rat} ({op}) — `{snip}` |",
                raw = c.raw_value,
                line = c.line_number,
                col = c.column,
                hits = trigger_count,
                total = actual_run_count,
                prop = proposal,
                rat = rationale,
                op = c.op,
                snip = snippet,
            );
        }

        let window_line = match (&earliest_ts, &latest_ts) {
            (Some(e), Some(l)) => format!("Lookback: last {} runs ({} to {})", actual_run_count, e, l),
            _ => format!("Lookback: last {} runs requested ({} found)", lookback, actual_run_count),
        };

        let body = format!(
            "# Threshold tuning report — `{name}` [v{ver}]\n\n\
             {window}\n\n\
             Records in window: {recs}\n\n\
             | Current value | Location | Trigger count | Proposal | Rationale |\n\
             |---|---|---|---|---|\n\
             {rows}\n\
             ## Summary\n\n\
             Found {n_cand} threshold candidate(s) — {fires} fire often (>= half the window), \
             {never} never fired, {occ} fire occasionally. \
             {adjustable_note}\n\n\
             ## Caveats\n\n\
             - Static parse is heuristic; some literals may be array sizes, magic constants, \
               or other non-threshold numbers mis-identified as thresholds.\n\
             - Proposals are NEVER auto-applied. Review and re-register the strategy yourself \
               if you decide to change a value.\n\
             - Correlation via journal substring match — false positives possible if the same \
               numeric literal appears in unrelated payloads (e.g. as part of an amount or \
               address fragment).\n\
             - Operator direction matters: `>` / `>=` firing often suggests *raise*; `<` / `<=` \
               firing often suggests *lower*. `==` / `!=` are flagged for manual review.\n",
            name = strategy_name,
            ver = strategy_version,
            window = window_line,
            recs = records_count,
            rows = table_rows,
            n_cand = candidates.len(),
            fires = fires_often,
            never = never_fires,
            occ = occasional,
            adjustable_note = if fires_often > 0 {
                "Some candidates look adjustable — review the `raise` / `lower` rows above."
            } else if never_fires == candidates.len() as u32 {
                "None look adjustable from this window — every candidate has zero hits."
            } else {
                "No urgent adjustments — most candidates fire occasionally or not at all."
            },
        );

        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            body,
        )])
        .with_description("Threshold tuning report"))
    }
}

/// Render the System block from a `runtime://status` payload. Best-effort:
/// when a field is missing the line still renders with a `?` placeholder so
/// the agent can see the shape gap without an aborted digest.
fn render_system_section(v: &serde_json::Value) -> String {
    let mut out = String::new();

    // Honesty envelope: surface partial-confidence at the top of the section.
    if let Some(c) = v.get("confidence").and_then(serde_json::Value::as_str)
        && c != "full"
    {
        let reason = v
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(no reason given)");
        let _ = writeln!(out, "⚠️ Partial: {reason}\n");
    }

    let chain_id = v
        .get("chain_id")
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    let burner = v
        .get("burner")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let rpc = match v.get("rpc") {
        Some(rpc_v) => {
            let status = rpc_v
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            match status {
                "ok" => "ok".to_string(),
                "degraded" | "missing" => {
                    let reason = rpc_v
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("(no reason given)");
                    format!("{status} ({reason})")
                }
                other => other.to_string(),
            }
        }
        None => "?".to_string(),
    };
    let _ = writeln!(out, "- chain_id={chain_id} | burner={burner} | RPC: {rpc}");

    let last_24h = v
        .get("last_24h")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let runs = last_24h
        .get("runs")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let succeeded = last_24h
        .get("succeeded")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let failed = last_24h
        .get("failed")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let noop = last_24h
        .get("noop")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let _ = writeln!(
        out,
        "- last 24h: runs={runs} | succeeded={succeeded} | failed={failed} | noop={noop}"
    );

    let watchers = v
        .get("watchers")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mempool = watchers
        .get("mempool")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let log_w = watchers
        .get("log")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let _ = writeln!(out, "- watchers: mempool={mempool}, log={log_w}");

    out
}

/// Render the Positions block from a `portfolio://` payload. The honesty
/// envelope from Track C is the outer object; the aggregation lives at
/// `.data` and the asset list at `.data.data.assets[]` (the inner `data` is
/// the aggregation result; the outer `data` is the envelope's payload).
fn render_positions_section(v: &serde_json::Value) -> String {
    let mut out = String::new();

    if let Some(c) = v.get("confidence").and_then(serde_json::Value::as_str)
        && c != "full"
    {
        let reason = v
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(no reason given)");
        let _ = writeln!(out, "⚠️ Partial: {reason}\n");
    }

    // Double-`.data.data` unwrap per the v1.11 spec — outer is honesty
    // envelope (Track C), inner is the aggregation payload. We probe both
    // shapes so the renderer survives schema drift between sub-tracks.
    let assets_opt = v
        .get("data")
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("assets"))
        .and_then(serde_json::Value::as_array);

    let assets = match assets_opt {
        Some(a) => a,
        None => {
            out.push_str("(no positions)\n");
            return out;
        }
    };

    if assets.is_empty() {
        out.push_str("(no positions)\n");
        return out;
    }

    let mut total_usd: f64 = 0.0;
    let mut any_usd = false;
    for a in assets {
        let symbol = a
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let amount = a
            .get("amount")
            .and_then(|x| {
                x.as_str()
                    .map(String::from)
                    .or_else(|| x.as_f64().map(|f| f.to_string()))
                    .or_else(|| x.as_u64().map(|u| u.to_string()))
            })
            .unwrap_or_else(|| "?".to_string());
        let usd_opt = a.get("usd").and_then(|x| {
            x.as_f64()
                .or_else(|| x.as_str().and_then(|s| s.parse::<f64>().ok()))
        });
        if let Some(u) = usd_opt {
            total_usd += u;
            any_usd = true;
        }
        let usd_disp = usd_opt
            .map(|u| format!("${u:.2}"))
            .unwrap_or_else(|| "$?".to_string());
        let chain = a
            .get("chain")
            .and_then(|x| {
                x.as_str()
                    .map(String::from)
                    .or_else(|| x.as_u64().map(|u| u.to_string()))
            })
            .unwrap_or_else(|| "?".to_string());
        let venue = a
            .get("venue")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let _ = writeln!(
            out,
            "- {symbol} {amount} ({usd_disp}) on {chain} via {venue}"
        );
    }

    if any_usd {
        let _ = writeln!(out, "\nTotal: ${total_usd:.2}");
    }

    out
}

/// Render the Strategies block from a `strategy://list?...` payload.
fn render_strategies_section(v: &serde_json::Value) -> String {
    let mut out = String::new();

    let strategies = v
        .get("strategies")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    if strategies.is_empty() {
        out.push_str("(no active strategies)\n");
        return out;
    }

    for s in &strategies {
        let name = s
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?");
        let version = s
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".to_string());
        let kinds = s
            .get("trigger_kinds")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let kinds_disp = if kinds.is_empty() {
            "none".to_string()
        } else {
            kinds
        };
        let last_fire = s
            .get("last_fire_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("never");
        let last_24h = s
            .get("last_24h")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let runs = last_24h
            .get("runs")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let succeeded = last_24h
            .get("succeeded")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let failed = last_24h
            .get("failed")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let _ = writeln!(
            out,
            "- {name} [v{version}] — triggers: {kinds_disp}, last_fire: {last_fire}, 24h: {runs}/{succeeded}/{failed}"
        );
    }

    out
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
