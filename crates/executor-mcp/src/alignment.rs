//! v1.5 Track 1C — `policy_alignment` derivation.
//!
//! Combines two pieces:
//!   * `contracts_touched` (Track 1B) — static regex extraction of
//!     `{address: [function_name, ...]}` from strategy source.
//!   * The active policy revision (Track 1A) — JSON body persisted in the
//!     `policies` table, shaped like `PolicyConfig` from `executor-policy`.
//!
//! Output is a `PolicyAlignment` verdict + a list of `MissingCapability`
//! entries. The verdict is **never enforced** here — the runtime gate
//! (`executor-policy::evaluate`) is the source of truth. Alignment is a
//! pre-run hint surfaced at `strategy_register`, on `strategy://{id}`,
//! `strategy://list?summary`, and inside the `policy_set` impact block so
//! the agent learns about policy gaps before invoking `strategy_run`.
//!
//! # Comparison semantics
//!
//! `contracts_touched` carries function NAMES (`"supply"`); the policy
//! carries 4-byte selectors (`"0x12345678"` or the `"any"` sentinel). We
//! deliberately do NOT compute a name → 4-byte selector here — without an
//! ABI, the canonical signature is ambiguous (`supply(address,uint256)` vs
//! `supply(address,uint256,uint16)`). The plan §3 calls this out: alignment
//! is name-level. We approximate selector coverage as follows:
//!
//!   * If `selectors.<chain>:<contract>` exists with the `"any"` entry,
//!     every function touched at that contract is considered satisfied.
//!   * If `selectors.<chain>:<contract>` exists with only specific 4-bytes,
//!     we conservatively mark the contract as **partial** — the runtime
//!     might allow some calls and deny others; the regex can't tell. The
//!     emitted `remediation` string surfaces this honestly so the agent
//!     can verify via the runtime gate or by tightening their policy
//!     authoring.
//!   * If no `selectors.<chain>:<contract>` entry exists at all, we treat
//!     the contract as denied at the selector level (deny-by-default).
//!
//! `contracts_touched` is also chain-less (the regex doesn't track
//! `chain:` from each `contractCall` body — strategies often resolve the
//! chain from `ctx.event` or hard-code via `ctx.chain`). We therefore
//! match each touched address against **every** chain's `contracts.<*>.allow`
//! and use the first match. Honest about the limitation: the registered
//! plan to upgrade this is "AST-based extraction" in v1.6+.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

/// Aggregate verdict — the top-level field surfaced in responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignmentVerdict {
    /// Every touched (contract, function) resolved to a policy allowlist
    /// entry, OR the strategy touched no contracts at all.
    Satisfied,
    /// Some addresses were covered; some are missing or selector-partial.
    Partial,
    /// No touched address is in any chain's `contracts.<chain>.allow`.
    Missing,
    /// Extraction was incomplete (dynamic dispatch / malformed source);
    /// alignment is best-effort and cannot promise coverage.
    Incomplete,
}

impl AlignmentVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            AlignmentVerdict::Satisfied => "satisfied",
            AlignmentVerdict::Partial => "partial",
            AlignmentVerdict::Missing => "missing",
            AlignmentVerdict::Incomplete => "incomplete",
        }
    }
}

/// Per-(contract, selector) reason explanation for `missing`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MissingCapability {
    pub contract: String,
    pub selectors: Vec<String>,
    /// Human-readable reason: `"contract not in policy"` or
    /// `"selectors not in policy"`.
    pub reason: String,
}

/// Top-level result. Serialised into responses verbatim.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PolicyAlignment {
    pub verdict: AlignmentVerdict,
    pub missing: Vec<MissingCapability>,
    pub remediation: Option<String>,
}

impl PolicyAlignment {
    /// `Satisfied` with no missing entries — used when there's no policy
    /// loaded, returning a vacuous-truth verdict would be a lie, so we use
    /// `Incomplete` for that path instead.
    pub fn vacuous_satisfied() -> Self {
        Self {
            verdict: AlignmentVerdict::Satisfied,
            missing: Vec::new(),
            remediation: None,
        }
    }

    /// Used when there is no active policy in the DB; alignment can't be
    /// computed without an authoritative policy to compare against.
    pub fn no_policy() -> Self {
        Self {
            verdict: AlignmentVerdict::Incomplete,
            missing: Vec::new(),
            remediation: Some(
                "no active policy revision — install one via `policy_set` to enable alignment"
                    .to_string(),
            ),
        }
    }
}

// ─── policy index ──────────────────────────────────────────────────────────
//
// A lightweight view of the policy JSON keyed by address (case-folded) so
// the lookup in `compute_alignment` is O(addresses_touched). Built once per
// alignment call. The plan calls this out as cheap at v1 scale (<100
// strategies × <50 contracts).

#[derive(Debug, Clone)]
struct PolicyIndex {
    /// `contract_addr (lowercase) → set of chain ids where it's allow-listed`.
    contracts_to_chains: std::collections::BTreeMap<String, BTreeSet<String>>,
    /// `(chain, contract_addr lowercase) → SelectorAllow`.
    selectors: std::collections::BTreeMap<(String, String), SelectorAllow>,
}

#[derive(Debug, Clone, Default)]
struct SelectorAllow {
    any: bool,
    specific: BTreeSet<String>,
}

impl PolicyIndex {
    /// Build from a policy JSON body. Tolerant of missing fields: a fresh
    /// policy with `{}` contracts produces an empty index, which makes
    /// `compute_alignment` correctly return `Missing` for any touched
    /// contract.
    fn from_json(policy: &Value) -> Self {
        let mut contracts_to_chains: std::collections::BTreeMap<String, BTreeSet<String>> =
            std::collections::BTreeMap::new();
        let mut selectors: std::collections::BTreeMap<(String, String), SelectorAllow> =
            std::collections::BTreeMap::new();

        if let Some(contracts_obj) = policy.pointer("/contracts").and_then(Value::as_object) {
            for (chain, sub) in contracts_obj {
                let allow = sub
                    .pointer("/allow")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for v in allow {
                    if let Some(addr) = v.as_str() {
                        let lc = addr.to_ascii_lowercase();
                        contracts_to_chains
                            .entry(lc)
                            .or_default()
                            .insert(chain.clone());
                    }
                }
            }
        }

        if let Some(sel_obj) = policy.pointer("/selectors").and_then(Value::as_object) {
            for (key, sub) in sel_obj {
                let (chain, contract) = match key.split_once(':') {
                    Some(p) => p,
                    None => continue,
                };
                let allow = sub
                    .pointer("/allow")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut entry = SelectorAllow::default();
                for v in allow {
                    if let Some(s) = v.as_str() {
                        if s.eq_ignore_ascii_case("any") {
                            entry.any = true;
                        } else {
                            entry.specific.insert(s.to_ascii_lowercase());
                        }
                    }
                }
                selectors.insert((chain.to_string(), contract.to_ascii_lowercase()), entry);
            }
        }

        Self {
            contracts_to_chains,
            selectors,
        }
    }

    /// `Some(chain_id_string)` if the address is allow-listed on at least
    /// one chain; returns the first match in lexicographic chain order so
    /// the selector lookup below is deterministic.
    fn lookup_chain_for_contract(&self, addr_lc: &str) -> Option<&String> {
        self.contracts_to_chains
            .get(addr_lc)
            .and_then(|set| set.iter().next())
    }
}

// ─── public entry point ────────────────────────────────────────────────────

/// Compute alignment of `contracts_touched` (raw JSON, as stored on the
/// strategies row) against the active `policy_json` (the body of the active
/// `policies` row).
///
/// `contracts_touched` may be `None` (legacy pre-v1.5 strategies) — in that
/// case we return [`AlignmentVerdict::Incomplete`] with a remediation hint
/// to re-register the strategy so the extractor populates the cache.
pub fn compute_alignment(
    contracts_touched: Option<&Value>,
    policy_json: Option<&Value>,
) -> PolicyAlignment {
    let policy = match policy_json {
        Some(p) => p,
        None => return PolicyAlignment::no_policy(),
    };
    let Some(touched) = contracts_touched else {
        return PolicyAlignment {
            verdict: AlignmentVerdict::Incomplete,
            missing: Vec::new(),
            remediation: Some(
                "strategy was registered before v1.5 (no cached contracts_touched). \
                 Re-register the same source to populate static extraction."
                    .to_string(),
            ),
        };
    };

    // Honour the extractor's incomplete signal first.
    let extraction_incomplete = touched
        .pointer("/_extraction")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("incomplete"));

    let index = PolicyIndex::from_json(policy);

    // Iterate `contracts_touched` skipping `_*` reserved keys.
    let touched_obj = match touched.as_object() {
        Some(o) => o,
        None => {
            // Malformed cache; treat as incomplete rather than panicking.
            return PolicyAlignment {
                verdict: AlignmentVerdict::Incomplete,
                missing: Vec::new(),
                remediation: Some(
                    "contracts_touched cache is malformed (not a JSON object) — re-register the strategy"
                        .to_string(),
                ),
            };
        }
    };

    let mut covered: usize = 0;
    let mut total: usize = 0;
    let mut partial: usize = 0;
    let mut missing_entries: Vec<MissingCapability> = Vec::new();

    for (addr, selectors_val) in touched_obj {
        if addr.starts_with('_') {
            continue;
        }
        total += 1;
        let funcs: Vec<String> = selectors_val
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default();

        let addr_lc = addr.to_ascii_lowercase();
        let Some(chain) = index.lookup_chain_for_contract(&addr_lc) else {
            missing_entries.push(MissingCapability {
                contract: addr.clone(),
                selectors: funcs,
                reason: "contract not in policy".to_string(),
            });
            continue;
        };

        // Contract allow-listed on at least one chain. Check selector entry.
        match index.selectors.get(&(chain.clone(), addr_lc.clone())) {
            Some(allow) if allow.any => {
                covered += 1;
            }
            Some(_allow) => {
                // Specific-only — we can't statically resolve name → 4-byte.
                // Treat as partial coverage. Surface remediation hint.
                partial += 1;
                missing_entries.push(MissingCapability {
                    contract: addr.clone(),
                    selectors: funcs,
                    reason: "selectors listed by 4-byte hex; static name-level alignment cannot verify match"
                        .to_string(),
                });
            }
            None => {
                missing_entries.push(MissingCapability {
                    contract: addr.clone(),
                    selectors: funcs,
                    reason: "selectors not in policy".to_string(),
                });
            }
        }
    }

    // Verdict roll-up.
    let verdict = if extraction_incomplete {
        AlignmentVerdict::Incomplete
    } else if total == 0 {
        // Strategy touches no contracts (pure log emitter, noop scaffolding).
        // Vacuously satisfied — no capabilities required.
        AlignmentVerdict::Satisfied
    } else if covered == total {
        AlignmentVerdict::Satisfied
    } else if covered + partial == 0 {
        AlignmentVerdict::Missing
    } else {
        AlignmentVerdict::Partial
    };

    let remediation = match verdict {
        AlignmentVerdict::Satisfied => None,
        AlignmentVerdict::Incomplete => {
            if extraction_incomplete {
                Some(
                    "static extraction was incomplete (dynamic dispatch detected). \
                     Runtime policy gate is authoritative — `strategy_run` will be \
                     evaluated against the live policy."
                        .to_string(),
                )
            } else {
                Some("alignment could not be computed; see warnings".to_string())
            }
        }
        AlignmentVerdict::Missing => Some(
            "no touched contract is allow-listed by the active policy. \
             Call `policy_set` with the additional contracts/selectors \
             needed by this strategy, or delete the strategy."
                .to_string(),
        ),
        AlignmentVerdict::Partial => Some(
            "some touched contracts are not yet allowed. Call `policy_set` \
             to widen the policy, or accept that the unmatched calls will be \
             refused at runtime."
                .to_string(),
        ),
    };

    PolicyAlignment {
        verdict,
        missing: missing_entries,
        remediation,
    }
}

/// JSON-shaped helper for response surfaces that want the full alignment as
/// a `serde_json::Value`. Equivalent to `serde_json::to_value`, but kept
/// here so the call sites stay one line and the wire shape lives next to
/// the type definition.
pub fn to_json(a: &PolicyAlignment) -> Value {
    serde_json::to_value(a).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy_with(chain: &str, contracts: &[&str], any_selectors_for: &[&str]) -> Value {
        let mut contracts_obj = serde_json::Map::new();
        contracts_obj.insert(
            chain.to_string(),
            json!({ "allow": contracts }),
        );
        let mut selectors_obj = serde_json::Map::new();
        for c in any_selectors_for {
            selectors_obj.insert(
                format!("{chain}:{c}"),
                json!({ "allow": ["any"] }),
            );
        }
        json!({
            "chains": { "allow": [chain.parse::<u64>().unwrap_or(31337)] },
            "contracts": contracts_obj,
            "selectors": selectors_obj,
            "native_value": {},
            "erc20_spend": {},
            "raw_call": { "allow_global": false, "allow": [] },
        })
    }

    #[test]
    fn satisfied_when_all_contracts_have_any_selector() {
        let touched = json!({
            "0xaaaa000000000000000000000000000000000001": ["supply"],
            "_extraction": "complete",
            "_warnings": [],
        });
        let policy = policy_with(
            "31337",
            &["0xaaaa000000000000000000000000000000000001"],
            &["0xaaaa000000000000000000000000000000000001"],
        );
        let a = compute_alignment(Some(&touched), Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Satisfied);
        assert!(a.missing.is_empty());
        assert!(a.remediation.is_none());
    }

    #[test]
    fn missing_when_contract_absent_from_policy() {
        let touched = json!({
            "0xbbbb000000000000000000000000000000000002": ["transfer"],
            "_extraction": "complete",
            "_warnings": [],
        });
        let policy = policy_with(
            "31337",
            &["0xaaaa000000000000000000000000000000000001"],
            &["0xaaaa000000000000000000000000000000000001"],
        );
        let a = compute_alignment(Some(&touched), Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Missing);
        assert_eq!(a.missing.len(), 1);
        assert_eq!(a.missing[0].contract, "0xbbbb000000000000000000000000000000000002");
        assert!(a.missing[0].reason.contains("not in policy"));
        assert!(a.remediation.is_some());
    }

    #[test]
    fn partial_when_some_match_some_dont() {
        let touched = json!({
            "0xaaaa000000000000000000000000000000000001": ["supply"],
            "0xbbbb000000000000000000000000000000000002": ["transfer"],
            "_extraction": "complete",
            "_warnings": [],
        });
        let policy = policy_with(
            "31337",
            &["0xaaaa000000000000000000000000000000000001"],
            &["0xaaaa000000000000000000000000000000000001"],
        );
        let a = compute_alignment(Some(&touched), Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Partial);
        assert_eq!(a.missing.len(), 1);
        assert_eq!(a.missing[0].contract, "0xbbbb000000000000000000000000000000000002");
    }

    #[test]
    fn incomplete_when_extraction_incomplete() {
        let touched = json!({
            "0xaaaa000000000000000000000000000000000001": ["supply"],
            "_extraction": "incomplete",
            "_warnings": ["dynamic dispatch"],
        });
        let policy = policy_with(
            "31337",
            &["0xaaaa000000000000000000000000000000000001"],
            &["0xaaaa000000000000000000000000000000000001"],
        );
        let a = compute_alignment(Some(&touched), Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Incomplete);
        assert!(a.remediation.as_ref().is_some_and(|s| s.contains("dynamic")));
    }

    #[test]
    fn no_policy_yields_incomplete() {
        let touched = json!({
            "0xaaaa000000000000000000000000000000000001": ["supply"],
            "_extraction": "complete",
            "_warnings": [],
        });
        let a = compute_alignment(Some(&touched), None);
        assert_eq!(a.verdict, AlignmentVerdict::Incomplete);
        assert!(a.remediation.as_ref().is_some_and(|s| s.contains("no active policy")));
    }

    #[test]
    fn empty_touched_is_satisfied() {
        let touched = json!({
            "_extraction": "complete",
            "_warnings": [],
        });
        let policy = policy_with("31337", &[], &[]);
        let a = compute_alignment(Some(&touched), Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Satisfied);
    }

    #[test]
    fn contract_allowed_but_selectors_specific_is_partial() {
        let touched = json!({
            "0xaaaa000000000000000000000000000000000001": ["supply"],
            "_extraction": "complete",
            "_warnings": [],
        });
        let policy = json!({
            "chains": { "allow": [31337] },
            "contracts": { "31337": { "allow": ["0xaaaa000000000000000000000000000000000001"] } },
            "selectors": {
                "31337:0xaaaa000000000000000000000000000000000001": {
                    "allow": ["0x12345678"]
                }
            },
            "native_value": {},
            "erc20_spend": {},
            "raw_call": { "allow_global": false, "allow": [] },
        });
        let a = compute_alignment(Some(&touched), Some(&policy));
        // Specific-only selectors → partial (can't verify name→4byte match).
        assert_eq!(a.verdict, AlignmentVerdict::Partial);
        assert!(a.missing[0].reason.contains("4-byte"));
    }

    #[test]
    fn case_folding_on_addresses_matches() {
        // contracts_touched normalises to lowercase; policy may carry mixed.
        let touched = json!({
            "0xaaaa000000000000000000000000000000000001": ["supply"],
            "_extraction": "complete",
            "_warnings": [],
        });
        let policy = json!({
            "chains": { "allow": [31337] },
            "contracts": { "31337": { "allow": ["0xAaAa000000000000000000000000000000000001"] } },
            "selectors": {
                "31337:0xAaAa000000000000000000000000000000000001": {
                    "allow": ["any"]
                }
            },
            "native_value": {},
            "erc20_spend": {},
            "raw_call": { "allow_global": false, "allow": [] },
        });
        let a = compute_alignment(Some(&touched), Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Satisfied);
    }

    #[test]
    fn legacy_strategy_without_cache_is_incomplete() {
        let policy = policy_with("31337", &[], &[]);
        let a = compute_alignment(None, Some(&policy));
        assert_eq!(a.verdict, AlignmentVerdict::Incomplete);
        assert!(a.remediation.as_ref().is_some_and(|s| s.contains("Re-register")));
    }

    #[test]
    fn vacuous_satisfied_helper_is_satisfied() {
        let a = PolicyAlignment::vacuous_satisfied();
        assert_eq!(a.verdict, AlignmentVerdict::Satisfied);
    }
}
