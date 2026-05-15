//! v1.12 honesty envelope: shared `{ data, confidence, reason?, remediation?, staleness? }`
//! shape used by `strategy://{id}/view` (and friends).
//!
//! Track B3 — types only; the wiring lives in `executor-mcp::resources`
//! (read_strategy_view) and the cache lives in `executor-state` as
//! `strategy_view_cache`.
//!
//! ## Why a dedicated `stale` variant?
//!
//! Before v1.12 the `view` failure path returned
//! `{ data: null, confidence: "partial", reason: "…" }`. The dashboard then
//! rendered an empty balance card and users mistook a transient view error
//! for asset loss. v1.12 adds a last-known-good cache: on view failure, if
//! the cache has a prior successful body we return THAT body with
//! `confidence: "stale"` plus a `staleness` block carrying the previous
//! success timestamp + the current failure reason. The original
//! `confidence: "partial"` path stays — it now only fires when we genuinely
//! have nothing to show (cache empty + view failed).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// String constant of the `stale` variant — kept stable so external
/// consumers (UI, tests, MCP clients) can string-compare without parsing
/// the enum. The matching `serde` rename below MUST not drift.
pub const CONFIDENCE_STALE: &str = "stale";

/// All confidence variants the `view` envelope can carry.
///
/// - `Full` — view ran cleanly; `data` is fresh.
/// - `Partial` — view failed AND there is no cached success to fall back to.
///   `data` is null; agents should consult `reason` + `remediation`.
/// - `Stale` — view failed BUT a prior successful body is available; `data`
///   is the cached payload, `staleness` carries succeeded_at + age + the
///   current failure reason.
/// - `Missing` — strategy registered without a `view` source. `data` is null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Full,
    Partial,
    Stale,
    Missing,
}

/// Cheap string-side validator for boundary code that wants to check a
/// caller-supplied confidence label without committing to enum decoding.
/// Returns `true` for any of the four canonical lowercase strings.
pub fn valid_confidence(s: &str) -> bool {
    matches!(s, "full" | "partial" | "stale" | "missing")
}

/// Staleness metadata attached to a `confidence: "stale"` envelope.
///
/// `succeeded_at` is the RFC3339 timestamp when the cache row was written
/// (i.e. the last successful view evaluation); `age_seconds` is the
/// integer-seconds delta from `succeeded_at` to "now" at serialization
/// time, saturating to 0 on backward clock skew. `current_error` carries
/// the *current* view failure reason so agents can act on it directly
/// without having to fetch the strategy and re-run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Staleness {
    pub succeeded_at: String,
    pub age_seconds: u64,
    pub current_error: String,
}

/// Canonical wire shape of the v1.4+ honesty envelope. The runtime
/// currently serializes ad-hoc `serde_json::json!` objects with the same
/// shape; this struct exists as the spec / schema generator anchor.
/// `data` is whatever the strategy view returned (or null on
/// partial/missing); `staleness` is only present on the `stale` variant.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HonestyEnvelope<T> {
    pub data: T,
    pub confidence: Confidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staleness: Option<Staleness>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Confidence::Full).unwrap(),
            "\"full\""
        );
        assert_eq!(
            serde_json::to_string(&Confidence::Partial).unwrap(),
            "\"partial\""
        );
        assert_eq!(
            serde_json::to_string(&Confidence::Stale).unwrap(),
            "\"stale\""
        );
        assert_eq!(
            serde_json::to_string(&Confidence::Missing).unwrap(),
            "\"missing\""
        );
    }

    #[test]
    fn confidence_stale_constant_matches_serde() {
        let s = serde_json::to_string(&Confidence::Stale).unwrap();
        // Strip the JSON quotes for the bare-string compare.
        assert_eq!(s.trim_matches('"'), CONFIDENCE_STALE);
    }

    #[test]
    fn valid_confidence_matches_all_variants() {
        assert!(valid_confidence("full"));
        assert!(valid_confidence("partial"));
        assert!(valid_confidence("stale"));
        assert!(valid_confidence("missing"));
        assert!(!valid_confidence("Full"));
        assert!(!valid_confidence(""));
        assert!(!valid_confidence("unknown"));
    }
}
