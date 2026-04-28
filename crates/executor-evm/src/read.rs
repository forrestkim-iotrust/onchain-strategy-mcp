//! `read_contract` — Phase 4 D-05 entry point.
//!
//! 9-step flow (RESEARCH §"ctx.evm.readContract Design"):
//!
//! 1. Parse address → `Address`.
//! 2. Parse `abi_json` → `JsonAbi`.
//! 3. Resolve overload (Pitfall 4): pick the function whose `inputs.len() ==
//!    args.len()`. Ambiguous (>1 hit) → `Err(EvmError::Decode { abi_overload })`.
//! 4. Encode args via `js_value_to_dyn_sol` (D-03), then `Function::abi_encode_input`.
//! 5. Build `TransactionRequest::default().to(addr).input(calldata)`.
//! 6. `tokio::time::timeout(cfg.call_timeout, provider.call(&tx).block(...))`.
//!    Wall-clock 2s envelope (Phase-3 D-03) caps total run time; per-call
//!    timeout is the safety net because the QuickJS interrupt does NOT
//!    preempt `block_on` (Pitfall 1).
//! 7. Decode output bytes via `Function::abi_decode_output`.
//! 8. `dyn_sol_to_js_value` (D-03) — single output unwraps to a value;
//!    multi-output yields a JSON array.
//! 9. Return `serde_json::Value`. The caller (host binding) journals via
//!    `record_source_read`.

use std::str::FromStr;
use std::sync::Arc;

use alloy::eips::BlockId;
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::TransactionRequest;
use alloy_dyn_abi::{DynSolType, DynSolValue, FunctionExt, JsonAbiExt};
use alloy_json_abi::{Function, JsonAbi};
use alloy_primitives::{Address, Bytes};

use crate::dyn_abi::{dyn_sol_to_js_value, js_value_to_dyn_sol};
use crate::{EvmConfig, EvmError};

/// Input shape mirroring the JS-facing `ctx.evm.readContract` signature
/// (Phase 4 D-05). The strategy-js host binding builds this from the JS
/// argument object and stringifies array-form `abi` to canonical JSON.
#[derive(Debug, Clone)]
pub struct ReadContractInput {
    pub address: String,
    /// ABI as canonical JSON. The host stringifies array-form `abi` before
    /// constructing this struct so the journal records a stable representation.
    pub abi_json: String,
    pub function: String,
    pub args: Vec<serde_json::Value>,
    pub block_tag: BlockTag,
}

/// Phase 4 supports `latest` / `pending` / explicit block number. `safe` /
/// `finalized` are deferred until a strategy actually requests them.
#[derive(Debug, Clone, Copy, Default)]
pub enum BlockTag {
    #[default]
    Latest,
    Pending,
    Number(u64),
}

impl BlockTag {
    /// Public so `executor_evm::native` (and any future helper module) can
    /// translate the agent-facing tag enum into the alloy [`BlockId`] used by
    /// `provider.call`/`provider.get_balance` etc. Phase-4 D-07.
    pub fn to_block_id(self) -> BlockId {
        match self {
            BlockTag::Latest => BlockId::latest(),
            BlockTag::Pending => BlockId::pending(),
            BlockTag::Number(n) => BlockId::number(n),
        }
    }
}

/// Resolve overload, encode args, eth_call with timeout, decode output.
/// Returns the decoded output as a `serde_json::Value` per Phase 4 D-03.
pub async fn read_contract(
    provider: Arc<DynProvider>,
    cfg: &EvmConfig,
    input: ReadContractInput,
) -> Result<serde_json::Value, EvmError> {
    // 1. Parse address.
    let addr = Address::from_str(&input.address).map_err(|e| EvmError::Encode {
        category: std::borrow::Cow::Borrowed("bad_address"),
        detail_for_log: format!("address parse: {e}"),
    })?;

    // 2. Parse JsonAbi (verbatim ABI string — agent supplied).
    let abi: JsonAbi = serde_json::from_str(&input.abi_json).map_err(|e| EvmError::Decode {
        category: std::borrow::Cow::Borrowed("abi_parse"),
        detail_for_log: format!("JsonAbi: {e}"),
    })?;

    // 3. Resolve overload by argument count (Pitfall 4).
    let func = resolve_overload(&abi, &input.function, &input.args)?;

    // 4. Encode args.
    let dyn_values: Vec<DynSolValue> = func
        .inputs
        .iter()
        .zip(input.args.iter())
        .map(|(p, a)| {
            let ty: DynSolType =
                p.selector_type().parse().map_err(|e| EvmError::Encode {
                    category: std::borrow::Cow::Borrowed("abi_type_parse"),
                    detail_for_log: format!("{}: {e}", p.selector_type()),
                })?;
            js_value_to_dyn_sol(a, &ty)
        })
        .collect::<Result<_, _>>()?;
    let calldata = func
        .abi_encode_input(&dyn_values)
        .map_err(|e| EvmError::Encode {
            category: std::borrow::Cow::Borrowed("abi_encode_input"),
            detail_for_log: format!("{e}"),
        })?;

    // 5. TransactionRequest.
    let tx = TransactionRequest::default()
        .to(addr)
        .input(Bytes::from(calldata).into());

    // 6. eth_call with timeout (Pitfall 1).
    let block_id = input.block_tag.to_block_id();
    let call_fut = provider.call(tx).block(block_id);
    let bytes: Bytes = match tokio::time::timeout(cfg.call_timeout, call_fut).await {
        Err(_) => return Err(EvmError::Timeout),
        Ok(Err(transport_err)) => return Err(classify_provider_error(&transport_err)),
        Ok(Ok(bytes)) => bytes,
    };

    // 7. Decode output.
    let outputs = func.abi_decode_output(&bytes).map_err(|e| EvmError::Decode {
        category: std::borrow::Cow::Borrowed("abi_decode_output"),
        detail_for_log: format!("{e}"),
    })?;

    // 8. DynSolValue → serde_json::Value (D-03).
    let json = match outputs.as_slice() {
        [single] => dyn_sol_to_js_value(single)?,
        many => {
            let arr: Vec<_> = many
                .iter()
                .map(dyn_sol_to_js_value)
                .collect::<Result<_, _>>()?;
            serde_json::Value::Array(arr)
        }
    };

    Ok(json)
}

/// Pick the unique overload whose `inputs.len() == args.len()`. Empty +
/// ambiguous variants surface as stable Decode errors (Pitfall 4).
fn resolve_overload<'a>(
    abi: &'a JsonAbi,
    name: &str,
    args: &[serde_json::Value],
) -> Result<&'a Function, EvmError> {
    let funcs = abi.function(name).ok_or_else(|| EvmError::Decode {
        category: std::borrow::Cow::Borrowed("abi_function_not_found"),
        detail_for_log: format!("function {name} not present in ABI"),
    })?;
    let candidates: Vec<&Function> = funcs
        .iter()
        .filter(|f| f.inputs.len() == args.len())
        .collect();
    match candidates.as_slice() {
        [] => Err(EvmError::Decode {
            category: std::borrow::Cow::Borrowed("abi_overload_arity"),
            detail_for_log: format!(
                "no overload of {name} accepts {} args",
                args.len()
            ),
        }),
        [only] => Ok(*only),
        _many => Err(EvmError::Decode {
            category: std::borrow::Cow::Borrowed("abi_overload_ambiguous"),
            detail_for_log: format!(
                "function {name} has overloads; cannot disambiguate by arg count alone"
            ),
        }),
    }
}

/// Classify an alloy transport error into `EvmError::Transport` /
/// `EvmError::Revert`. Best-effort revert decoding via the standard
/// `Error(string)` selector (`0x08c379a0`). Raw error text is captured
/// in `detail_for_log` ONLY (Phase 4 D-12 / MR-01 carry-forward).
fn classify_provider_error(e: &dyn std::error::Error) -> EvmError {
    let raw = e.to_string();

    // Heuristic: an error whose Display / Debug carries the standard
    // `Error(string)` revert selector indicates a contract revert.
    // alloy 2.0's TransportErrorKind::ErrorResp carries a JSON-RPC
    // error; we look for hallmarks across the chain.
    let lower = raw.to_lowercase();
    if lower.contains("revert")
        || lower.contains("execution reverted")
        || lower.contains("0x08c379a0")
    {
        // Best-effort decode: scan for an embedded `0x08c379a0...` hex blob
        // and decode the abi-encoded `Error(string)`.
        let reason = try_extract_revert_reason(&raw).unwrap_or_else(|| "unknown".to_string());
        // WR-04: revert reason is contract-controlled (attacker can craft any
        // UTF-8 — newlines, ANSI escapes, fake taxonomy prefixes, multi-KiB).
        // Strip control chars and cap length before letting it reach the wire.
        let reason = sanitize_revert_reason(&reason);
        return EvmError::Revert {
            reason,
            detail_for_log: raw,
        };
    }

    EvmError::Transport { detail_for_log: raw }
}

/// Best-effort: scan a transport-error string for a `0x08c379a0...` payload
/// and decode the `Error(string)` selector. Returns `None` if no decodable
/// payload is present (caller falls back to `"unknown"`).
pub(crate) fn try_extract_revert_reason(raw: &str) -> Option<String> {
    // Find an `0x08c379a0` substring and treat what follows as hex.
    let needle = "08c379a0";
    let lower = raw.to_lowercase();
    let pos = lower.find(needle)?;
    // Take everything from the selector onward, stop at first non-hex char.
    let tail: String = lower[pos..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if tail.len() < 8 + 64 + 64 {
        return None;
    }
    // Decode the hex tail as bytes.
    let bytes = hex_decode_loose(&tail)?;
    // Skip selector (4 bytes) and offset (32 bytes).
    if bytes.len() < 4 + 32 + 32 {
        return None;
    }
    let len_word = &bytes[4 + 32..4 + 64];
    // Treat last 8 bytes as the length (lengths fit u64 in practice).
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&len_word[24..32]);
    let len = u64::from_be_bytes(len_bytes) as usize;
    let data_start = 4 + 64;
    if bytes.len() < data_start + len {
        return None;
    }
    let data = &bytes[data_start..data_start + len];
    String::from_utf8(data.to_vec()).ok()
}

/// WR-04: sanitize an attacker-controlled revert reason before embedding it in
/// `EvmError::Revert.reason` (which reaches the wire via `Display`). Strips
/// control characters (`\n`, `\r`, `\t`, ANSI ESC `\x1b`, plus any other
/// C0/DEL byte) and caps length at 256 bytes (truncating with an ellipsis).
/// Revert reasons are NOT trusted input — a malicious contract can revert
/// with arbitrary UTF-8 including newlines and fake taxonomy prefixes.
///
/// Phase 5 D-19: promoted from `pub(crate)` so `executor_evm::simulate` can
/// reuse the same sanitizer for `SimulationFailReason::Revert.decoded` —
/// avoids copy-paste of the WR-04 invariants.
pub fn sanitize_revert_reason(s: &str) -> String {
    const CAP: usize = 256;
    let mut out = String::with_capacity(s.len().min(CAP));
    for c in s.chars() {
        // Reject ASCII control (\x00-\x1F including \n, \r, \t, ESC) and DEL.
        if (c as u32) < 0x20 || c == '\x7f' {
            continue;
        }
        out.push(c);
    }
    if out.len() > CAP {
        // Truncate at a UTF-8 char boundary close to CAP.
        let mut end = CAP;
        while end > 0 && !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push('…');
    }
    out
}

fn hex_decode_loose(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk).ok()?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const COUNTER_ABI: &str = r#"[
        {"type":"function","name":"number","inputs":[],"outputs":[{"name":"","type":"uint256"}],"stateMutability":"view"},
        {"type":"function","name":"increment","inputs":[],"outputs":[],"stateMutability":"nonpayable"}
    ]"#;

    #[test]
    fn sanitize_revert_reason_strips_control_chars_and_caps_length() {
        // Newlines, tabs, ANSI ESC are removed.
        let dirty = "ERC20:\n insufficient\tbalance\x1b[31m red";
        let clean = sanitize_revert_reason(dirty);
        assert_eq!(clean, "ERC20: insufficientbalance[31m red");
        assert!(!clean.contains('\n'));
        assert!(!clean.contains('\r'));
        assert!(!clean.contains('\t'));
        assert!(!clean.contains('\x1b'));

        // Long inputs get truncated with an ellipsis.
        let long = "a".repeat(1000);
        let clean = sanitize_revert_reason(&long);
        assert!(clean.len() <= 256 + 4); // 256 bytes + 3-byte ellipsis
        assert!(clean.ends_with('…'));

        // Inputs ≤ 256 are not ellipsized.
        let ok = "a".repeat(200);
        let clean = sanitize_revert_reason(&ok);
        assert_eq!(clean, ok);
        assert!(!clean.ends_with('…'));

        // Fake-taxonomy prefix is NOT stripped (it's still attacker-controlled
        // text — the wire prefix `evm revert: ` distinguishes it from RPC
        // errors). This is documented WR-04 behaviour.
        let spoof = "transport";
        assert_eq!(sanitize_revert_reason(spoof), "transport");
    }

    #[test]
    fn read_contract_decode_error_when_abi_function_not_found() {
        // No anvil needed — the function-not-found check fires before any RPC.
        let cfg = EvmConfig::default();
        let provider = crate::build_provider(&cfg).unwrap();
        let input = ReadContractInput {
            address: "0x0000000000000000000000000000000000000001".into(),
            abi_json: COUNTER_ABI.into(),
            function: "doesNotExist".into(),
            args: vec![],
            block_tag: BlockTag::Latest,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(read_contract(provider, &cfg, input)).unwrap_err();
        assert_eq!(err.data_kind(), "evm_decode_error");
        match err {
            EvmError::Decode { category, .. } => {
                assert_eq!(category, "abi_function_not_found");
            }
            other => panic!("expected Decode(abi_function_not_found), got {other:?}"),
        }
    }

    #[test]
    fn read_contract_decode_error_when_overload_arity_mismatch() {
        let cfg = EvmConfig::default();
        let provider = crate::build_provider(&cfg).unwrap();
        let input = ReadContractInput {
            address: "0x0000000000000000000000000000000000000001".into(),
            abi_json: COUNTER_ABI.into(),
            function: "number".into(),
            args: vec![json!("1")], // number() takes zero args
            block_tag: BlockTag::Latest,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(read_contract(provider, &cfg, input)).unwrap_err();
        match err {
            EvmError::Decode { category, .. } => {
                assert_eq!(category, "abi_overload_arity");
            }
            other => panic!("expected Decode(abi_overload_arity), got {other:?}"),
        }
    }

    #[test]
    fn read_contract_encode_error_on_bad_address() {
        let cfg = EvmConfig::default();
        let provider = crate::build_provider(&cfg).unwrap();
        let input = ReadContractInput {
            address: "not-an-address".into(),
            abi_json: COUNTER_ABI.into(),
            function: "number".into(),
            args: vec![],
            block_tag: BlockTag::Latest,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt.block_on(read_contract(provider, &cfg, input)).unwrap_err();
        match err {
            EvmError::Encode { category, .. } => {
                assert_eq!(category, "bad_address");
            }
            other => panic!("expected Encode(bad_address), got {other:?}"),
        }
    }

    #[test]
    fn read_contract_timeout_fires_when_rpc_unreachable() {
        // Closed port — connection refused or timeout, depending on platform.
        let cfg = EvmConfig::from_raw(
            "http://127.0.0.1:1",
            200,
            "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
        )
        .unwrap();
        let provider = crate::build_provider(&cfg).unwrap();
        let input = ReadContractInput {
            address: "0x0000000000000000000000000000000000000001".into(),
            abi_json: COUNTER_ABI.into(),
            function: "number".into(),
            args: vec![],
            block_tag: BlockTag::Latest,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        let err = rt.block_on(read_contract(provider, &cfg, input)).unwrap_err();
        let elapsed = start.elapsed();
        // Either Timeout (timer fired) or Transport (connection refused
        // before timer). Both surface as evm_rpc_error wire code.
        assert_eq!(err.data_kind(), "evm_rpc_error");
        // The bound is generous: just prove the call doesn't hang past the
        // wall-clock budget by an order of magnitude.
        assert!(
            elapsed < std::time::Duration::from_millis(5_000),
            "read_contract hung: {elapsed:?}"
        );
    }

    #[test]
    fn classify_revert_finds_standard_error_string() {
        // Synthetic transport error string carrying a real revert payload.
        let raw = "execution reverted: Error(string), data=0x08c379a0\
                   0000000000000000000000000000000000000000000000000000000000000020\
                   000000000000000000000000000000000000000000000000000000000000000c\
                   48656c6c6f20576f726c64210000000000000000000000000000000000000000";
        let reason = try_extract_revert_reason(raw).expect("decodable");
        assert_eq!(reason, "Hello World!");
    }
}
