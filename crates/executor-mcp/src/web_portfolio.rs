//! v1.6 Track 6C — portfolio aggregation primitives.
//!
//! Three responsibilities, kept out of `web.rs` so the route handler stays
//! readable:
//!
//! 1. **Idle balance walk.** Read native ETH balance + ERC20 balances for the
//!    burner, using the union of token addresses surfaced via each active
//!    strategy's `contracts_touched_json`. Token metadata (`symbol`,
//!    `decimals`) is cached forever in [`TokenMetaCache`] because the values
//!    don't change. Balances are NOT cached at this layer — the upstream
//!    5s view cache in `web.rs` bounds the polling load.
//!
//! 2. **`$assets` aggregation across strategies.** Each strategy view returns
//!    an object whose top-level `$assets` array (if present) lists positions.
//!    We concatenate these, tagging every entry with `_attribution` and
//!    flagging duplicate `(chain_id, venue, asset, address)` tuples with
//!    differing `amount` as `_amount_conflict: true`.
//!
//! 3. **Caps.** Hard limits keep a misbehaving strategy from blowing up the
//!    response: 50 entries per strategy, 200 entries total. Truncation is
//!    surfaced honestly via `_truncated` / `_balance_walk_status`.
//!
//! ## Wire shape
//!
//! Both the idle walk and the aggregation emit `AssetDeclaration` JSON
//! objects per the v1.6 plan §5:
//!
//! ```jsonc
//! { "chain_id": 8453, "venue": "wallet"|"aave-v3-base"|..., "asset": "USDC",
//!   "amount": "0.257164", "raw": "257164", "decimals": 6,
//!   "address": null|"0x...", "usd"?: number }
//! ```
//!
//! Aggregated entries additionally carry `_attribution: <strategy_id>` plus
//! `_amount_conflict: bool` and `_truncated: bool` flags. The frontend dedupes
//! visually; we never silently pick a winner.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

use alloy::providers::DynProvider;
use executor_evm::{
    BlockTag, EvmConfig, EvmError, erc20_balance_of, erc20_decimals, erc20_symbol,
    format_units_from_str, native_balance,
};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;

/// Per-strategy cap. A misbehaving view can't blow up the portfolio response
/// with 1000s of entries. Hit cap ⇒ contribution gets `_truncated: true`.
pub const MAX_ASSETS_PER_STRATEGY: usize = 50;

/// Total cap across all strategies + idle balances. Hit cap ⇒ response
/// surfaces `_balance_walk_status: "truncated"`.
pub const MAX_TOTAL_ASSETS: usize = 200;

/// Per-RPC timeout for the balance walk. Plan: "All RPC calls timeout after
/// 2s." Applied to each individual call (`balanceOf`, `symbol`, `decimals`,
/// native `eth_getBalance`).
pub const RPC_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Cached `(symbol, decimals)` per token address. Lives for the lifetime of
/// the web server — symbol/decimals don't change. Keyed by lowercase hex
/// address.
pub type TokenMetaCache = Arc<Mutex<HashMap<String, TokenMeta>>>;

#[derive(Clone, Debug)]
pub struct TokenMeta {
    pub symbol: String,
    pub decimals: u8,
}

/// Status sentinel for the top-level `_balance_walk_status` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BalanceWalkStatus {
    /// Walk completed (possibly with per-token skips) within the cap.
    Ok,
    /// No provider configured / construction failed ⇒ `idle_balances: []`.
    NoProvider,
    /// Total entry cap hit ⇒ aggregated list was truncated.
    Truncated,
    /// Native-balance fetch failed (chain id may have succeeded). Walk still
    /// returns whatever it managed to gather; this just signals partial.
    RpcError,
}

impl BalanceWalkStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NoProvider => "no_provider",
            Self::Truncated => "truncated",
            Self::RpcError => "rpc_error",
        }
    }
}

/// Native asset symbol for the small set of chains we surface in the UI.
/// Anything else falls back to a generic `"NATIVE"` label so the row still
/// renders without lying about the ticker.
pub fn native_symbol_for(chain_id: Option<u64>) -> &'static str {
    match chain_id {
        Some(1) | Some(11_155_111) | Some(17_000) => "ETH",
        Some(8_453) | Some(84_532) => "ETH", // base + base sepolia use ETH gas
        Some(10) | Some(11_155_420) => "ETH", // optimism + op sepolia
        Some(42_161) | Some(421_614) => "ETH", // arbitrum one + sepolia
        Some(137) | Some(80_002) => "MATIC",
        _ => "NATIVE",
    }
}

/// Build the `AssetDeclaration` JSON envelope. Optional `address` is `null`
/// when the entry is a native asset. Optional `usd` is OMITTED entirely
/// (not set to null) when the resolver had no quote — matches the v1.6
/// plan's "missing field ⇒ amount-only" semantic.
#[allow(clippy::too_many_arguments)]
pub fn asset_decl(
    chain_id: u64,
    venue: &str,
    asset: &str,
    amount: &str,
    raw: &str,
    decimals: u8,
    address: Option<&str>,
    usd: Option<f64>,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("chain_id".into(), Value::from(chain_id));
    obj.insert("venue".into(), Value::String(venue.to_string()));
    obj.insert("asset".into(), Value::String(asset.to_string()));
    obj.insert("amount".into(), Value::String(amount.to_string()));
    obj.insert("raw".into(), Value::String(raw.to_string()));
    obj.insert("decimals".into(), Value::from(decimals));
    obj.insert(
        "address".into(),
        match address {
            Some(a) => Value::String(a.to_string()),
            None => Value::Null,
        },
    );
    if let Some(u) = usd
        && let Some(n) = serde_json::Number::from_f64(u)
    {
        obj.insert("usd".into(), Value::Number(n));
    }
    Value::Object(obj)
}

/// Pull the `eth_getBalance` value for the burner as an `AssetDeclaration`.
/// Returns `None` if the call fails or the parsed amount is unrepresentable
/// — never panics. A zero balance is preserved (the wallet may have spent
/// down; honesty over hiding).
async fn fetch_native(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    price_cache: Option<&Arc<executor_evm::PriceCache>>,
    burner: &str,
    chain_id: u64,
) -> Result<Value, EvmError> {
    // Use a per-call config with our 2s envelope, NOT the global cfg.call_timeout
    // (which may be 1s — we want a slightly more lenient ceiling for the UI walk).
    let walk_cfg = EvmConfig {
        call_timeout: RPC_TIMEOUT,
        ..cfg.clone()
    };
    let raw_value = native_balance(provider.clone(), &walk_cfg, burner, BlockTag::Latest).await?;
    let raw_str = raw_value
        .as_str()
        .ok_or_else(|| EvmError::Decode {
            category: std::borrow::Cow::Borrowed("native_balance_shape"),
            detail_for_log: format!("expected decimal-string, got {raw_value:?}"),
        })?
        .to_string();
    let amount = format_units_from_str(&raw_str, 18)?;
    let usd = if let Some(cache) = price_cache {
        // Native sentinel = Address::ZERO; the resolver re-maps to WETH per chain.
        let amount_u256 = executor_evm::U256::from_str_radix(&raw_str, 10).ok();
        match amount_u256 {
            Some(a) => executor_evm::resolve_usd_micros(
                chain_id,
                executor_evm::NATIVE_SENTINEL,
                a,
                &provider,
                cache,
            )
            .await
            .map(|m| (m as f64) / 1_000_000.0),
            None => None,
        }
    } else {
        None
    };
    Ok(asset_decl(
        chain_id,
        "wallet",
        native_symbol_for(Some(chain_id)),
        &amount,
        &raw_str,
        18,
        None,
        usd,
    ))
}

/// Pull `(symbol, decimals)` for a token, consulting the cache first. Returns
/// `None` when EITHER RPC call fails — the token gets skipped from the walk.
async fn token_meta_cached(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    cache: &TokenMetaCache,
    token: &str,
) -> Option<TokenMeta> {
    let key = token.to_ascii_lowercase();
    if let Some(m) = cache.lock().await.get(&key).cloned() {
        return Some(m);
    }
    let walk_cfg = EvmConfig {
        call_timeout: RPC_TIMEOUT,
        ..cfg.clone()
    };
    let symbol_v = match erc20_symbol(provider.clone(), &walk_cfg, token, BlockTag::Latest).await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(token = %token, error = %e, "erc20_symbol failed; skipping token");
            return None;
        }
    };
    let decimals_v = match erc20_decimals(provider, &walk_cfg, token, BlockTag::Latest).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(token = %token, error = %e, "erc20_decimals failed; skipping token");
            return None;
        }
    };
    let symbol = symbol_v.as_str().unwrap_or("UNK").to_string();
    // `decimals()` returns uint8 → JSON Number per D-03; clamp into u8.
    let decimals_u64 = decimals_v.as_u64().unwrap_or(18);
    let decimals = decimals_u64.min(77) as u8;
    let meta = TokenMeta { symbol, decimals };
    cache.lock().await.insert(key, meta.clone());
    Some(meta)
}

/// Read `balanceOf(burner)` for a single token. Returns `Some(asset)` when
/// the balance is strictly positive, `None` otherwise (zero or RPC error).
async fn fetch_erc20(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    cache: &TokenMetaCache,
    price_cache: Option<&Arc<executor_evm::PriceCache>>,
    burner: &str,
    chain_id: u64,
    token: &str,
) -> Option<Value> {
    let meta = token_meta_cached(provider.clone(), cfg, cache, token).await?;
    let walk_cfg = EvmConfig {
        call_timeout: RPC_TIMEOUT,
        ..cfg.clone()
    };
    let raw_value =
        match erc20_balance_of(provider.clone(), &walk_cfg, token, burner, BlockTag::Latest).await {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(token = %token, error = %e, "erc20_balance_of failed; skipping");
                return None;
            }
        };
    let raw_str = raw_value.as_str()?.to_string();
    // Zero ⇒ omit. The UI doesn't need rows of "0 USDC".
    if raw_str == "0" {
        return None;
    }
    let amount = format_units_from_str(&raw_str, meta.decimals).ok()?;
    let usd = if let Some(pcache) = price_cache {
        let amount_u256 = executor_evm::U256::from_str_radix(&raw_str, 10).ok();
        let token_addr =
            <executor_evm::Address as std::str::FromStr>::from_str(token).ok();
        match (amount_u256, token_addr) {
            (Some(a), Some(t)) => {
                executor_evm::resolve_usd_micros(chain_id, t, a, &provider, pcache)
                    .await
                    .map(|m| (m as f64) / 1_000_000.0)
            }
            _ => None,
        }
    } else {
        None
    };
    Some(asset_decl(
        chain_id,
        "wallet",
        &meta.symbol,
        &amount,
        &raw_str,
        meta.decimals,
        Some(token),
        usd,
    ))
}

/// Extract all unique ERC20 candidate addresses referenced in the supplied
/// list of `contracts_touched_json` blobs. Lowercase + dedup. Keys starting
/// with `_` (reserved per [`contracts_touched`]) are skipped.
pub fn collect_token_candidates(blobs: &[Value]) -> Vec<String> {
    let mut set: BTreeMap<String, ()> = BTreeMap::new();
    for blob in blobs {
        let Some(obj) = blob.as_object() else { continue };
        for k in obj.keys() {
            if k.starts_with('_') {
                continue;
            }
            // Cheap shape check: lowercase 0x + 40 hex.
            if k.len() == 42 && k.starts_with("0x") && k[2..].chars().all(|c| c.is_ascii_hexdigit())
            {
                set.insert(k.to_ascii_lowercase(), ());
            }
        }
    }
    set.into_keys().collect()
}

/// Run the full balance walk. Returns `(idle_balances, status, chain_id_used)`.
/// `chain_id_used` is the resolved chain id (may be different from the input
/// if the input was None and we resolved via the provider) — propagated back
/// to the caller so the response's `chain_id` field stays in sync with the
/// labels on each native asset entry.
pub async fn run_balance_walk(
    provider: Option<Arc<DynProvider>>,
    cfg: &EvmConfig,
    cache: &TokenMetaCache,
    price_cache: Option<&Arc<executor_evm::PriceCache>>,
    burner: &str,
    chain_id: Option<u64>,
    token_candidates: &[String],
) -> (Vec<Value>, BalanceWalkStatus, Option<u64>) {
    let Some(provider) = provider else {
        return (Vec::new(), BalanceWalkStatus::NoProvider, chain_id);
    };
    // Resolve chain id if we don't have it yet. A failure here doesn't kill
    // the walk; we just omit the native row (we wouldn't know what to label
    // it) and report `rpc_error`.
    let chain_id_resolved = match chain_id {
        Some(id) => Some(id),
        None => match tokio::time::timeout(
            RPC_TIMEOUT,
            executor_evm::fetch_chain_id(&provider),
        )
        .await
        {
            Ok(Ok(id)) => Some(id),
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "balance walk: chain_id fetch failed");
                None
            }
            Err(_) => {
                tracing::warn!("balance walk: chain_id fetch timed out");
                None
            }
        },
    };
    let mut out: Vec<Value> = Vec::new();
    let mut status = BalanceWalkStatus::Ok;

    if let Some(cid) = chain_id_resolved {
        match fetch_native(provider.clone(), cfg, price_cache, burner, cid).await {
            Ok(v) => out.push(v),
            Err(e) => {
                tracing::warn!(error = %e, "balance walk: native balance failed");
                status = BalanceWalkStatus::RpcError;
            }
        }
        for token in token_candidates {
            if out.len() >= MAX_TOTAL_ASSETS {
                status = BalanceWalkStatus::Truncated;
                break;
            }
            if let Some(v) = fetch_erc20(provider.clone(), cfg, cache, price_cache, burner, cid, token).await {
                out.push(v);
            }
        }
    } else {
        // No chain id ⇒ we can't even label the native asset. Treat as a
        // partial result rather than masquerading the data.
        status = BalanceWalkStatus::RpcError;
    }
    (out, status, chain_id_resolved)
}

/// Aggregate `$assets` arrays from every strategy view. Returns the merged
/// list with attribution + conflict flags, plus a parallel map of
/// `strategy_id → was_truncated`. The merged list respects [`MAX_TOTAL_ASSETS`]
/// across all strategies + the idle balance count supplied in `already_used`.
///
/// Conflict detection: when two or more strategies declare the same
/// `(chain_id, venue, asset, address)` tuple with DIFFERENT `amount` strings,
/// every matching entry is flagged `_amount_conflict: true` and a
/// `_conflict_summary` array carries `[{"attribution":..., "amount":...}]`
/// for each.
pub fn aggregate_strategy_assets(
    strategy_views: &[(String, Value)],
    already_used: usize,
) -> (Vec<Value>, HashMap<String, bool>, bool) {
    let mut out: Vec<Value> = Vec::new();
    let mut truncated_map: HashMap<String, bool> = HashMap::new();
    let mut hit_total_cap = false;

    // First pass: gather raw entries (capped per strategy + per total).
    'strategies: for (sid, view) in strategy_views {
        let assets = match extract_assets_array(view) {
            Some(a) => a,
            None => continue,
        };
        let per_strategy_cap = assets.len() > MAX_ASSETS_PER_STRATEGY;
        truncated_map.insert(sid.clone(), per_strategy_cap);
        let take_n = assets.len().min(MAX_ASSETS_PER_STRATEGY);
        for entry in assets.into_iter().take(take_n) {
            if out.len() + already_used >= MAX_TOTAL_ASSETS {
                hit_total_cap = true;
                break 'strategies;
            }
            let Value::Object(mut m) = entry else {
                // Skip non-object entries — they don't match the
                // AssetDeclaration shape and would lie in the aggregate.
                continue;
            };
            // Inject attribution + default flags so downstream consumers
            // have a uniform shape even for non-conflicting rows.
            m.insert("_attribution".to_string(), Value::String(sid.clone()));
            m.insert("_amount_conflict".to_string(), Value::Bool(false));
            m.insert(
                "_truncated".to_string(),
                Value::Bool(per_strategy_cap),
            );
            out.push(Value::Object(m));
        }
    }

    // Second pass: dedup conflict detection across the merged list.
    flag_amount_conflicts(&mut out);

    (out, truncated_map, hit_total_cap)
}

/// Pull the `$assets` array from a view body. The view body is the
/// `{ data: <user>, confidence, ... }` envelope returned by
/// `strategy://{id}/view`. The `$assets` field MUST live at
/// `data.$assets`; anything else is ignored (per the v1.4 docs convention).
fn extract_assets_array(view_body: &Value) -> Option<Vec<Value>> {
    view_body
        .get("data")
        .and_then(Value::as_object)
        .and_then(|d| d.get("$assets"))
        .and_then(Value::as_array)
        .cloned()
}

/// Walk the aggregated entries and flag any `(chain_id, venue, asset,
/// address)` tuple appearing more than once with DIFFERING `amount` strings.
/// Identical amounts are tolerated — those are honest restatements, not
/// conflicts. Each conflicting entry also gets a `_conflict_summary` with
/// the attributed amounts of every member of the conflict group.
fn flag_amount_conflicts(entries: &mut [Value]) {
    // Build groups by tuple key. We iterate twice; first pass records
    // (tuple_key → Vec<(index, amount, attribution)>), second pass mutates.
    let mut groups: BTreeMap<String, Vec<(usize, String, String)>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        let Some(obj) = e.as_object() else { continue };
        let chain_id = obj.get("chain_id").and_then(Value::as_u64).unwrap_or(0);
        let venue = obj.get("venue").and_then(Value::as_str).unwrap_or("");
        let asset = obj.get("asset").and_then(Value::as_str).unwrap_or("");
        // `address` may be null (native) → empty string for keying.
        let address = obj.get("address").and_then(Value::as_str).unwrap_or("");
        let amount = obj
            .get("amount")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let attribution = obj
            .get("_attribution")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let key = format!(
            "{}\x1f{}\x1f{}\x1f{}",
            chain_id,
            venue,
            asset,
            address.to_ascii_lowercase()
        );
        groups.entry(key).or_default().push((i, amount, attribution));
    }
    for (_k, members) in groups {
        if members.len() < 2 {
            continue;
        }
        // Are amounts all identical?
        let first = &members[0].1;
        let all_same = members.iter().all(|(_, a, _)| a == first);
        if all_same {
            continue;
        }
        // Build the summary once and clone into every member's entry.
        let summary: Vec<Value> = members
            .iter()
            .map(|(_, amount, attr)| {
                json!({
                    "attribution": attr,
                    "amount": amount,
                })
            })
            .collect();
        for (idx, _, _) in &members {
            if let Some(Value::Object(m)) = entries.get_mut(*idx) {
                m.insert("_amount_conflict".to_string(), Value::Bool(true));
                m.insert(
                    "_conflict_summary".to_string(),
                    Value::Array(summary.clone()),
                );
            }
        }
    }
}

/// Tiny convenience: build a fresh empty `TokenMetaCache` Arc.
pub fn new_token_meta_cache() -> TokenMetaCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Build the per-strategy `_truncated` payload appended to each strategy
/// entry. Returns a `serde_json::Map` so the caller can splice it into the
/// strategy envelope without a clone-and-merge step.
pub fn strategy_contribution_summary(
    sid: &str,
    truncated_map: &HashMap<String, bool>,
) -> Map<String, Value> {
    let mut m = Map::new();
    if truncated_map.get(sid).copied().unwrap_or(false) {
        m.insert("_truncated".to_string(), Value::Bool(true));
    }
    m
}

// ─────────── unit tests ───────────

#[cfg(test)]
mod tests {
    use super::*;

    fn view_with_assets(assets: Value) -> Value {
        json!({
            "data": { "$assets": assets },
            "confidence": "full",
            "logs": [],
        })
    }

    fn entry(
        chain_id: u64,
        venue: &str,
        asset: &str,
        amount: &str,
        address: Option<&str>,
    ) -> Value {
        // Tests construct entries manually (not via asset_decl) — keep
        // them schema-shaped without a usd field, mirroring "amount-only".
        json!({
            "chain_id": chain_id,
            "venue": venue,
            "asset": asset,
            "amount": amount,
            "raw": amount,
            "decimals": 6,
            "address": address,
        })
    }

    #[test]
    fn extracts_dollar_assets_only_from_data_top_level() {
        let v = view_with_assets(json!([entry(8453, "aave", "USDC", "1.0", None)]));
        let arr = extract_assets_array(&v).expect("present");
        assert_eq!(arr.len(), 1);
        // No $assets ⇒ None.
        let v2 = json!({ "data": { "principal": "10" } });
        assert!(extract_assets_array(&v2).is_none());
        // $assets nested somewhere else ⇒ None (we only honor top-level).
        let v3 = json!({ "data": { "nested": { "$assets": [] } } });
        assert!(extract_assets_array(&v3).is_none());
    }

    #[test]
    fn aggregation_attaches_attribution_and_default_flags() {
        let views = vec![(
            "sid-1".to_string(),
            view_with_assets(json!([entry(8453, "aave", "USDC", "1.0", None)])),
        )];
        let (out, _t, _cap) = aggregate_strategy_assets(&views, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["_attribution"], json!("sid-1"));
        assert_eq!(out[0]["_amount_conflict"], json!(false));
        assert_eq!(out[0]["_truncated"], json!(false));
    }

    #[test]
    fn aggregation_flags_conflict_when_amounts_differ() {
        let views = vec![
            (
                "sid-a".to_string(),
                view_with_assets(json!([entry(8453, "aave", "USDC", "1.0", None)])),
            ),
            (
                "sid-b".to_string(),
                view_with_assets(json!([entry(8453, "aave", "USDC", "2.5", None)])),
            ),
        ];
        let (out, _, _) = aggregate_strategy_assets(&views, 0);
        assert_eq!(out.len(), 2);
        for e in &out {
            assert_eq!(e["_amount_conflict"], json!(true), "{e}");
            let summary = e["_conflict_summary"].as_array().expect("summary array");
            assert_eq!(summary.len(), 2);
        }
    }

    #[test]
    fn aggregation_does_not_flag_identical_restatements() {
        let views = vec![
            (
                "sid-a".to_string(),
                view_with_assets(json!([entry(8453, "aave", "USDC", "1.0", None)])),
            ),
            (
                "sid-b".to_string(),
                view_with_assets(json!([entry(8453, "aave", "USDC", "1.0", None)])),
            ),
        ];
        let (out, _, _) = aggregate_strategy_assets(&views, 0);
        for e in &out {
            assert_eq!(e["_amount_conflict"], json!(false));
            assert!(e.get("_conflict_summary").is_none());
        }
    }

    #[test]
    fn aggregation_truncates_strategy_at_max_assets() {
        let many: Vec<Value> = (0..(MAX_ASSETS_PER_STRATEGY + 10))
            .map(|i| entry(8453, "venue", &format!("T{i}"), "1", None))
            .collect();
        let views = vec![("sid-1".to_string(), view_with_assets(Value::Array(many)))];
        let (out, t, cap) = aggregate_strategy_assets(&views, 0);
        assert!(!cap, "per-strategy truncation should not trip the total cap");
        assert_eq!(out.len(), MAX_ASSETS_PER_STRATEGY);
        assert_eq!(t.get("sid-1"), Some(&true));
        for e in &out {
            assert_eq!(e["_truncated"], json!(true));
        }
    }

    #[test]
    fn aggregation_respects_total_cap_with_already_used_seed() {
        // Pretend the idle walk already used MAX_TOTAL_ASSETS - 3 slots.
        let already = MAX_TOTAL_ASSETS - 3;
        let many: Vec<Value> = (0..10)
            .map(|i| entry(8453, "v", &format!("X{i}"), "1", None))
            .collect();
        let views = vec![("sid-1".to_string(), view_with_assets(Value::Array(many)))];
        let (out, _, cap) = aggregate_strategy_assets(&views, already);
        assert!(cap, "must trip total cap");
        assert_eq!(out.len(), 3, "only 3 slots remained");
    }

    #[test]
    fn token_candidates_dedup_lowercase_and_skip_reserved() {
        let blobs = vec![
            json!({
                "0xAAAA000000000000000000000000000000000001": ["balanceOf"],
                "_extraction": "complete",
                "_warnings": []
            }),
            json!({
                "0xaaaa000000000000000000000000000000000001": ["transfer"],
                "0xBBBB000000000000000000000000000000000002": ["approve"]
            }),
        ];
        let v = collect_token_candidates(&blobs);
        assert_eq!(v.len(), 2);
        assert!(v.contains(&"0xaaaa000000000000000000000000000000000001".to_string()));
        assert!(v.contains(&"0xbbbb000000000000000000000000000000000002".to_string()));
    }

    #[test]
    fn token_candidates_ignore_non_addresses() {
        let blobs = vec![json!({
            "not-an-address": ["balanceOf"],
            "0xtoo-short": ["transfer"],
            "0x1234567890123456789012345678901234567890": ["balanceOf"],
            "_extraction": "complete"
        })];
        let v = collect_token_candidates(&blobs);
        assert_eq!(v, vec!["0x1234567890123456789012345678901234567890".to_string()]);
    }

    #[test]
    fn native_symbol_for_known_chains() {
        assert_eq!(native_symbol_for(Some(1)), "ETH");
        assert_eq!(native_symbol_for(Some(8453)), "ETH");
        assert_eq!(native_symbol_for(Some(137)), "MATIC");
        assert_eq!(native_symbol_for(Some(999_999)), "NATIVE");
        assert_eq!(native_symbol_for(None), "NATIVE");
    }

    #[test]
    fn aggregation_skips_non_object_entries() {
        let views = vec![(
            "sid".to_string(),
            view_with_assets(json!([
                "this is not an object",
                12345,
                entry(8453, "v", "A", "1", None),
            ])),
        )];
        let (out, _, _) = aggregate_strategy_assets(&views, 0);
        assert_eq!(out.len(), 1, "non-object entries dropped");
        assert_eq!(out[0]["asset"], json!("A"));
    }

    #[tokio::test]
    async fn balance_walk_no_provider_short_circuits() {
        let cfg = EvmConfig::default();
        let cache = new_token_meta_cache();
        let (balances, status, cid) = run_balance_walk(
            None,
            &cfg,
            &cache,
            None,
            "0x0000000000000000000000000000000000000001",
            Some(8453),
            &[],
        )
        .await;
        assert!(balances.is_empty());
        assert_eq!(status, BalanceWalkStatus::NoProvider);
        assert_eq!(cid, Some(8453));
    }
}
