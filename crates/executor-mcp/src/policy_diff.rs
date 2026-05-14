//! v1.5 Track 1A — hand-rolled JSON Patch (RFC 6902) computation for the
//! `policy_set` response.
//!
//! The plan permits either the `json-patch` crate or a hand-rolled diff; we
//! ship the hand-roll to avoid a workspace-wide dep for v1. The diff is
//! ordered (`object_keys_sorted`) so the response is deterministic across
//! runs even though `HashMap` iteration order is not.
//!
//! ## Capabilities granted
//!
//! [`new_capabilities_granted`] walks the diff ops and produces
//! human-readable strings for the response's `impact.new_capabilities_granted`
//! field. It surfaces two flavours of "add":
//!
//!   - A wholly new contract address allowed on a chain
//!     (`contracts.<chain>.allow[N]`), formatted as
//!     `"Chain {chain}: {contract}"`.
//!   - A new selector under an existing `(chain, contract)` pair
//!     (`selectors."<chain>:<contract>".allow[N]`), formatted as
//!     `"Chain {chain}: {contract} {selector}"`.

use serde_json::{Value, json};

/// JSON-Patch op kinds we emit. The wire serialization follows RFC 6902.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOp {
    Add { path: String, value: Value },
    Remove { path: String },
    Replace { path: String, value: Value },
}

impl DiffOp {
    pub fn to_json(&self) -> Value {
        match self {
            DiffOp::Add { path, value } => {
                json!({ "op": "add", "path": path, "value": value })
            }
            DiffOp::Remove { path } => {
                json!({ "op": "remove", "path": path })
            }
            DiffOp::Replace { path, value } => {
                json!({ "op": "replace", "path": path, "value": value })
            }
        }
    }

    pub fn path(&self) -> &str {
        match self {
            DiffOp::Add { path, .. }
            | DiffOp::Remove { path }
            | DiffOp::Replace { path, .. } => path.as_str(),
        }
    }
}

/// Compute a JSON Patch from `old` → `new`. Order is deterministic: object
/// keys are visited in sorted order; arrays are length-compared as a whole
/// (we emit a single `replace` on the array path when contents differ) to
/// keep the patch small and readable. This is enough for human review and
/// for the `new_capabilities_granted` extractor below.
pub fn diff_json(old: &Value, new: &Value) -> Vec<DiffOp> {
    let mut ops = Vec::new();
    walk("".to_string(), old, new, &mut ops);
    ops
}

fn walk(path: String, old: &Value, new: &Value, ops: &mut Vec<DiffOp>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let escaped = escape_pointer_token(k);
                let child = format!("{path}/{escaped}");
                match (a.get(k), b.get(k)) {
                    (Some(av), Some(bv)) => walk(child, av, bv, ops),
                    (None, Some(bv)) => ops.push(DiffOp::Add {
                        path: child,
                        value: bv.clone(),
                    }),
                    (Some(_), None) => ops.push(DiffOp::Remove { path: child }),
                    (None, None) => {}
                }
            }
        }
        // Arrays: emit a coarse `replace` on the path rather than per-index
        // ops. The policy diff is read by humans and the
        // `new_capabilities_granted` extractor already specializes
        // contracts/selectors via their parent object paths.
        _ => {
            ops.push(DiffOp::Replace {
                path,
                value: new.clone(),
            });
        }
    }
}

/// RFC 6901 path-token escaping: `~` → `~0`, `/` → `~1`.
fn escape_pointer_token(t: &str) -> String {
    t.replace('~', "~0").replace('/', "~1")
}

/// Extract human-readable capability descriptions from the diff. Surfaces:
///   - new contract addresses allowed on a chain
///   - new selectors allowed under a (chain, contract) pair
///   - `raw_call.allow_global = true` (replace `false → true`)
///
/// The intent is the response message that makes `[DESTRUCTIVE]` worthwhile
/// — the operator/agent sees a one-line list of "what this policy now
/// allows that it didn't before" before confirming.
pub fn new_capabilities_granted(old: &Value, new: &Value, ops: &[DiffOp]) -> Vec<String> {
    let mut out = Vec::new();

    // 1. raw_call.allow_global true-flip.
    let old_global = old
        .pointer("/raw_call/allow_global")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let new_global = new
        .pointer("/raw_call/allow_global")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !old_global && new_global {
        out.push("Global raw_call gate ENABLED (raw_call.allow_global = true)".to_string());
    }

    // 2. New contract addresses per chain.
    // Old/new shape: { contracts: { "<chain>": { allow: [...] } } }
    let old_contracts = old
        .pointer("/contracts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let new_contracts = new
        .pointer("/contracts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut chain_keys: Vec<&String> = new_contracts.keys().collect();
    chain_keys.sort();
    for chain in chain_keys {
        let new_allow: Vec<&str> = new_contracts
            .get(chain)
            .and_then(|v| v.pointer("/allow"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let old_allow: Vec<&str> = old_contracts
            .get(chain)
            .and_then(|v| v.pointer("/allow"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for addr in &new_allow {
            if !old_allow.iter().any(|a| a.eq_ignore_ascii_case(addr)) {
                out.push(format!("Chain {chain}: {addr}"));
            }
        }
    }

    // 3. New selectors per (chain, contract) pair.
    // Shape: { selectors: { "<chain>:<contract>": { allow: [<selector>, ...] } } }
    let old_sel = old
        .pointer("/selectors")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let new_sel = new
        .pointer("/selectors")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut sel_keys: Vec<&String> = new_sel.keys().collect();
    sel_keys.sort();
    for key in sel_keys {
        let (chain, contract) = match key.split_once(':') {
            Some(pair) => pair,
            None => continue,
        };
        let new_allow: Vec<&str> = new_sel
            .get(key)
            .and_then(|v| v.pointer("/allow"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let old_allow: Vec<&str> = old_sel
            .get(key)
            .and_then(|v| v.pointer("/allow"))
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for sel in &new_allow {
            if !old_allow.contains(sel) {
                out.push(format!("Chain {chain}: {contract} {sel}"));
            }
        }
    }

    // Belt-and-braces: silence the ops parameter — kept in the signature so
    // future expansions (e.g. surfacing native_value cap relaxations) have
    // structured input. For 1A the value extraction is exhaustive enough
    // without iterating ops.
    let _ = ops;

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_for_equal_values() {
        let v = json!({ "a": 1 });
        assert!(diff_json(&v, &v).is_empty());
    }

    #[test]
    fn add_object_key() {
        let a = json!({});
        let b = json!({ "x": 1 });
        let ops = diff_json(&a, &b);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], DiffOp::Add { path, .. } if path == "/x"));
    }

    #[test]
    fn replace_array_emits_single_op() {
        let a = json!({ "xs": [1, 2] });
        let b = json!({ "xs": [1, 2, 3] });
        let ops = diff_json(&a, &b);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], DiffOp::Replace { path, .. } if path == "/xs"));
    }

    #[test]
    fn new_contract_address_surfaces_as_capability() {
        let old = json!({
            "contracts": { "8453": { "allow": [] } },
        });
        let new = json!({
            "contracts": { "8453": { "allow": ["0xc3D688B6"] } },
        });
        let ops = diff_json(&old, &new);
        let caps = new_capabilities_granted(&old, &new, &ops);
        assert_eq!(caps, vec!["Chain 8453: 0xc3D688B6"]);
    }

    #[test]
    fn new_selector_surfaces_with_pair_path() {
        let old = json!({
            "selectors": { "8453:0xc3D688B6": { "allow": [] } },
        });
        let new = json!({
            "selectors": { "8453:0xc3D688B6": { "allow": ["supply"] } },
        });
        let ops = diff_json(&old, &new);
        let caps = new_capabilities_granted(&old, &new, &ops);
        assert_eq!(caps, vec!["Chain 8453: 0xc3D688B6 supply"]);
    }

    #[test]
    fn raw_call_global_flip_surfaces() {
        let old = json!({ "raw_call": { "allow_global": false } });
        let new = json!({ "raw_call": { "allow_global": true } });
        let ops = diff_json(&old, &new);
        let caps = new_capabilities_granted(&old, &new, &ops);
        assert_eq!(caps.len(), 1);
        assert!(caps[0].contains("raw_call.allow_global"));
    }
}
