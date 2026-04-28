//! Phase 5 D-01 / D-02: `Action` -> `TransactionRequest` normalization.
//!
//! Pure-function module — no provider, no RPC. Builds the `tx` shape that
//! Plan 05-02's simulator and Phase 6's signer consume.
//!
//! - [`Noop`](executor_core::schema::action::Action::Noop) returns
//!   `Ok(None)` — the orchestrator filters it out before policy/sim.
//! - The 5 emitting variants return `Ok(Some(NormalizedAction { tx,
//!   source, selector, native_value, erc20_amount }))` per the D-02 table:
//!   - `ContractCall` — calldata via [`encode_call_input`].
//!   - `RawCall` — calldata is the user-supplied hex; selector is
//!     `Some` only when calldata is at least 4 bytes (P-4).
//!   - `Erc20Transfer` / `Erc20Approve` — calldata via
//!     [`encode_call_input`] over [`ERC20_WRITE_ABI`]; `value = 0`.
//!   - `NativeTransfer` — empty calldata; full `value`.
//!
//! All bad-input paths return [`EvmError::Encode`] with stable wire-safe
//! taxonomy strings (`bad_address_to`, `bad_decimal_value`,
//! `bad_calldata`). Raw input echoes go to `detail_for_log` only
//! (Phase-4 BR-01 / MR-01 carry-forward).

use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Bytes, U256};
use executor_core::schema::action::{
    Action, ContractCallAction, Erc20ApproveAction, Erc20TransferAction, NativeTransferAction,
    RawCallAction,
};
use std::borrow::Cow;

use crate::EvmError;
use crate::action::{validate_address, validate_calldata, validate_decimal_amount};
use crate::dyn_abi::encode_call_input;
use crate::erc20::ERC20_WRITE_ABI;

/// Normalized form of a Phase-4 [`Action`] ready for simulation/signing.
///
/// `Noop` is filtered out earlier — [`normalize_action`] returns
/// `Ok(None)` for it. The `tx` field has `to`, `data` and `value`
/// populated; `gas` / `nonce` / `chain_id` are intentionally NOT set
/// (Phase 6 owns signer-side completion).
#[derive(Debug, Clone)]
pub struct NormalizedAction {
    pub tx: TransactionRequest,
    pub source: NormalizedActionKind,
    /// 4-byte calldata selector. `None` for native transfers and
    /// sub-4-byte raw calls (P-4).
    pub selector: Option<[u8; 4]>,
    /// Native-coin value attached to the call (POL-04 cap input).
    pub native_value: U256,
    /// ERC20 amount for `Erc20Transfer` / `Erc20Approve` (POL-05 cap
    /// input). `None` for non-ERC20 actions.
    pub erc20_amount: Option<U256>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedActionKind {
    ContractCall,
    RawCall,
    Erc20Transfer,
    Erc20Approve,
    NativeTransfer,
}

/// Top-level dispatcher per D-02. Returns `Ok(None)` for `Noop`; the
/// orchestrator skips Noop before calling normalize on the rest.
pub fn normalize_action(action: &Action) -> Result<Option<NormalizedAction>, EvmError> {
    match action {
        Action::Noop => Ok(None),
        Action::ContractCall(cc) => Ok(Some(normalize_contract_call(cc)?)),
        Action::RawCall(rc) => Ok(Some(normalize_raw_call(rc)?)),
        Action::Erc20Transfer(et) => Ok(Some(normalize_erc20_transfer(et)?)),
        Action::Erc20Approve(ea) => Ok(Some(normalize_erc20_approve(ea)?)),
        Action::NativeTransfer(nt) => Ok(Some(normalize_native_transfer(nt)?)),
    }
}

fn parse_address_field(
    s: &str,
    field: &'static str,
) -> Result<alloy_primitives::Address, EvmError> {
    // validate_address (Phase 4 D-09) is lenient EIP-55 + lowercase. Map
    // the inner bad_address category to the normalize-side `bad_address_to`
    // taxonomy so the wire detail names the failing field.
    validate_address(s).map_err(|inner| {
        let inner_detail = match &inner {
            EvmError::Encode { detail_for_log, .. } => detail_for_log.clone(),
            _ => format!("validate_address: {inner}"),
        };
        EvmError::Encode {
            category: Cow::Borrowed("bad_address_to"),
            detail_for_log: format!("{field} = {s} rejected: {inner_detail}"),
        }
    })
}

fn parse_decimal_field(s: &str, field: &'static str) -> Result<U256, EvmError> {
    validate_decimal_amount(s).map_err(|inner| {
        let inner_detail = match &inner {
            EvmError::Encode { detail_for_log, .. } => detail_for_log.clone(),
            _ => format!("validate_decimal_amount: {inner}"),
        };
        EvmError::Encode {
            category: Cow::Borrowed("bad_decimal_value"),
            detail_for_log: format!("{field} = {s} rejected: {inner_detail}"),
        }
    })
}

fn parse_calldata_field(s: &str, field: &'static str) -> Result<Bytes, EvmError> {
    validate_calldata(s).map_err(|inner| {
        let inner_detail = match &inner {
            EvmError::Encode { detail_for_log, .. } => detail_for_log.clone(),
            _ => format!("validate_calldata: {inner}"),
        };
        EvmError::Encode {
            category: Cow::Borrowed("bad_calldata"),
            detail_for_log: format!("{field} = {s} rejected: {inner_detail}"),
        }
    })
}

fn first_four(bytes: &[u8]) -> Option<[u8; 4]> {
    if bytes.len() < 4 {
        None
    } else {
        Some([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

pub fn normalize_contract_call(cc: &ContractCallAction) -> Result<NormalizedAction, EvmError> {
    let to = parse_address_field(&cc.address, "address")?;
    let value = parse_decimal_field(&cc.value, "value")?;
    // D-03 shared encoder. Returns selector + tail-encoded args; abi size
    // cap (D-08) is enforced inside encode_call_input.
    let calldata = encode_call_input(&cc.abi, &cc.function, &cc.args)?;
    let selector = first_four(&calldata);
    let tx = TransactionRequest::default()
        .to(to)
        .input(calldata.into())
        .value(value);
    Ok(NormalizedAction {
        tx,
        source: NormalizedActionKind::ContractCall,
        selector,
        native_value: value,
        erc20_amount: None,
    })
}

pub fn normalize_raw_call(rc: &RawCallAction) -> Result<NormalizedAction, EvmError> {
    let to = parse_address_field(&rc.address, "address")?;
    let value = parse_decimal_field(&rc.value, "value")?;
    let calldata = parse_calldata_field(&rc.data, "data")?;
    // P-4: selector is None for sub-4-byte calldata (e.g. "0x" or "0x1234").
    // The POL-06 raw_call gate (Plan 05-03) still applies regardless.
    let selector = first_four(&calldata);
    let tx = TransactionRequest::default()
        .to(to)
        .input(calldata.into())
        .value(value);
    Ok(NormalizedAction {
        tx,
        source: NormalizedActionKind::RawCall,
        selector,
        native_value: value,
        erc20_amount: None,
    })
}

pub fn normalize_erc20_transfer(et: &Erc20TransferAction) -> Result<NormalizedAction, EvmError> {
    let token = parse_address_field(&et.token, "token")?;
    // Validate the recipient before encoding so a bad `to` surfaces with
    // the normalize-side wire taxonomy (`bad_address_to`) instead of the
    // dyn_abi `bad_address` category.
    let _ = parse_address_field(&et.to, "to")?;
    let amount = parse_decimal_field(&et.amount, "amount")?;
    // ERC20_WRITE_ABI + encode_call_input prepends the 0xa9059cbb selector.
    let calldata = encode_call_input(
        ERC20_WRITE_ABI,
        "transfer",
        &[
            serde_json::Value::String(et.to.clone()),
            serde_json::Value::String(et.amount.clone()),
        ],
    )?;
    debug_assert_eq!(&calldata[..4], &[0xa9, 0x05, 0x9c, 0xbb], "transfer selector");
    let tx = TransactionRequest::default()
        .to(token)
        .input(calldata.into())
        .value(U256::ZERO);
    Ok(NormalizedAction {
        tx,
        source: NormalizedActionKind::Erc20Transfer,
        selector: Some([0xa9, 0x05, 0x9c, 0xbb]),
        native_value: U256::ZERO,
        erc20_amount: Some(amount),
    })
}

pub fn normalize_erc20_approve(ea: &Erc20ApproveAction) -> Result<NormalizedAction, EvmError> {
    let token = parse_address_field(&ea.token, "token")?;
    let _ = parse_address_field(&ea.spender, "spender")?;
    let amount = parse_decimal_field(&ea.amount, "amount")?;
    let calldata = encode_call_input(
        ERC20_WRITE_ABI,
        "approve",
        &[
            serde_json::Value::String(ea.spender.clone()),
            serde_json::Value::String(ea.amount.clone()),
        ],
    )?;
    debug_assert_eq!(&calldata[..4], &[0x09, 0x5e, 0xa7, 0xb3], "approve selector");
    let tx = TransactionRequest::default()
        .to(token)
        .input(calldata.into())
        .value(U256::ZERO);
    Ok(NormalizedAction {
        tx,
        source: NormalizedActionKind::Erc20Approve,
        selector: Some([0x09, 0x5e, 0xa7, 0xb3]),
        native_value: U256::ZERO,
        erc20_amount: Some(amount),
    })
}

pub fn normalize_native_transfer(nt: &NativeTransferAction) -> Result<NormalizedAction, EvmError> {
    let to = parse_address_field(&nt.to, "to")?;
    let value = parse_decimal_field(&nt.value, "value")?;
    let tx = TransactionRequest::default()
        .to(to)
        .input(Bytes::new().into())
        .value(value);
    Ok(NormalizedAction {
        tx,
        source: NormalizedActionKind::NativeTransfer,
        selector: None,
        native_value: value,
        erc20_amount: None,
    })
}
