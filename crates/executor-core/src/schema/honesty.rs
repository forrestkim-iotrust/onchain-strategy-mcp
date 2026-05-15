//! v1.4 honesty contract — the wire envelope that wraps every
//! `strategy://{id}/view` (and portfolio-aggregate) response so agents can
//! tell "this is real" apart from "we couldn't read it, here's why."
//!
//! ## Wire shape
//!
//! ```jsonc
//! {
//!   "data": <any>,
//!   "confidence": "full" | "partial" | "missing" | "stale",
//!   "reason": "<string|null>",
//!   "remediation": "<string|null>",
//!   "staleness": { ... }   // ONLY when confidence == "stale"
//! }
//! ```
//!
//! ## Confidence values
//!
//! | value     | meaning                                                       |
//! |-----------|---------------------------------------------------------------|
//! | `full`    | View ran cleanly; `data` is the strategy's actual return.     |
//! | `partial` | View ran but some helpers failed; `data` is best-effort.      |
//! | `missing` | View is unimplemented or wholly unavailable; `data` is `"—"`. |
//! | `stale`   | View is currently failing BUT we have a prior good cache;    |
//! |           | `data` is the last successful body and `staleness` is set.    |
//!
//! `stale` is v1.12 (Track B3): added to keep failed-view dashboards from
//! looking like a scary empty balance. The agent / UI is expected to render
//! `stale` with a "last known good" affordance instead of treating the body
//! as fresh.
//!
//! ## Staleness envelope
//!
//! `staleness` is populated ONLY when `confidence == "stale"`. It carries the
//! freshness of the cached body plus the *current* error so the agent can
//! distinguish "this is the historical last-good" from "this is why we're
//! not refreshing." Skipped on the wire when `None`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Confidence value attached to every honesty-envelope response.
///
/// Variants are listed in declaration order with the v1.4 trio first and the
/// v1.12 addition (`Stale`) last so existing wire goldens stay byte-stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// View executed cleanly; `data` is the strategy's actual return.
    Full,
    /// View executed but some helpers degraded; `data` is best-effort.
    Partial,
    /// View is unimplemented or wholly unavailable; `data` is `"—"`.
    Missing,
    /// v1.12: view is failing right now but a prior successful body is cached.
    /// `data` is the last successful body and `staleness` MUST be populated.
    Stale,
}

/// Convenience string constants for callers that build envelopes as inline
/// JSON instead of via the typed enum.
pub const CONFIDENCE_FULL: &str = "full";
pub const CONFIDENCE_PARTIAL: &str = "partial";
pub const CONFIDENCE_MISSING: &str = "missing";
/// v1.12: stale fallback — view failing now, prior good cache being served.
pub const CONFIDENCE_STALE: &str = "stale";

/// Returns true iff `s` is one of the four wire-valid confidence values
/// (`full`, `partial`, `missing`, `stale`). String-typed callers should use
/// this helper instead of comparing against ad-hoc literals.
pub fn valid_confidence(s: &str) -> bool {
    matches!(s, CONFIDENCE_FULL | CONFIDENCE_PARTIAL | CONFIDENCE_MISSING | CONFIDENCE_STALE)
}

/// v1.12: freshness envelope attached to a `Stale` honesty response. Carries
/// when the cached body was captured, how long ago that was, and the *current*
/// error that prevented a fresh read. Absent on full / partial / missing
/// responses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "v1.12: freshness metadata attached to a `confidence: \"stale\"` \
honesty response. Carries when the cached body was captured, how long ago \
that was, and the current error that prevented a fresh read.")]
pub struct Staleness {
    /// RFC3339 timestamp of the last successful view evaluation whose body is
    /// being served as the stale fallback.
    #[schemars(description = "RFC3339 timestamp of the last successful view evaluation.")]
    pub succeeded_at: String,
    /// Seconds since `succeeded_at`, computed at response time so clients
    /// don't have to parse the timestamp themselves.
    #[schemars(description = "Seconds since `succeeded_at`, computed at response time.")]
    pub age_seconds: u64,
    /// The error from the *current* (failing) view attempt. Same shape as
    /// the envelope's `reason` field but scoped to this attempt, so the
    /// agent can distinguish "this is the historical last-good" from "this
    /// is why we're not refreshing."
    #[schemars(description = "Error from the current (failing) view attempt — distinct from \
the envelope `reason`, which describes WHY we're serving stale data.")]
    pub current_error: String,
}

/// v1.4 honesty envelope. Wraps every view / portfolio response with a
/// confidence value plus optional human-readable explanation. v1.12 adds
/// the optional `staleness` field for the new `Stale` variant.
///
/// Valid `confidence` values: `full`, `partial`, `missing`, `stale`.
/// `staleness` is populated ONLY when `confidence == "stale"` and is skipped
/// on the wire otherwise.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(description = "v1.4 honesty envelope wrapping view / portfolio responses. \
Confidence ∈ full | partial | missing | stale. `staleness` is populated ONLY when \
confidence == \"stale\" (v1.12).")]
pub struct HonestyEnvelope {
    /// The strategy's return value (or a fallback when degraded). Loosely
    /// typed because each strategy's view shape is its own contract.
    pub data: serde_json::Value,
    /// One of `full`, `partial`, `missing`, `stale`.
    pub confidence: Confidence,
    /// Human-readable reason for any non-`full` confidence. `null` on `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Human-readable remediation hint — what the agent should try next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    /// v1.12: freshness metadata. Populated ONLY when `confidence ==
    /// Confidence::Stale`; omitted from the wire otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staleness: Option<Staleness>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn confidence_full_serializes_as_full() {
        assert_eq!(serde_json::to_value(Confidence::Full).unwrap(), json!("full"));
    }

    #[test]
    fn confidence_partial_serializes_as_partial() {
        assert_eq!(
            serde_json::to_value(Confidence::Partial).unwrap(),
            json!("partial")
        );
    }

    #[test]
    fn confidence_missing_serializes_as_missing() {
        assert_eq!(
            serde_json::to_value(Confidence::Missing).unwrap(),
            json!("missing")
        );
    }

    #[test]
    fn confidence_stale_serializes_as_stale() {
        assert_eq!(
            serde_json::to_value(Confidence::Stale).unwrap(),
            json!("stale")
        );
    }

    #[test]
    fn confidence_all_four_roundtrip() {
        for c in [
            Confidence::Full,
            Confidence::Partial,
            Confidence::Missing,
            Confidence::Stale,
        ] {
            let v = serde_json::to_value(c).expect("serialize");
            let back: Confidence = serde_json::from_value(v.clone()).expect("deserialize");
            assert_eq!(back, c, "roundtrip failed for {c:?} -> {v}");
        }
    }

    #[test]
    fn confidence_unknown_value_rejected() {
        let err = serde_json::from_value::<Confidence>(json!("definitely-not-a-confidence"));
        assert!(
            err.is_err(),
            "expected unknown confidence to fail deserialization, got {err:?}"
        );
    }

    #[test]
    fn valid_confidence_accepts_four_known_values() {
        assert!(valid_confidence(CONFIDENCE_FULL));
        assert!(valid_confidence(CONFIDENCE_PARTIAL));
        assert!(valid_confidence(CONFIDENCE_MISSING));
        assert!(valid_confidence(CONFIDENCE_STALE));
        // string-literal parity with the const
        assert!(valid_confidence("full"));
        assert!(valid_confidence("partial"));
        assert!(valid_confidence("missing"));
        assert!(valid_confidence("stale"));
    }

    #[test]
    fn valid_confidence_rejects_unknown() {
        assert!(!valid_confidence(""));
        assert!(!valid_confidence("FULL")); // case-sensitive
        assert!(!valid_confidence("ok"));
        assert!(!valid_confidence("error"));
    }

    #[test]
    fn envelope_skips_staleness_when_none() {
        let env = HonestyEnvelope {
            data: json!({"balance": "1.23"}),
            confidence: Confidence::Full,
            reason: None,
            remediation: None,
            staleness: None,
        };
        let v = serde_json::to_value(&env).expect("serialize");
        let obj = v.as_object().expect("object");
        assert!(
            !obj.contains_key("staleness"),
            "staleness must be omitted when None; got {v}"
        );
        // confidence still rides the wire as the lowercase string
        assert_eq!(obj.get("confidence"), Some(&json!("full")));
    }

    #[test]
    fn envelope_emits_staleness_when_some() {
        let env = HonestyEnvelope {
            data: json!({"balance": "1.23"}),
            confidence: Confidence::Stale,
            reason: Some("showing last successful values from 2026-05-15T10:32:00Z".into()),
            remediation: Some(
                "the strategy's view function is currently failing — see `staleness.current_error`"
                    .into(),
            ),
            staleness: Some(Staleness {
                succeeded_at: "2026-05-15T10:32:00Z".into(),
                age_seconds: 612,
                current_error: "view function failed: evm revert: unknown".into(),
            }),
        };
        let v = serde_json::to_value(&env).expect("serialize");
        let obj = v.as_object().expect("object");
        assert_eq!(obj.get("confidence"), Some(&json!("stale")));
        let st = obj.get("staleness").expect("staleness present");
        assert_eq!(st.get("succeeded_at"), Some(&json!("2026-05-15T10:32:00Z")));
        assert_eq!(st.get("age_seconds"), Some(&json!(612)));
        assert_eq!(
            st.get("current_error"),
            Some(&json!("view function failed: evm revert: unknown"))
        );
    }

    #[test]
    fn envelope_roundtrips_with_staleness() {
        let original = HonestyEnvelope {
            data: json!([1, 2, 3]),
            confidence: Confidence::Stale,
            reason: Some("r".into()),
            remediation: Some("rem".into()),
            staleness: Some(Staleness {
                succeeded_at: "2026-01-01T00:00:00Z".into(),
                age_seconds: 99,
                current_error: "err".into(),
            }),
        };
        let v = serde_json::to_value(&original).expect("serialize");
        let back: HonestyEnvelope = serde_json::from_value(v).expect("deserialize");
        assert_eq!(back.confidence, Confidence::Stale);
        let st = back.staleness.expect("staleness present");
        assert_eq!(st.succeeded_at, "2026-01-01T00:00:00Z");
        assert_eq!(st.age_seconds, 99);
        assert_eq!(st.current_error, "err");
    }

    #[test]
    fn envelope_accepts_legacy_three_variant_payload() {
        // Existing wire payloads (pre-v1.12) lack `staleness`. They must
        // continue to deserialize cleanly.
        let raw: Value = json!({
            "data": "—",
            "confidence": "partial",
            "reason": "view function failed: evm revert: unknown",
            "remediation": "inspect `strategy://{id}` for the view source and try `evm_view` with a minimal repro",
        });
        let env: HonestyEnvelope = serde_json::from_value(raw).expect("deserialize legacy");
        assert_eq!(env.confidence, Confidence::Partial);
        assert!(env.staleness.is_none());
    }
}
