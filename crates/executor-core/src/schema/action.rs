//! Phase-4 Action wire schema (D-08).
//!
//! Six variants — `Noop` (Phase 3) plus the five Phase-4 write actions
//! (`ContractCall`, `RawCall`, `Erc20Transfer`, `Erc20Approve`,
//! `NativeTransfer`). All Phase-4 variants enforce `deny_unknown_fields`;
//! forward-compat is via NEW variants (Phase 5+ may add e.g. `MultiCall` and
//! gate it through [`Action::phase4_emittable`]). Mirrors the
//! `RunStatus::phase2_emittable` / `JournalActionOutcome::phase3_emittable`
//! future-lock pattern (Phase 2 D-05 / Phase 3 D-06).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum allowed serialized ABI size for `ContractCallAction.abi`.
///
/// Typical ERC20 ABI ≈ 5 KiB; full DEX router ≈ 20 KiB. 64 KiB caps
/// pathological inputs (e.g. a 200 KiB malformed ABI shipped by a strategy
/// trying to DoS the validator) without rejecting legitimate uses
/// (D-08 / RESEARCH Pitfall 11).
pub const MAX_ABI_BYTES: usize = 64 * 1024; // 65_536

/// Default `value` field for variants that omit explicit native-coin transfer.
/// Wire format is decimal-string (D-03 BigInt bridge); `"0"` is the universal
/// "no native value attached" marker.
fn default_zero_value() -> String {
    "0".into()
}

/// Phase-4 action wire schema. The discriminator field is `kind`
/// (snake_case), and every Phase-4 variant struct rejects unknown fields.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Phase-3 sentinel — strategy returned `[]`-equivalent intent.
    Noop,
    /// CTX-05: ABI-driven contract call.
    ContractCall(ContractCallAction),
    /// CTX-06: pre-encoded raw calldata call.
    RawCall(RawCallAction),
    /// CTX-07a: ERC20 `transfer(to, amount)`.
    Erc20Transfer(Erc20TransferAction),
    /// CTX-07b: ERC20 `approve(spender, amount)`.
    Erc20Approve(Erc20ApproveAction),
    /// CTX-08: native-coin transfer.
    NativeTransfer(NativeTransferAction),
}

/// CTX-05 — ABI-driven contract call.
///
/// `abi` is a JSON-string of the contract ABI (size ≤ [`MAX_ABI_BYTES`]).
/// Phase 5 normalization re-parses it into canonical calldata; Phase 4 only
/// dry-run-encodes during builder validation and discards the bytes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContractCallAction {
    pub address: String,
    pub abi: String,
    pub function: String,
    pub args: Vec<serde_json::Value>,
    /// Native-coin attached to the call (decimal-string wei). Defaults to
    /// `"0"`. Phase 5 owns transaction-level fields beyond this.
    #[serde(default = "default_zero_value")]
    pub value: String,
}

/// CTX-06 — explicit pre-encoded calldata call.
///
/// `data` is `0x`-prefixed even-length hex. The validator checks shape only;
/// Phase 5 re-validates against the resolved selector before broadcast.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RawCallAction {
    pub address: String,
    pub data: String,
    #[serde(default = "default_zero_value")]
    pub value: String,
}

/// CTX-07a — ERC20 `transfer(to, amount)`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Erc20TransferAction {
    pub token: String,
    pub to: String,
    pub amount: String,
}

/// CTX-07b — ERC20 `approve(spender, amount)`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Erc20ApproveAction {
    pub token: String,
    pub spender: String,
    pub amount: String,
}

/// CTX-08 — native-coin transfer to `to`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NativeTransferAction {
    pub to: String,
    pub value: String,
}

impl Action {
    /// Phase-4 emission gate (mirrors `RunStatus::phase2_emittable` /
    /// `JournalActionOutcome::phase3_emittable`). All five Phase-4 variants
    /// emit; Phase-5+ variants (e.g. `MultiCall`, `Bridge`) MUST extend this
    /// gate to opt in.
    pub fn phase4_emittable(&self) -> bool {
        matches!(
            self,
            Self::Noop
                | Self::ContractCall(_)
                | Self::RawCall(_)
                | Self::Erc20Transfer(_)
                | Self::Erc20Approve(_)
                | Self::NativeTransfer(_)
        )
    }

    /// Stable wire `kind` discriminator string.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::ContractCall(_) => "contract_call",
            Self::RawCall(_) => "raw_call",
            Self::Erc20Transfer(_) => "erc20_transfer",
            Self::Erc20Approve(_) => "erc20_approve",
            Self::NativeTransfer(_) => "native_transfer",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn addr() -> &'static str {
        "0x0000000000000000000000000000000000000001"
    }

    #[test]
    fn action_enum_has_six_variants() {
        // Noop
        let a: Action = serde_json::from_value(json!({"kind":"noop"})).unwrap();
        assert!(matches!(a, Action::Noop));

        // ContractCall
        let a: Action = serde_json::from_value(json!({
            "kind":"contract_call",
            "address": addr(),
            "abi":"[]",
            "function":"f",
            "args":[]
        }))
        .unwrap();
        assert!(matches!(a, Action::ContractCall(_)));

        // RawCall
        let a: Action = serde_json::from_value(json!({
            "kind":"raw_call",
            "address": addr(),
            "data":"0xdeadbeef"
        }))
        .unwrap();
        assert!(matches!(a, Action::RawCall(_)));

        // Erc20Transfer
        let a: Action = serde_json::from_value(json!({
            "kind":"erc20_transfer",
            "token": addr(),
            "to": addr(),
            "amount":"1000"
        }))
        .unwrap();
        assert!(matches!(a, Action::Erc20Transfer(_)));

        // Erc20Approve
        let a: Action = serde_json::from_value(json!({
            "kind":"erc20_approve",
            "token": addr(),
            "spender": addr(),
            "amount":"0"
        }))
        .unwrap();
        assert!(matches!(a, Action::Erc20Approve(_)));

        // NativeTransfer
        let a: Action = serde_json::from_value(json!({
            "kind":"native_transfer",
            "to": addr(),
            "value":"1000000000000000000"
        }))
        .unwrap();
        assert!(matches!(a, Action::NativeTransfer(_)));
    }

    #[test]
    fn action_enum_rejects_unknown_kind() {
        let r: Result<Action, _> = serde_json::from_value(json!({
            "kind":"multi_call",
            "items": []
        }));
        assert!(r.is_err());
    }

    #[test]
    fn contract_call_deny_unknown_fields() {
        let r: Result<Action, _> = serde_json::from_value(json!({
            "kind":"contract_call",
            "address": addr(),
            "abi":"[]","function":"f","args":[],
            "extra": 1
        }));
        let err = r.unwrap_err().to_string();
        assert!(err.contains("unknown field"), "got: {err}");
    }

    #[test]
    fn raw_call_deny_unknown_fields() {
        let r: Result<Action, _> = serde_json::from_value(json!({
            "kind":"raw_call",
            "address": addr(),
            "data":"0x",
            "gas": 21000
        }));
        assert!(r.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn erc20_transfer_deny_unknown_fields() {
        let r: Result<Action, _> = serde_json::from_value(json!({
            "kind":"erc20_transfer",
            "token": addr(),
            "to": addr(),
            "amount":"1",
            "memo":"hi"
        }));
        assert!(r.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn contract_call_value_default_is_zero() {
        let a: Action = serde_json::from_value(json!({
            "kind":"contract_call",
            "address": addr(),
            "abi":"[]","function":"f","args":[]
        }))
        .unwrap();
        if let Action::ContractCall(cc) = a {
            assert_eq!(cc.value, "0");
        } else {
            panic!("expected ContractCall");
        }
    }

    #[test]
    fn raw_call_value_default_is_zero() {
        let a: Action = serde_json::from_value(json!({
            "kind":"raw_call",
            "address": addr(),
            "data":"0x"
        }))
        .unwrap();
        if let Action::RawCall(rc) = a {
            assert_eq!(rc.value, "0");
        } else {
            panic!("expected RawCall");
        }
    }

    #[test]
    fn phase4_emittable_returns_true_for_new_variants() {
        let cases = [
            Action::Noop,
            Action::ContractCall(ContractCallAction {
                address: addr().into(),
                abi: "[]".into(),
                function: "f".into(),
                args: vec![],
                value: "0".into(),
            }),
            Action::RawCall(RawCallAction {
                address: addr().into(),
                data: "0x".into(),
                value: "0".into(),
            }),
            Action::Erc20Transfer(Erc20TransferAction {
                token: addr().into(),
                to: addr().into(),
                amount: "1".into(),
            }),
            Action::Erc20Approve(Erc20ApproveAction {
                token: addr().into(),
                spender: addr().into(),
                amount: "0".into(),
            }),
            Action::NativeTransfer(NativeTransferAction {
                to: addr().into(),
                value: "1".into(),
            }),
        ];
        for a in &cases {
            assert!(a.phase4_emittable(), "variant should be emittable: {:?}", a);
        }
    }

    #[test]
    fn max_abi_bytes_constant_is_64_kib() {
        assert_eq!(MAX_ABI_BYTES, 65_536);
    }

    #[test]
    fn kind_string_matches_serde_tag() {
        let a = Action::ContractCall(ContractCallAction {
            address: addr().into(),
            abi: "[]".into(),
            function: "f".into(),
            args: vec![],
            value: "0".into(),
        });
        assert_eq!(a.kind(), "contract_call");
        assert_eq!(Action::Noop.kind(), "noop");
    }
}
