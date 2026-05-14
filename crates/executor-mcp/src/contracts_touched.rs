//! v1.5 Track 1B — Static `contracts_touched` extraction from strategy source.
//!
//! At `strategy_register` time we want a cheap, read-only view of which
//! contracts + selectors a strategy will touch when executed, so policy
//! alignment can surface in the register response and `strategy://{id}` reads.
//!
//! Implementation is **pure regex** over the JS source. No AST, no JS parser.
//! Three syntactic patterns are recognised:
//!
//! - Pattern A — literal address + literal function name inside
//!   `ctx.actions.contractCall({...})`.
//! - Pattern B — `const NAME = "0x..."` declarations resolved through a
//!   pre-pass map; later contractCall blocks that reference the const by
//!   bare identifier (instead of a string literal) are substituted.
//! - Pattern C — `ctx.actions.erc20Approve({ token: ..., ... })` ⇒
//!   token address mapped to selector `"approve"`.
//!
//! Anything else (computed addresses, function names from variables, dynamic
//! dispatch via helpers) sets [`ExtractionStatus::Incomplete`] and records a
//! warning. The runtime policy gate still has final say — extraction is a
//! hint, not enforcement (P6 honesty: never claim coverage you don't have).
//!
//! All addresses are normalised to **lowercase 0x-prefixed 40-hex**. Selectors
//! are the function **NAME** (lowercase JS identifier), never the 4-byte hex —
//! policy alignment (Track 1C) compares at name-level.

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Status flag attached to an [`ExtractionResult`] so downstream alignment
/// can distinguish "everything visible matched" from "we know we missed some".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionStatus {
    /// Every `ctx.actions.contractCall` / `erc20Approve` site in the source
    /// resolved to a concrete (address, selector) pair.
    Complete,
    /// At least one call site had a dynamic address (helper call, ternary,
    /// unknown identifier). Output is the best-effort subset; the runtime
    /// gate is still the source of truth.
    Incomplete,
}

/// Result of running [`extract`] over a strategy source string.
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// Normalised `address → set of selector names` map.
    pub contracts: BTreeMap<String, BTreeSet<String>>,
    pub extraction_status: ExtractionStatus,
    /// Human-readable warnings (malformed addresses, dynamic dispatch sites,
    /// etc.). Empty when [`ExtractionStatus::Complete`].
    pub warnings: Vec<String>,
}

impl ExtractionResult {
    /// Canonical JSON representation persisted on the `strategies` row and
    /// echoed back in the `strategy_register` response.
    ///
    /// Shape:
    /// ```json
    /// {
    ///   "0xabc...": ["supply", "approve"],
    ///   "_extraction": "complete" | "incomplete",
    ///   "_warnings": ["..."]
    /// }
    /// ```
    ///
    /// Reserved keys are prefixed with `_` so they can't collide with a real
    /// address (addresses are always lowercase hex starting with `0x`).
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (addr, selectors) in &self.contracts {
            let arr: Vec<serde_json::Value> = selectors
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect();
            map.insert(addr.clone(), serde_json::Value::Array(arr));
        }
        map.insert(
            "_extraction".to_string(),
            serde_json::Value::String(match self.extraction_status {
                ExtractionStatus::Complete => "complete".to_string(),
                ExtractionStatus::Incomplete => "incomplete".to_string(),
            }),
        );
        let warns: Vec<serde_json::Value> = self
            .warnings
            .iter()
            .map(|w| serde_json::Value::String(w.clone()))
            .collect();
        map.insert("_warnings".to_string(), serde_json::Value::Array(warns));
        serde_json::Value::Object(map)
    }
}

// ─── regex singletons ───────────────────────────────────────────────────────
//
// Each regex is compiled exactly once and reused across calls. `OnceLock` is
// `Sync` so this is safe under the tokio multi-thread runtime.
//
// Notes on the patterns:
// - `(?ms)` enables both multi-line `^/$` and dot-matches-newline so a
//   contractCall block spanning multiple lines is captured by a single match.
// - `[^{}]*?` is the non-greedy inter-key chomp. We deliberately bound it to
//   "no nested braces" so an outer `{...}` doesn't bleed into the next call.
//   This is OK because the action-call shape is flat (address/function are
//   simple string fields; the only object value is `args:[…]` which uses
//   square brackets).

fn const_address_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Matches:
        //   const NAME = "0x...";
        //   let NAME  = '0x...'
        //   var NAME  = "0x..."
        // 40-hex (no `0x` prefix counted in body — the prefix is required).
        Regex::new(
            r#"(?m)\b(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*["'](0x[0-9a-fA-F]{40})["']"#,
        )
        .expect("const address regex compiles")
    })
}

fn contract_call_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Matches a full `ctx.actions.contractCall({ ... })` block, capturing
        // the brace-delimited body so we can scan its keys.
        //
        // `[^{}]*?` is the non-greedy body chomp — safe because the action
        // builder shape never nests braces (args use `[...]`).
        Regex::new(r"(?ms)ctx\.actions\.contractCall\s*\(\s*\{([^{}]*?)\}\s*\)")
            .expect("contractCall regex compiles")
    })
}

fn erc20_approve_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?ms)ctx\.actions\.erc20Approve\s*\(\s*\{([^{}]*?)\}\s*\)")
            .expect("erc20Approve regex compiles")
    })
}

fn address_field_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Inside a brace body, match `address: <rhs>` where rhs is either
        // a string literal or a bare identifier.
        //   address: "0x..."   → literal
        //   address: AAVE      → identifier (look up in const map)
        //   address: foo()     → dynamic (caught as identifier OR slips
        //                       through — we explicitly detect the `(` form)
        Regex::new(r#"(?m)\baddress\s*:\s*(?:["']([^"']*)["']|([A-Za-z_$][A-Za-z0-9_$]*))"#)
            .expect("address field regex compiles")
    })
}

fn token_field_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?m)\btoken\s*:\s*(?:["']([^"']*)["']|([A-Za-z_$][A-Za-z0-9_$]*))"#)
            .expect("token field regex compiles")
    })
}

fn function_field_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // `function: "supply"` — selector is a string literal. We do NOT
        // resolve identifier-valued function fields in v1; flag those as
        // incomplete.
        Regex::new(r#"(?m)\bfunction\s*:\s*(?:["']([^"']*)["']|([A-Za-z_$][A-Za-z0-9_$]*))"#)
            .expect("function field regex compiles")
    })
}

// Dynamic-dispatch sentinel: address value that is neither a string literal
// nor a plain identifier. Catches `address: chooseRouter()`, ternaries,
// concatenations, etc.
fn dynamic_address_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // After `address:` and optional spaces, anything that is NOT a
        // double-quote, single-quote, or an identifier-start byte indicates a
        // dynamic expression on the RHS.
        Regex::new(r#"(?m)\baddress\s*:\s*([^"'\sA-Za-z_$])"#)
            .expect("dynamic address regex compiles")
    })
}

// ─── public entry point ─────────────────────────────────────────────────────

/// Run the regex extractor over a strategy `source` string.
///
/// Never panics. Malformed addresses or function names are skipped with a
/// warning rather than aborting extraction.
pub fn extract(source: &str) -> ExtractionResult {
    let mut contracts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut status = ExtractionStatus::Complete;

    // Pre-pass: build `identifier → address` map for Pattern B substitution.
    let mut const_map: BTreeMap<String, String> = BTreeMap::new();
    for cap in const_address_re().captures_iter(source) {
        let name = cap.get(1).map(|m| m.as_str().to_string());
        let addr = cap.get(2).map(|m| m.as_str().to_string());
        if let (Some(n), Some(a)) = (name, addr) {
            if let Some(normalised) = normalise_address(&a) {
                // Last-write wins; in real strategies a const is declared once.
                const_map.insert(n, normalised);
            }
        }
    }

    // Pattern A + Pattern B: `ctx.actions.contractCall({...})`.
    for cap in contract_call_re().captures_iter(source) {
        let body = match cap.get(1) {
            Some(m) => m.as_str(),
            None => continue,
        };

        // Detect dynamic dispatch BEFORE address-field extraction so a
        // mixed-case block (some literal, some dynamic) flips status.
        let is_dynamic = dynamic_address_re().is_match(body);

        // Pull `address: ...`.
        let addr_opt = resolve_address_field(body, &const_map, &mut warnings);

        // Pull `function: ...`.
        let fn_opt = resolve_function_field(body, &mut warnings);

        match (addr_opt, fn_opt) {
            (Some(addr), Some(func)) => {
                contracts.entry(addr).or_default().insert(func);
            }
            _ => {
                status = ExtractionStatus::Incomplete;
                if is_dynamic {
                    warnings.push(
                        "contractCall with dynamic `address:` (not a string literal or known const) — skipped".to_string(),
                    );
                } else {
                    warnings.push(
                        "contractCall missing or unresolved `address`/`function` field — skipped"
                            .to_string(),
                    );
                }
            }
        }

        // If we DID extract a pair but the block also had a dynamic field,
        // alignment must still be marked incomplete.
        if is_dynamic && status == ExtractionStatus::Complete {
            status = ExtractionStatus::Incomplete;
            warnings.push(
                "contractCall has dynamic address expression — extraction may be partial"
                    .to_string(),
            );
        }
    }

    // Pattern C: `ctx.actions.erc20Approve({ token: ..., ... })`.
    for cap in erc20_approve_re().captures_iter(source) {
        let body = match cap.get(1) {
            Some(m) => m.as_str(),
            None => continue,
        };

        let token_opt = resolve_token_field(body, &const_map, &mut warnings);
        match token_opt {
            Some(addr) => {
                contracts.entry(addr).or_default().insert("approve".to_string());
            }
            None => {
                status = ExtractionStatus::Incomplete;
                warnings.push(
                    "erc20Approve with unresolved `token:` field — skipped".to_string(),
                );
            }
        }
    }

    ExtractionResult {
        contracts,
        extraction_status: status,
        warnings,
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────

/// Normalise an address to lowercase 0x-prefixed 40-hex.
///
/// Returns `None` for anything that doesn't look like a 20-byte hex address.
fn normalise_address(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X"))?;
    if stripped.len() != 40 {
        return None;
    }
    if !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x{}", stripped.to_ascii_lowercase()))
}

fn resolve_address_field(
    body: &str,
    const_map: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let cap = address_field_re().captures(body)?;
    if let Some(lit) = cap.get(1) {
        let raw = lit.as_str();
        match normalise_address(raw) {
            Some(n) => Some(n),
            None => {
                warnings.push(format!("malformed address literal `{raw}` — skipped"));
                None
            }
        }
    } else if let Some(ident) = cap.get(2) {
        let name = ident.as_str();
        const_map.get(name).cloned()
    } else {
        None
    }
}

fn resolve_token_field(
    body: &str,
    const_map: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let cap = token_field_re().captures(body)?;
    if let Some(lit) = cap.get(1) {
        let raw = lit.as_str();
        match normalise_address(raw) {
            Some(n) => Some(n),
            None => {
                warnings.push(format!(
                    "malformed token address literal `{raw}` — skipped"
                ));
                None
            }
        }
    } else if let Some(ident) = cap.get(2) {
        let name = ident.as_str();
        const_map.get(name).cloned()
    } else {
        None
    }
}

fn resolve_function_field(body: &str, warnings: &mut Vec<String>) -> Option<String> {
    let cap = function_field_re().captures(body)?;
    if let Some(lit) = cap.get(1) {
        let name = lit.as_str().trim();
        if name.is_empty() {
            warnings.push("empty `function` literal — skipped".to_string());
            return None;
        }
        Some(name.to_string())
    } else if let Some(_ident) = cap.get(2) {
        // Identifier-valued function name: we don't resolve those in v1.
        warnings.push(
            "contractCall `function:` is an identifier, not a string literal — skipped"
                .to_string(),
        );
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_lowercases_and_validates() {
        assert_eq!(
            normalise_address("0xAbC1234567890123456789012345678901234567"),
            Some("0xabc1234567890123456789012345678901234567".to_string())
        );
        assert_eq!(normalise_address("not-an-address"), None);
        assert_eq!(normalise_address("0x12"), None);
    }

    #[test]
    fn empty_source_is_complete_and_empty() {
        let r = extract("");
        assert!(r.contracts.is_empty());
        assert_eq!(r.extraction_status, ExtractionStatus::Complete);
    }

    #[test]
    fn json_envelope_includes_extraction_and_warnings_keys() {
        let r = ExtractionResult {
            contracts: BTreeMap::new(),
            extraction_status: ExtractionStatus::Complete,
            warnings: vec![],
        };
        let v = r.to_json();
        assert_eq!(v["_extraction"], "complete");
        assert!(v["_warnings"].is_array());
    }
}
