//! v1.4 Track A2 — Records DSL evaluator.
//!
//! Strategies ship with a declarative `records` schema (see
//! `executor_core::schema::strategy::RecordSpec`). At action-confirm time the
//! runtime asks this module two questions:
//!
//! 1. **Does the spec's `on` clause match this confirmed action?**
//!    ([`evaluate_match`]).
//! 2. **What does the spec's `capture` map produce against this action's
//!    context?** ([`evaluate_capture`]).
//!
//! Both questions are total: any error inside an individual capture expression
//! is non-fatal and surfaces as a structured warning ([`CaptureWarning`])
//! attached to the offending field, so the surrounding [`record_action`] path
//! never breaks because records-capture failed.
//!
//! The DSL is intentionally narrow in v1:
//!
//! Match kinds (case-insensitive `kind` discriminator):
//! - `contractCall { target?: addr, selector?: name|hex }`
//! - `erc20Approve { token?: addr, spender?: addr }`
//! - `log { address?: addr, topics?: [topic|null, ...] }`
//!
//! Capture accessors (string expressions):
//! - `args[N]`, `args.name` (positional / named — named requires ABI lookup)
//! - `logs.<Event>[<index|"self">].<field>` (decoded log accessor; v1 supports
//!   ERC20 Transfer with `[self]` / `[0]` and `.value`/`.from`/`.to`)
//! - `tx.hash | tx.block | tx.ts | tx.gas_used`
//! - `view.aaveLiquidityIndex(asset)` (Aave V3 Pool, Base mainnet)
//!
//! Anything outside this set is a [`CaptureWarning::UnsupportedExpr`] — we
//! never silently extend.

use alloy_primitives::{Address, B256, U256, keccak256};
use executor_core::schema::action::Action;
use executor_core::schema::strategy::RecordSpec;
use serde_json::{Map, Value};
use std::str::FromStr;

/// Context for a single confirmed action. The capture hook in
/// `executor-mcp::tools::record_action` builds one of these per action and
/// hands it to [`evaluate_spec`].
#[derive(Debug, Clone)]
pub struct ActionContext<'a> {
    /// The strategy-emitted action (decoded; same shape as journal_actions).
    pub action: &'a Action,
    /// Tx hash from `execution_actions` (None for pre-broadcast / noop).
    pub tx_hash: Option<String>,
    /// Block number; v1 leaves this `None` (the local managed-execution path
    /// does not currently surface a block on the receipt). The capture
    /// expression `tx.block` returns null when this is `None`.
    pub block: Option<u64>,
    /// `started_at` / `recorded_at` RFC3339 timestamp.
    pub ts: Option<String>,
    /// Gas used (decimal-string wei).
    pub gas_used: Option<String>,
    /// The burner address that signed this action (used by `logs.<E>[self]`).
    pub burner: Option<String>,
    /// Decoded receipt logs (RPC-fetched, optional). For v1 we accept a
    /// JSON-shaped vec mirroring `eth_getTransactionReceipt.logs[]`. When
    /// `None`, log accessors return a warning.
    pub logs: Option<&'a [Value]>,
}

/// Output of [`evaluate_spec`]: the evaluated capture map (one field per
/// `capture` entry) plus any non-fatal field-level warnings.
#[derive(Debug, Clone, Default)]
pub struct CaptureOutput {
    pub fields: Map<String, Value>,
    pub warnings: Vec<CaptureWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureWarning {
    pub field: String,
    pub kind: WarningKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// Expression string didn't match any DSL grammar rule.
    UnsupportedExpr,
    /// DSL accessor failed at runtime (e.g. arg index out of bounds, log
    /// shape missing).
    AccessorFailure,
    /// `view.<helper>(...)` helper isn't implemented yet.
    HelperUnavailable,
    /// Expression resolved to a malformed JSON value.
    BadValue,
}

/// Top-level entry: decide if `spec` matches the action, and if so, evaluate
/// `capture`. Returns `None` when the match clause filters out this action.
///
/// Capture-evaluation errors are NEVER fatal — they surface as
/// `CaptureOutput.warnings` while the map carries best-effort fields.
pub fn evaluate_spec(spec: &RecordSpec, ctx: &ActionContext<'_>) -> Option<CaptureOutput> {
    if !evaluate_match(&spec.on, ctx) {
        return None;
    }
    Some(evaluate_capture(&spec.capture, ctx))
}

/// True when `on` matches `ctx`. Unknown / malformed clauses are NOT a match.
pub fn evaluate_match(on: &Value, ctx: &ActionContext<'_>) -> bool {
    let Some(obj) = on.as_object() else {
        return false;
    };
    let Some(kind) = obj.get("kind").and_then(Value::as_str) else {
        return false;
    };
    match kind {
        "contractCall" | "contract_call" => match_contract_call(obj, ctx.action),
        "erc20Approve" | "erc20_approve" => match_erc20_approve(obj, ctx.action),
        "log" => match_log(obj, ctx.logs),
        _ => false,
    }
}

fn match_contract_call(filter: &Map<String, Value>, action: &Action) -> bool {
    let Action::ContractCall(cc) = action else {
        return false;
    };
    if let Some(target) = filter.get("target").and_then(Value::as_str)
        && !addr_eq(target, &cc.address)
    {
        return false;
    }
    if let Some(sel) = filter.get("selector").and_then(Value::as_str)
        && !selector_matches(sel, &cc.abi, &cc.function)
    {
        return false;
    }
    true
}

fn match_erc20_approve(filter: &Map<String, Value>, action: &Action) -> bool {
    let Action::Erc20Approve(a) = action else {
        return false;
    };
    if let Some(tok) = filter.get("token").and_then(Value::as_str)
        && !addr_eq(tok, &a.token)
    {
        return false;
    }
    if let Some(sp) = filter.get("spender").and_then(Value::as_str)
        && !addr_eq(sp, &a.spender)
    {
        return false;
    }
    true
}

fn match_log(filter: &Map<String, Value>, logs: Option<&[Value]>) -> bool {
    let Some(logs) = logs else { return false };
    let want_addr = filter.get("address").and_then(Value::as_str);
    let want_topics = filter.get("topics").and_then(Value::as_array);
    logs.iter().any(|log| {
        if let Some(addr) = want_addr {
            let got = log.get("address").and_then(Value::as_str).unwrap_or("");
            if !addr_eq(addr, got) {
                return false;
            }
        }
        if let Some(topics) = want_topics {
            let log_topics = match log.get("topics").and_then(Value::as_array) {
                Some(t) => t,
                None => return false,
            };
            for (i, want_topic) in topics.iter().enumerate() {
                if want_topic.is_null() {
                    continue; // wildcard slot
                }
                let want = match want_topic.as_str() {
                    Some(s) => s,
                    None => return false,
                };
                let got = log_topics.get(i).and_then(Value::as_str).unwrap_or("");
                if !hex32_eq(want, got) {
                    return false;
                }
            }
        }
        true
    })
}

/// Evaluate every entry in `capture` (must be an object). Non-object inputs
/// produce an empty fields map + a single top-level warning so the caller can
/// surface the structural problem.
pub fn evaluate_capture(capture: &Value, ctx: &ActionContext<'_>) -> CaptureOutput {
    let mut out = CaptureOutput::default();
    let Some(obj) = capture.as_object() else {
        out.warnings.push(CaptureWarning {
            field: "<root>".to_string(),
            kind: WarningKind::UnsupportedExpr,
            detail: format!("capture must be a JSON object, got {}", short_type(capture)),
        });
        return out;
    };
    for (k, expr) in obj.iter() {
        match expr.as_str() {
            Some(s) => match evaluate_expr(s, ctx) {
                Ok(v) => {
                    out.fields.insert(k.clone(), v);
                }
                Err(w) => out.warnings.push(CaptureWarning {
                    field: k.clone(),
                    kind: w.kind,
                    detail: w.detail,
                }),
            },
            None => out.warnings.push(CaptureWarning {
                field: k.clone(),
                kind: WarningKind::UnsupportedExpr,
                detail: format!(
                    "capture field `{k}` must be a string expression, got {}",
                    short_type(expr)
                ),
            }),
        }
    }
    out
}

/// Public for tests — evaluate one expression string.
pub fn evaluate_expr(expr: &str, ctx: &ActionContext<'_>) -> Result<Value, ExprError> {
    let expr = expr.trim();

    // tx.<field>
    if let Some(rest) = expr.strip_prefix("tx.") {
        return eval_tx(rest, ctx);
    }
    // args[N] / args.name
    if let Some(rest) = expr.strip_prefix("args") {
        return eval_args(rest, ctx);
    }
    // logs.<Event>[<filter>].<field>
    if let Some(rest) = expr.strip_prefix("logs.") {
        return eval_logs(rest, ctx);
    }
    // view.<helper>(...)
    if let Some(rest) = expr.strip_prefix("view.") {
        return eval_view(rest, ctx);
    }
    Err(ExprError {
        kind: WarningKind::UnsupportedExpr,
        detail: format!(
            "unknown expression `{expr}`; supported prefixes: tx., args, logs., view."
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    pub kind: WarningKind,
    pub detail: String,
}

fn eval_tx(rest: &str, ctx: &ActionContext<'_>) -> Result<Value, ExprError> {
    match rest {
        "hash" => Ok(ctx
            .tx_hash
            .as_ref()
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null)),
        "block" => Ok(ctx
            .block
            .map(|b| Value::Number(serde_json::Number::from(b)))
            .unwrap_or(Value::Null)),
        "ts" => Ok(ctx
            .ts
            .as_ref()
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null)),
        "gas_used" => Ok(ctx
            .gas_used
            .as_ref()
            .map(|s| Value::String(s.clone()))
            .unwrap_or(Value::Null)),
        other => Err(ExprError {
            kind: WarningKind::UnsupportedExpr,
            detail: format!(
                "unknown tx accessor `tx.{other}`; supported: tx.hash, tx.block, tx.ts, tx.gas_used"
            ),
        }),
    }
}

fn eval_args(rest: &str, ctx: &ActionContext<'_>) -> Result<Value, ExprError> {
    // Build the args list + optional named ABI inputs for the current action.
    let (args, named): (Vec<&Value>, Option<Vec<String>>) = match ctx.action {
        Action::ContractCall(cc) => {
            let names = abi_input_names(&cc.abi, &cc.function);
            (cc.args.iter().collect(), names)
        }
        Action::Erc20Approve(a) => {
            // Synthesize argv = [spender, amount] so capture specs that target
            // erc20Approve actions still get positional access without
            // requiring callers to author with a contract_call shape.
            let v = vec![Value::String(a.spender.clone()), Value::String(a.amount.clone())];
            return eval_args_from_synth(rest, v, Some(vec!["spender".into(), "amount".into()]));
        }
        Action::Erc20Transfer(a) => {
            let v = vec![Value::String(a.to.clone()), Value::String(a.amount.clone())];
            return eval_args_from_synth(rest, v, Some(vec!["to".into(), "amount".into()]));
        }
        _ => {
            return Err(ExprError {
                kind: WarningKind::AccessorFailure,
                detail: "args accessor not supported for this action kind".to_string(),
            });
        }
    };

    eval_args_from_refs(rest, &args, named.as_deref())
}

fn eval_args_from_refs(
    rest: &str,
    args: &[&Value],
    named: Option<&[String]>,
) -> Result<Value, ExprError> {
    if let Some(stripped) = rest.strip_prefix('[') {
        let idx_str = stripped.strip_suffix(']').ok_or_else(|| ExprError {
            kind: WarningKind::UnsupportedExpr,
            detail: format!("malformed args index: `args{rest}` (missing `]`)"),
        })?;
        let idx: usize = idx_str.parse().map_err(|_| ExprError {
            kind: WarningKind::UnsupportedExpr,
            detail: format!("args index must be non-negative integer, got `{idx_str}`"),
        })?;
        return args
            .get(idx)
            .map(|v| (*v).clone())
            .ok_or_else(|| ExprError {
                kind: WarningKind::AccessorFailure,
                detail: format!("args[{idx}] out of bounds (len {})", args.len()),
            });
    }
    if let Some(name) = rest.strip_prefix('.') {
        let names = named.ok_or_else(|| ExprError {
            kind: WarningKind::AccessorFailure,
            detail: format!(
                "args.{name} requires ABI input names but the action's ABI did not parse"
            ),
        })?;
        let pos = names.iter().position(|n| n == name).ok_or_else(|| ExprError {
            kind: WarningKind::AccessorFailure,
            detail: format!(
                "args.{name} not found in ABI input names: {:?}",
                names
            ),
        })?;
        return args
            .get(pos)
            .map(|v| (*v).clone())
            .ok_or_else(|| ExprError {
                kind: WarningKind::AccessorFailure,
                detail: format!("args.{name} resolved to index {pos} but only {} args present", args.len()),
            });
    }
    Err(ExprError {
        kind: WarningKind::UnsupportedExpr,
        detail: format!("expected `args[N]` or `args.name`, got `args{rest}`"),
    })
}

fn eval_args_from_synth(
    rest: &str,
    owned: Vec<Value>,
    named: Option<Vec<String>>,
) -> Result<Value, ExprError> {
    let refs: Vec<&Value> = owned.iter().collect();
    eval_args_from_refs(rest, &refs, named.as_deref())
}

/// Best-effort ABI input-name extraction. Returns `None` if the ABI string
/// doesn't parse OR no function with the given name exists — the caller falls
/// back to positional access (or errors out for named access).
fn abi_input_names(abi_json: &str, function: &str) -> Option<Vec<String>> {
    let v: Value = serde_json::from_str(abi_json).ok()?;
    let arr = v.as_array()?;
    for entry in arr {
        let is_function = entry
            .get("type")
            .and_then(Value::as_str)
            .map(|t| t == "function")
            .unwrap_or(false);
        let name = entry.get("name").and_then(Value::as_str)?;
        if is_function && name == function {
            let inputs = entry.get("inputs").and_then(Value::as_array)?;
            let names: Vec<String> = inputs
                .iter()
                .map(|i| {
                    i.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .collect();
            return Some(names);
        }
    }
    None
}

/// ERC20 Transfer topic0 = keccak256("Transfer(address,address,uint256)").
const ERC20_TRANSFER_TOPIC0: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

fn eval_logs(rest: &str, ctx: &ActionContext<'_>) -> Result<Value, ExprError> {
    // Parse `<Event>[<filter>].<field>`.
    let (event, after_event) = match rest.find('[') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => {
            return Err(ExprError {
                kind: WarningKind::UnsupportedExpr,
                detail: format!("expected `logs.<Event>[<filter>].<field>`, got `logs.{rest}`"),
            });
        }
    };
    let stripped = after_event
        .strip_prefix('[')
        .ok_or_else(|| ExprError {
            kind: WarningKind::UnsupportedExpr,
            detail: format!("malformed log accessor `logs.{rest}`"),
        })?;
    let close = stripped.find(']').ok_or_else(|| ExprError {
        kind: WarningKind::UnsupportedExpr,
        detail: format!("malformed log accessor `logs.{rest}` (missing `]`)"),
    })?;
    let filter = &stripped[..close];
    let after = &stripped[close + 1..];
    let field = after.strip_prefix('.').ok_or_else(|| ExprError {
        kind: WarningKind::UnsupportedExpr,
        detail: format!("missing `.field` in log accessor `logs.{rest}`"),
    })?;

    let logs = ctx.logs.ok_or_else(|| ExprError {
        kind: WarningKind::AccessorFailure,
        detail: format!("logs.{event}[{filter}].{field} requires receipt logs, none provided"),
    })?;

    // v1: only Transfer is decoded.
    if event != "Transfer" {
        return Err(ExprError {
            kind: WarningKind::HelperUnavailable,
            detail: format!(
                "log event `{event}` is not decoded in v1; only `Transfer` is supported. Drop this expression or wait for v1.5."
            ),
        });
    }

    // Find candidate Transfer logs (topic0 matches).
    let candidates: Vec<&Value> = logs
        .iter()
        .filter(|l| {
            l.get("topics")
                .and_then(Value::as_array)
                .and_then(|t| t.first())
                .and_then(Value::as_str)
                .map(|s| hex32_eq(s, ERC20_TRANSFER_TOPIC0))
                .unwrap_or(false)
        })
        .collect();
    if candidates.is_empty() {
        return Err(ExprError {
            kind: WarningKind::AccessorFailure,
            detail: format!("no Transfer logs in receipt for `logs.{rest}`"),
        });
    }

    // Resolve filter — `self` (burner), an integer index, or `0` (first).
    let chosen: &Value = if filter == "self" {
        let burner = ctx.burner.as_deref().ok_or_else(|| ExprError {
            kind: WarningKind::AccessorFailure,
            detail: "logs[self] requires burner address but ActionContext.burner is None"
                .to_string(),
        })?;
        // For Transfer: topics[1]=from, topics[2]=to (left-padded address). Pick
        // first log whose to OR from matches the burner.
        candidates
            .iter()
            .find(|l| {
                let topics = match l.get("topics").and_then(Value::as_array) {
                    Some(t) => t,
                    None => return false,
                };
                let from = topics
                    .get(1)
                    .and_then(Value::as_str)
                    .map(topic_to_addr)
                    .unwrap_or_default();
                let to = topics
                    .get(2)
                    .and_then(Value::as_str)
                    .map(topic_to_addr)
                    .unwrap_or_default();
                addr_eq(burner, &from) || addr_eq(burner, &to)
            })
            .copied()
            .ok_or_else(|| ExprError {
                kind: WarningKind::AccessorFailure,
                detail: format!("no Transfer log involving burner {burner} for `logs.{rest}`"),
            })?
    } else if let Ok(i) = filter.parse::<usize>() {
        candidates.get(i).copied().ok_or_else(|| ExprError {
            kind: WarningKind::AccessorFailure,
            detail: format!(
                "logs.{event}[{i}] out of bounds (only {} Transfer logs)",
                candidates.len()
            ),
        })?
    } else {
        return Err(ExprError {
            kind: WarningKind::UnsupportedExpr,
            detail: format!(
                "log filter `[{filter}]` not supported in v1; use `[self]` or `[<index>]`"
            ),
        });
    };

    // Decode the requested field. Transfer: topics[1]=from, topics[2]=to,
    // data=value.
    let topics = chosen.get("topics").and_then(Value::as_array).ok_or_else(|| ExprError {
        kind: WarningKind::BadValue,
        detail: "Transfer log missing `topics` array".to_string(),
    })?;
    match field {
        "from" => {
            let raw = topics
                .get(1)
                .and_then(Value::as_str)
                .ok_or_else(|| ExprError {
                    kind: WarningKind::BadValue,
                    detail: "Transfer log missing topics[1] (from)".to_string(),
                })?;
            Ok(Value::String(topic_to_addr(raw)))
        }
        "to" => {
            let raw = topics
                .get(2)
                .and_then(Value::as_str)
                .ok_or_else(|| ExprError {
                    kind: WarningKind::BadValue,
                    detail: "Transfer log missing topics[2] (to)".to_string(),
                })?;
            Ok(Value::String(topic_to_addr(raw)))
        }
        "value" | "amount" => {
            let data = chosen
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| ExprError {
                    kind: WarningKind::BadValue,
                    detail: "Transfer log missing `data`".to_string(),
                })?;
            let v = u256_from_hex(data).map_err(|e| ExprError {
                kind: WarningKind::BadValue,
                detail: format!("Transfer data not a valid uint256: {e}"),
            })?;
            Ok(Value::String(v.to_string()))
        }
        other => Err(ExprError {
            kind: WarningKind::UnsupportedExpr,
            detail: format!(
                "Transfer field `{other}` not supported in v1; supported: from, to, value"
            ),
        }),
    }
}

fn eval_view(rest: &str, _ctx: &ActionContext<'_>) -> Result<Value, ExprError> {
    // Parse `helperName(arg1, arg2, ...)`.
    let open = rest.find('(').ok_or_else(|| ExprError {
        kind: WarningKind::UnsupportedExpr,
        detail: format!("view helpers require parentheses: `view.{rest}`"),
    })?;
    let close = rest.rfind(')').ok_or_else(|| ExprError {
        kind: WarningKind::UnsupportedExpr,
        detail: format!("view helpers require closing `)`: `view.{rest}`"),
    })?;
    let helper = &rest[..open];
    let args_raw = &rest[open + 1..close];

    match helper {
        "aaveLiquidityIndex" => {
            // v1: synchronous capture path → helper unavailable. The hook can
            // upgrade to an async-flavored helper in a follow-up; capturing
            // here would otherwise block the action confirm path on an RPC.
            // The expression is *valid*; it just defers.
            Err(ExprError {
                kind: WarningKind::HelperUnavailable,
                detail: format!(
                    "view.aaveLiquidityIndex({args_raw}) is recognised but not evaluated at capture time in v1; \
                     compute index-at-block in the view function (`ctx.evm.readContract`) instead. \
                     Aave V3 Pool on Base: 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"
                ),
            })
        }
        other => Err(ExprError {
            kind: WarningKind::HelperUnavailable,
            detail: format!(
                "view helper `{other}` is not implemented in v1; supported: aaveLiquidityIndex"
            ),
        }),
    }
}

// ─────────── small utilities ───────────

fn addr_eq(a: &str, b: &str) -> bool {
    Address::from_str(a)
        .ok()
        .zip(Address::from_str(b).ok())
        .map(|(x, y)| x == y)
        .unwrap_or(false)
}

fn hex32_eq(a: &str, b: &str) -> bool {
    let na = strip_hex(a);
    let nb = strip_hex(b);
    na.eq_ignore_ascii_case(nb)
}

fn strip_hex(s: &str) -> &str {
    s.strip_prefix("0x").unwrap_or(s)
}

fn topic_to_addr(topic: &str) -> String {
    // topics are 32-byte left-padded; take last 20 bytes.
    let s = strip_hex(topic);
    if s.len() >= 40 {
        format!("0x{}", &s[s.len() - 40..])
    } else {
        format!("0x{s}")
    }
}

fn u256_from_hex(s: &str) -> Result<U256, String> {
    let trimmed = strip_hex(s);
    if trimmed.is_empty() {
        return Ok(U256::ZERO);
    }
    U256::from_str_radix(trimmed, 16).map_err(|e| e.to_string())
}

fn short_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Compute the 4-byte function selector hex from the `function` name + the
/// ABI's input types. Returns `None` if the ABI doesn't parse or the function
/// isn't found.
fn function_selector_hex(abi_json: &str, function: &str) -> Option<String> {
    let v: Value = serde_json::from_str(abi_json).ok()?;
    let arr = v.as_array()?;
    for entry in arr {
        let is_function = entry
            .get("type")
            .and_then(Value::as_str)
            .map(|t| t == "function")
            .unwrap_or(false);
        let name = entry.get("name").and_then(Value::as_str)?;
        if !is_function || name != function {
            continue;
        }
        let inputs = entry.get("inputs").and_then(Value::as_array)?;
        let mut sig = String::with_capacity(32);
        sig.push_str(name);
        sig.push('(');
        for (i, inp) in inputs.iter().enumerate() {
            if i > 0 {
                sig.push(',');
            }
            let t = inp.get("type").and_then(Value::as_str)?;
            sig.push_str(t);
        }
        sig.push(')');
        let hash: B256 = keccak256(sig.as_bytes());
        let bytes = hash.as_slice();
        return Some(format!(
            "0x{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        ));
    }
    None
}

/// Does `selector_spec` (either a bare function name OR `0xDEADBEEF` hex)
/// match the action's ABI-resolved selector?
fn selector_matches(selector_spec: &str, abi_json: &str, function: &str) -> bool {
    if selector_spec.starts_with("0x") || selector_spec.starts_with("0X") {
        // Hex form — compute the action's selector and compare 4-byte hex.
        let Some(actual) = function_selector_hex(abi_json, function) else {
            return false;
        };
        return hex32_eq(selector_spec, &actual);
    }
    // Name form — straight string compare against `function`.
    selector_spec == function
}

// ─────────── tests ───────────

#[cfg(test)]
mod tests {
    use super::*;
    use executor_core::schema::action::{
        ContractCallAction, Erc20ApproveAction,
    };
    use serde_json::json;

    fn addr_pool() -> &'static str {
        "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5"
    }
    fn addr_usdc() -> &'static str {
        "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
    }
    fn addr_burner() -> &'static str {
        "0x0000000000000000000000000000000000000B0B"
    }

    fn aave_supply_abi() -> &'static str {
        r#"[{"type":"function","name":"supply","inputs":[
            {"name":"asset","type":"address"},
            {"name":"amount","type":"uint256"},
            {"name":"onBehalfOf","type":"address"},
            {"name":"referralCode","type":"uint16"}
        ],"outputs":[]}]"#
    }

    fn aave_supply_action() -> Action {
        Action::ContractCall(ContractCallAction {
            address: addr_pool().into(),
            abi: aave_supply_abi().into(),
            function: "supply".into(),
            args: vec![
                json!(addr_usdc()),
                json!("1000000"),
                json!(addr_burner()),
                json!(0),
            ],
            value: "0".into(),
        })
    }

    fn ctx_for<'a>(action: &'a Action) -> ActionContext<'a> {
        ActionContext {
            action,
            tx_hash: Some("0xfeedface".into()),
            block: Some(123456),
            ts: Some("2026-05-14T00:00:00Z".into()),
            gas_used: Some("42000".into()),
            burner: Some(addr_burner().into()),
            logs: None,
        }
    }

    #[test]
    fn match_contract_call_target_and_selector_by_name() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let on = json!({
            "kind": "contractCall",
            "target": addr_pool().to_lowercase(),
            "selector": "supply",
        });
        assert!(evaluate_match(&on, &ctx));
    }

    #[test]
    fn match_contract_call_selector_by_hex_4byte() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        // Aave V3 supply selector = 0x617ba037
        let on = json!({
            "kind": "contractCall",
            "selector": "0x617ba037",
        });
        assert!(evaluate_match(&on, &ctx));
    }

    #[test]
    fn match_contract_call_misses_on_wrong_target() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let on = json!({
            "kind": "contractCall",
            "target": "0x0000000000000000000000000000000000000001",
            "selector": "supply",
        });
        assert!(!evaluate_match(&on, &ctx));
    }

    #[test]
    fn match_contract_call_misses_on_wrong_selector_name() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let on = json!({
            "kind": "contractCall",
            "selector": "withdraw",
        });
        assert!(!evaluate_match(&on, &ctx));
    }

    #[test]
    fn match_erc20_approve_token_spender() {
        let action = Action::Erc20Approve(Erc20ApproveAction {
            token: addr_usdc().into(),
            spender: addr_pool().into(),
            amount: "1000".into(),
        });
        let ctx = ctx_for(&action);
        let on = json!({
            "kind": "erc20Approve",
            "token": addr_usdc(),
            "spender": addr_pool(),
        });
        assert!(evaluate_match(&on, &ctx));
    }

    #[test]
    fn match_log_topics_wildcard_then_burner() {
        // ERC20 transfer to burner.
        let topic_burner = format!(
            "0x000000000000000000000000{}",
            &addr_burner()[2..].to_lowercase()
        );
        let logs = vec![json!({
            "address": addr_usdc(),
            "topics": [
                ERC20_TRANSFER_TOPIC0,
                "0x0000000000000000000000000000000000000000000000000000000000000bad",
                topic_burner,
            ],
            "data": "0x00000000000000000000000000000000000000000000000000000000000003e8",
        })];
        let action = aave_supply_action();
        let mut ctx = ctx_for(&action);
        ctx.logs = Some(&logs);
        let on = json!({
            "kind": "log",
            "address": addr_usdc(),
            "topics": [ERC20_TRANSFER_TOPIC0, null, format!("0x000000000000000000000000{}", &addr_burner()[2..].to_lowercase())],
        });
        assert!(evaluate_match(&on, &ctx));
    }

    #[test]
    fn capture_args_positional() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let v = evaluate_expr("args[1]", &ctx).unwrap();
        assert_eq!(v, json!("1000000"));
    }

    #[test]
    fn capture_args_named_via_abi() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let v = evaluate_expr("args.amount", &ctx).unwrap();
        assert_eq!(v, json!("1000000"));
        let v = evaluate_expr("args.asset", &ctx).unwrap();
        assert_eq!(v.as_str().unwrap().to_lowercase(), addr_usdc().to_lowercase());
    }

    #[test]
    fn capture_args_named_unknown_is_warning() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let err = evaluate_expr("args.bogus", &ctx).unwrap_err();
        assert_eq!(err.kind, WarningKind::AccessorFailure);
    }

    #[test]
    fn capture_tx_accessors() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        assert_eq!(evaluate_expr("tx.hash", &ctx).unwrap(), json!("0xfeedface"));
        assert_eq!(evaluate_expr("tx.block", &ctx).unwrap(), json!(123456));
        assert_eq!(
            evaluate_expr("tx.ts", &ctx).unwrap(),
            json!("2026-05-14T00:00:00Z")
        );
        assert_eq!(evaluate_expr("tx.gas_used", &ctx).unwrap(), json!("42000"));
    }

    #[test]
    fn unknown_expression_is_unsupported_warning() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let err = evaluate_expr("blockchain.foo", &ctx).unwrap_err();
        assert_eq!(err.kind, WarningKind::UnsupportedExpr);
    }

    #[test]
    fn view_helper_unavailable_is_warning_not_panic() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let err = evaluate_expr("view.aaveLiquidityIndex(args[0])", &ctx).unwrap_err();
        assert_eq!(err.kind, WarningKind::HelperUnavailable);
    }

    #[test]
    fn capture_map_collects_warnings_per_field() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let spec = json!({
            "amount": "args[1]",
            "asset": "args[0]",
            "broken": "args[99]",
            "alsoBroken": "blockchain.foo",
        });
        let out = evaluate_capture(&spec, &ctx);
        assert_eq!(out.fields.get("amount"), Some(&json!("1000000")));
        assert!(out.fields.get("asset").is_some());
        assert!(out.fields.get("broken").is_none());
        assert!(out.fields.get("alsoBroken").is_none());
        assert_eq!(out.warnings.len(), 2);
        let kinds: Vec<_> = out.warnings.iter().map(|w| &w.kind).collect();
        assert!(kinds.contains(&&WarningKind::AccessorFailure));
        assert!(kinds.contains(&&WarningKind::UnsupportedExpr));
    }

    #[test]
    fn evaluate_spec_returns_none_when_match_misses() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let spec = RecordSpec {
            name: "supply".into(),
            on: json!({"kind": "contractCall", "selector": "withdraw"}),
            capture: json!({"amount": "args[1]"}),
        };
        assert!(evaluate_spec(&spec, &ctx).is_none());
    }

    #[test]
    fn evaluate_spec_runs_capture_when_match_hits() {
        let action = aave_supply_action();
        let ctx = ctx_for(&action);
        let spec = RecordSpec {
            name: "supply".into(),
            on: json!({
                "kind": "contractCall",
                "target": addr_pool(),
                "selector": "supply",
            }),
            capture: json!({
                "amount": "args[1]",
                "ts": "tx.ts",
            }),
        };
        let out = evaluate_spec(&spec, &ctx).expect("should match");
        assert_eq!(out.fields.get("amount"), Some(&json!("1000000")));
        assert_eq!(out.fields.get("ts"), Some(&json!("2026-05-14T00:00:00Z")));
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn function_selector_hex_for_aave_supply() {
        let sel = function_selector_hex(aave_supply_abi(), "supply").unwrap();
        assert_eq!(sel, "0x617ba037");
    }

    #[test]
    fn topic_to_addr_strips_padding() {
        let topic = "0x000000000000000000000000abcdef0000000000000000000000000000000001";
        // The topic is 32 bytes, last 20 hex pairs are the address. After
        // stripping `0x`, char positions 24..64 = 40 chars.
        let s = topic_to_addr(topic);
        assert_eq!(s.len(), 42); // 0x + 40 hex
    }

    #[test]
    fn log_accessor_transfer_value_decoded() {
        let topic_to = format!(
            "0x000000000000000000000000{}",
            &addr_burner()[2..].to_lowercase()
        );
        let logs = vec![json!({
            "address": addr_usdc(),
            "topics": [
                ERC20_TRANSFER_TOPIC0,
                "0x0000000000000000000000000000000000000000000000000000000000000bad",
                topic_to,
            ],
            // 1000 in hex, left-padded to 32 bytes
            "data": "0x00000000000000000000000000000000000000000000000000000000000003e8",
        })];
        let action = aave_supply_action();
        let mut ctx = ctx_for(&action);
        ctx.logs = Some(&logs);

        let v = evaluate_expr("logs.Transfer[self].value", &ctx).unwrap();
        assert_eq!(v, json!("1000"));

        let v = evaluate_expr("logs.Transfer[0].to", &ctx).unwrap();
        assert_eq!(v.as_str().unwrap().to_lowercase(), addr_burner().to_lowercase());
    }
}
