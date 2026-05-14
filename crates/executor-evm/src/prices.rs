//! Local USD price resolver — v1.6 deferred item (`ctx.price.usd`).
//!
//! Two sources, no external network dependencies:
//!
//! 1. **Static stablecoin map.** USDC / USDT / DAI / USDbC on Base + Mainnet
//!    return $1.00 (`1_000_000` micros per whole token).
//! 2. **Uniswap V3 pool `slot0`** for ETH (native or WETH) on Base + Mainnet.
//!    We read `sqrtPriceX96` from the canonical WETH/USDC 0.05% pool and
//!    derive ETH USD via fixed-point U256 math (no `f64` until the JS
//!    boundary).
//!
//! Everything else returns `None`. Callers degrade gracefully (portfolio
//! rows show "amount-only").
//!
//! ## Cache semantics
//!
//! - Positive hits cached for [`POSITIVE_TTL`] (60s).
//! - Negative hits (RPC timeout / unsupported pool) cached for
//!   [`NEGATIVE_TTL`] (10s). Short so a transient blip doesn't kill pricing
//!   for long, but long enough that a flaky pool doesn't get hammered.
//! - Keyed `(chain_id, lookup_address)`. Native sentinel re-maps to the
//!   chain's WETH equivalent so native ETH and explicit WETH share one
//!   cache entry — correct because the runtime prices them identically.
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, Bytes, U256};
use tokio::sync::Mutex;

pub const POSITIVE_TTL: Duration = Duration::from_secs(60);
pub const NEGATIVE_TTL: Duration = Duration::from_secs(10);
pub const POOL_RPC_TIMEOUT: Duration = Duration::from_millis(2_000);
pub const NATIVE_SENTINEL: Address = Address::ZERO;

const SLOT0_SELECTOR: [u8; 4] = [0x38, 0x50, 0xc7, 0xbd];

#[derive(Clone, Copy, Debug)]
struct CacheEntry {
    unit_micros: Option<u128>,
    decimals: u8,
    inserted_at: Instant,
}

#[derive(Clone, Debug)]
pub struct PriceCache {
    inner: Arc<Mutex<HashMap<(u64, Address), CacheEntry>>>,
}

impl PriceCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for PriceCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
struct Stable {
    decimals: u8,
}

#[derive(Clone, Copy, Debug)]
struct EthPool {
    weth: Address,
    pool: Address,
    weth_decimals: u8,
    usdc_decimals: u8,
}

fn stable_for(chain_id: u64, token: Address) -> Option<Stable> {
    let stables: &[(u64, &str, u8)] = &[
        (8453, "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", 6),
        (8453, "0xd9aaec86b65d86f6a7b5b1b0c42ffa531710b6ca", 6),
        (1, "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", 6),
        (1, "0xdac17f958d2ee523a2206206994597c13d831ec7", 6),
        (1, "0x6b175474e89094c44da98b954eedeac495271d0f", 18),
    ];
    for (cid, raw, decimals) in stables {
        if *cid != chain_id {
            continue;
        }
        if let Ok(addr) = Address::from_str(raw)
            && addr == token
        {
            return Some(Stable { decimals: *decimals });
        }
    }
    None
}

fn eth_pool_for(chain_id: u64) -> Option<EthPool> {
    match chain_id {
        8453 => Some(EthPool {
            weth: Address::from_str("0x4200000000000000000000000000000000000006")
                .expect("static WETH address parses"),
            pool: Address::from_str("0xd0b53D9277642d899DF5C87A3966A349A798F224")
                .expect("static WETH/USDC pool address parses"),
            weth_decimals: 18,
            usdc_decimals: 6,
        }),
        1 => Some(EthPool {
            weth: Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")
                .expect("static WETH address parses"),
            pool: Address::from_str("0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640")
                .expect("static WETH/USDC pool address parses"),
            weth_decimals: 18,
            usdc_decimals: 6,
        }),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
enum TokenKind {
    Stable { decimals: u8 },
    Eth,
}

fn lookup_address(chain_id: u64, token: Address) -> Option<(Address, TokenKind)> {
    if token == NATIVE_SENTINEL {
        let pool = eth_pool_for(chain_id)?;
        return Some((pool.weth, TokenKind::Eth));
    }
    if let Some(s) = stable_for(chain_id, token) {
        return Some((token, TokenKind::Stable { decimals: s.decimals }));
    }
    if let Some(pool) = eth_pool_for(chain_id)
        && token == pool.weth
    {
        return Some((token, TokenKind::Eth));
    }
    None
}

pub async fn resolve_usd_micros(
    chain_id: u64,
    token: Address,
    amount: U256,
    provider: &Arc<DynProvider>,
    cache: &PriceCache,
) -> Option<u128> {
    let (lookup, kind) = lookup_address(chain_id, token)?;
    if let Some(entry) = cache_get(cache, chain_id, lookup).await {
        return scale_amount(entry.unit_micros?, entry.decimals, amount);
    }
    let (unit_micros, decimals): (Option<u128>, u8) = match kind {
        TokenKind::Stable { decimals } => (Some(1_000_000), decimals),
        TokenKind::Eth => {
            let pool = match eth_pool_for(chain_id) {
                Some(p) => p,
                None => return None,
            };
            match quote_eth_via_uniswap_v3(provider, pool).await {
                Some(micros) => (Some(micros), pool.weth_decimals),
                None => (None, pool.weth_decimals),
            }
        }
    };
    cache_insert(cache, chain_id, lookup, unit_micros, decimals).await;
    scale_amount(unit_micros?, decimals, amount)
}

fn scale_amount(unit_micros: u128, decimals: u8, amount: U256) -> Option<u128> {
    let scale = U256::from(10u128).checked_pow(U256::from(decimals as u64))?;
    let unit = U256::from(unit_micros);
    let prod = unit.checked_mul(amount)?;
    let micros_u256 = prod.checked_div(scale)?;
    u128_from_u256(micros_u256)
}

fn u128_from_u256(v: U256) -> Option<u128> {
    let bytes = v.to_be_bytes::<32>();
    if bytes[..16].iter().any(|b| *b != 0) {
        return None;
    }
    let mut tail = [0u8; 16];
    tail.copy_from_slice(&bytes[16..]);
    Some(u128::from_be_bytes(tail))
}

async fn cache_get(cache: &PriceCache, chain_id: u64, lookup: Address) -> Option<CacheEntry> {
    let g = cache.inner.lock().await;
    let entry = g.get(&(chain_id, lookup)).copied()?;
    let ttl = if entry.unit_micros.is_some() { POSITIVE_TTL } else { NEGATIVE_TTL };
    if entry.inserted_at.elapsed() >= ttl {
        return None;
    }
    Some(entry)
}

async fn cache_insert(
    cache: &PriceCache,
    chain_id: u64,
    lookup: Address,
    unit_micros: Option<u128>,
    decimals: u8,
) {
    let mut g = cache.inner.lock().await;
    g.insert(
        (chain_id, lookup),
        CacheEntry { unit_micros, decimals, inserted_at: Instant::now() },
    );
}

async fn quote_eth_via_uniswap_v3(
    provider: &Arc<DynProvider>,
    pool: EthPool,
) -> Option<u128> {
    let tx = TransactionRequest::default()
        .to(pool.pool)
        .input(Bytes::from(SLOT0_SELECTOR.to_vec()).into());
    let call = provider.call(tx);
    let bytes: Bytes = match tokio::time::timeout(POOL_RPC_TIMEOUT, call).await {
        Err(_) => {
            tracing::debug!(pool = %pool.pool, "uniswap v3 slot0 call timed out");
            return None;
        }
        Ok(Err(e)) => {
            tracing::debug!(pool = %pool.pool, error = %e, "uniswap v3 slot0 transport error");
            return None;
        }
        Ok(Ok(b)) => b,
    };
    let sqrt_price_x96 = decode_slot0_sqrt_price(&bytes)?;
    micros_per_weth_from_sqrt(sqrt_price_x96, pool.weth_decimals, pool.usdc_decimals)
}

fn decode_slot0_sqrt_price(bytes: &[u8]) -> Option<U256> {
    if bytes.len() < 32 {
        tracing::debug!(len = bytes.len(), "slot0 return shorter than one word");
        return None;
    }
    if bytes[..12].iter().any(|b| *b != 0) {
        return None;
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&bytes[..32]);
    Some(U256::from_be_bytes(buf))
}

fn micros_per_weth_from_sqrt(
    sqrt_price_x96: U256,
    weth_decimals: u8,
    usdc_decimals: u8,
) -> Option<u128> {
    let exponent_signed: i32 = weth_decimals as i32 + 6 - usdc_decimals as i32;
    if !(0..=60).contains(&exponent_signed) {
        tracing::debug!(exponent = exponent_signed, "sqrt-price exponent out of safe range");
        return None;
    }
    let exponent = exponent_signed as u8;
    // Strategy: compute `(sqrtPriceX96^2 * 10^exponent) >> 192` end-to-end in
    // U256. For sane prices (sqrtPriceX96 fits in ~130 bits) `sp*sp` fits in
    // ~260 bits — most realistic values stay below U256::MAX, but to be safe
    // we re-order: do the square first, then multiply by the (small) scale,
    // then shift. The shift right by 192 effectively divides by 2^192 with
    // truncation toward zero. We multiply by `10^exponent` BEFORE the shift
    // so the result keeps enough precision for sub-dollar reads.
    let sp_sq = sqrt_price_x96.checked_mul(sqrt_price_x96)?;
    let scale = U256::from(10u128).checked_pow(U256::from(exponent as u64))?;
    let numerator = sp_sq.checked_mul(scale)?;
    let micros_u256 = numerator >> 192;
    u128_from_u256(micros_u256)
}

#[cfg(test)]
pub(crate) async fn cache_insert_for_test(
    cache: &PriceCache,
    chain_id: u64,
    lookup: Address,
    unit_micros: Option<u128>,
    decimals: u8,
    inserted_at: Instant,
) {
    let mut g = cache.inner.lock().await;
    g.insert((chain_id, lookup), CacheEntry { unit_micros, decimals, inserted_at });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EvmConfig;

    fn usdc_base() -> Address {
        Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap()
    }
    fn usdc_mainnet() -> Address {
        Address::from_str("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").unwrap()
    }
    fn dai_mainnet() -> Address {
        Address::from_str("0x6b175474e89094c44da98b954eedeac495271d0f").unwrap()
    }
    fn random_token() -> Address {
        Address::from_str("0x1234567890abcdef1234567890abcdef12345678").unwrap()
    }

    #[test]
    fn lookup_recognises_stables_on_base_and_mainnet() {
        match lookup_address(8453, usdc_base()) {
            Some((addr, TokenKind::Stable { decimals })) => {
                assert_eq!(addr, usdc_base());
                assert_eq!(decimals, 6);
            }
            other => panic!("base USDC unexpected: {other:?}"),
        }
        match lookup_address(1, usdc_mainnet()) {
            Some((_, TokenKind::Stable { decimals })) => assert_eq!(decimals, 6),
            other => panic!("mainnet USDC unexpected: {other:?}"),
        }
        match lookup_address(1, dai_mainnet()) {
            Some((_, TokenKind::Stable { decimals })) => assert_eq!(decimals, 18),
            other => panic!("mainnet DAI unexpected: {other:?}"),
        }
    }

    #[test]
    fn lookup_native_routes_to_weth() {
        let (lookup, kind) = lookup_address(8453, NATIVE_SENTINEL).expect("base native");
        let weth_base = Address::from_str("0x4200000000000000000000000000000000000006").unwrap();
        assert_eq!(lookup, weth_base);
        assert!(matches!(kind, TokenKind::Eth));
        let (lookup, _) = lookup_address(1, NATIVE_SENTINEL).expect("mainnet native");
        let weth_main = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        assert_eq!(lookup, weth_main);
    }

    #[test]
    fn lookup_unknown_chain_returns_none() {
        assert!(lookup_address(137, usdc_base()).is_none());
        assert!(lookup_address(42_161, NATIVE_SENTINEL).is_none());
    }

    #[test]
    fn lookup_unknown_token_returns_none() {
        assert!(lookup_address(8453, random_token()).is_none());
        assert!(lookup_address(1, random_token()).is_none());
    }

    #[test]
    fn scale_amount_stables_round_trip() {
        let m = scale_amount(1_000_000, 6, U256::from(1_000_000u64)).unwrap();
        assert_eq!(m, 1_000_000);
        let m = scale_amount(1_000_000, 6, U256::from(12_500_000u64)).unwrap();
        assert_eq!(m, 12_500_000);
        let m = scale_amount(1_000_000, 18, U256::from(1u64)).unwrap();
        assert_eq!(m, 0);
    }

    #[test]
    fn scale_amount_dai_one_whole_token_is_one_dollar() {
        let one = U256::from(10u128).checked_pow(U256::from(18u64)).unwrap();
        let m = scale_amount(1_000_000, 18, one).unwrap();
        assert_eq!(m, 1_000_000);
    }

    #[test]
    fn scale_amount_overflow_returns_none() {
        let m = scale_amount(u128::MAX, 0, U256::MAX);
        assert!(m.is_none());
    }

    #[test]
    fn decode_slot0_reads_first_word() {
        let mut blob = vec![0u8; 32 * 2];
        for i in 0..20 {
            blob[12 + i] = 0xde;
        }
        let v = decode_slot0_sqrt_price(&blob).unwrap();
        assert!(v > U256::ZERO);
        blob[0] = 0xff;
        assert!(decode_slot0_sqrt_price(&blob).is_none());
    }

    #[test]
    fn decode_slot0_short_blob_is_none() {
        let short = vec![0u8; 31];
        assert!(decode_slot0_sqrt_price(&short).is_none());
        assert!(decode_slot0_sqrt_price(&[]).is_none());
    }

    /// Real-world-ish sqrtPriceX96 for WETH/USDC at ~$2500/ETH.
    /// `sqrt(2.5e-9) ≈ 5.0e-5 ⇒ sqrtPriceX96 ≈ 3.961e24`.
    #[test]
    fn sqrt_price_math_yields_plausible_eth_price() {
        let sp = U256::from_str_radix("3961408125713210000000000", 10).unwrap();
        let micros = micros_per_weth_from_sqrt(sp, 18, 6).expect("converts");
        let lower = 2_500_000_000u128 * 98 / 100;
        let upper = 2_500_000_000u128 * 102 / 100;
        assert!(
            (lower..=upper).contains(&micros),
            "expected ~$2500 in micros, got {micros}"
        );
    }

    #[test]
    fn sqrt_price_math_handles_unit_pair_smoke() {
        let sp = U256::from(1u64) << 96;
        let micros = micros_per_weth_from_sqrt(sp, 18, 6);
        assert_eq!(micros, Some(1_000_000_000_000_000_000));
    }

    #[tokio::test]
    async fn cache_negative_entries_expire_faster_than_positive() {
        let cache = PriceCache::new();
        let chain = 8453u64;
        let token = usdc_base();
        cache_insert_for_test(
            &cache,
            chain,
            token,
            None,
            6,
            Instant::now() - (NEGATIVE_TTL - Duration::from_millis(50)),
        )
        .await;
        let hit = cache_get(&cache, chain, token).await;
        assert!(hit.is_some(), "negative entry within TTL must be a hit");
        assert!(hit.unwrap().unit_micros.is_none());
        cache_insert_for_test(
            &cache,
            chain,
            token,
            None,
            6,
            Instant::now() - (NEGATIVE_TTL + Duration::from_secs(1)),
        )
        .await;
        let miss = cache_get(&cache, chain, token).await;
        assert!(miss.is_none(), "expired negative entry must miss");
        cache_insert_for_test(
            &cache,
            chain,
            token,
            Some(1_000_000),
            6,
            Instant::now() - (NEGATIVE_TTL + Duration::from_secs(1)),
        )
        .await;
        let hit2 = cache_get(&cache, chain, token).await;
        assert!(
            hit2.is_some(),
            "positive entry past NEGATIVE_TTL but within POSITIVE_TTL must hit"
        );
        assert_eq!(hit2.unwrap().unit_micros, Some(1_000_000));
    }

    #[tokio::test]
    async fn resolve_stable_hits_static_map_without_rpc() {
        let cfg = EvmConfig::default();
        let provider = crate::provider::build_provider(&cfg)
            .expect("provider builds against config URL even without network");
        let cache = PriceCache::new();
        let amount = U256::from(1_234_000_000u128);
        let micros = resolve_usd_micros(8453, usdc_base(), amount, &provider, &cache).await;
        assert_eq!(micros, Some(1_234_000_000));
    }

    #[tokio::test]
    async fn resolve_unknown_token_returns_none_without_rpc() {
        let cfg = EvmConfig::default();
        let provider = crate::provider::build_provider(&cfg)
            .expect("provider builds against config URL even without network");
        let cache = PriceCache::new();
        let micros = resolve_usd_micros(8453, random_token(), U256::from(1u64), &provider, &cache).await;
        assert!(micros.is_none());
    }

    #[tokio::test]
    async fn cache_hit_short_circuits_rpc() {
        let cfg = EvmConfig::default();
        let provider = crate::provider::build_provider(&cfg)
            .expect("provider builds against config URL even without network");
        let cache = PriceCache::new();
        let weth_base = Address::from_str("0x4200000000000000000000000000000000000006").unwrap();
        cache_insert_for_test(&cache, 8453, weth_base, Some(3_000_000_000), 18, Instant::now())
            .await;
        let amount = U256::from(10u128).checked_pow(U256::from(18u64)).unwrap();
        let micros = resolve_usd_micros(8453, NATIVE_SENTINEL, amount, &provider, &cache)
            .await
            .expect("cache hit");
        assert_eq!(micros, 3_000_000_000);
    }
}
