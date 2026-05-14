//! v1.5 Track 1B — regex extractor unit tests.
//!
//! Six cases drawn from the plan §3 acceptance list. Each case asserts the
//! exact (address, selectors) pairs the extractor recovers from the source —
//! addresses are lowercased, selectors are the JS function name string.

use executor_mcp::contracts_touched::{extract, ExtractionStatus};

// Real-world addresses from `examples/strategies/eth-funnel-bundle.js`. Kept
// in mixed case in the source on purpose so we can prove normalisation.
const AAVE_RAW: &str = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5";
const AAVE_LC: &str = "0xa238dd80c259a72e81d7e4664a9801593f98d1c5";
const USDC_RAW: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
const USDC_LC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
const ROUTER_LC: &str = "0x2626664c2603336e57b271c5c0b26f421741e481";

// ─── Case 1 — Pattern A literal address + literal function name ────────────

#[test]
fn case1_pattern_a_literal_address_and_function() {
    let source = format!(
        r#"
        const execute = (ctx) => {{
          return [
            ctx.actions.contractCall({{
              address: "{AAVE_RAW}",
              function: "supply",
              args: [],
            }})
          ];
        }};
    "#
    );
    let r = extract(&source);
    assert_eq!(r.extraction_status, ExtractionStatus::Complete);
    let selectors = r
        .contracts
        .get(AAVE_LC)
        .unwrap_or_else(|| panic!("expected aave entry; got {:?}", r.contracts));
    assert!(selectors.contains("supply"));
    assert!(r.warnings.is_empty(), "no warnings; got {:?}", r.warnings);
}

// ─── Case 2 — Pattern B `const NAME = "0x..."` resolved through identifier ──

#[test]
fn case2_pattern_b_const_resolved_identifier() {
    let source = format!(
        r#"
        const AAVE = "{AAVE_RAW}";
        const execute = (ctx) => [
          ctx.actions.contractCall({{
            address: AAVE,
            function: "supply",
            args: [],
          }})
        ];
    "#
    );
    let r = extract(&source);
    assert_eq!(
        r.extraction_status,
        ExtractionStatus::Complete,
        "should resolve const → Complete; warnings={:?}",
        r.warnings
    );
    let selectors = r
        .contracts
        .get(AAVE_LC)
        .unwrap_or_else(|| panic!("expected aave; got {:?}", r.contracts));
    assert!(selectors.contains("supply"));
}

// ─── Case 3 — Pattern C `erc20Approve` token → `approve` ───────────────────

#[test]
fn case3_pattern_c_erc20_approve() {
    let source = format!(
        r#"
        const USDC = "{USDC_RAW}";
        ctx.actions.erc20Approve({{ token: USDC, spender: "0x0", amount: "0" }})
    "#
    );
    let r = extract(&source);
    assert_eq!(r.extraction_status, ExtractionStatus::Complete);
    let selectors = r
        .contracts
        .get(USDC_LC)
        .unwrap_or_else(|| panic!("expected usdc; got {:?}", r.contracts));
    assert!(
        selectors.contains("approve"),
        "erc20Approve maps token → 'approve'; got {selectors:?}"
    );
}

// ─── Case 4 — Multi-pattern: examples/strategies/eth-funnel-bundle.js ─────

#[test]
fn case4_eth_funnel_bundle_extracts_three_pairs() {
    // Read the bundled example so we exercise the extractor against real
    // shipped JS rather than a synthetic snippet.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/strategies/eth-funnel-bundle.js");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let r = extract(&source);
    assert_eq!(
        r.extraction_status,
        ExtractionStatus::Complete,
        "eth-funnel-bundle.js should extract fully; warnings={:?}",
        r.warnings
    );

    // Aave V3 Pool .supply
    let aave = r
        .contracts
        .get(AAVE_LC)
        .unwrap_or_else(|| panic!("expected aave; map={:?}", r.contracts));
    assert!(
        aave.contains("supply"),
        "aave should have 'supply' selector; got {aave:?}"
    );

    // Uniswap V3 SwapRouter02 .exactInputSingle
    let router = r
        .contracts
        .get(ROUTER_LC)
        .unwrap_or_else(|| panic!("expected router; map={:?}", r.contracts));
    assert!(
        router.contains("exactInputSingle"),
        "router should have 'exactInputSingle'; got {router:?}"
    );

    // USDC approve (erc20Approve)
    let usdc = r
        .contracts
        .get(USDC_LC)
        .unwrap_or_else(|| panic!("expected usdc; map={:?}", r.contracts));
    assert!(
        usdc.contains("approve"),
        "usdc should have 'approve'; got {usdc:?}"
    );
}

// ─── Case 5 — Dynamic dispatch detected → Incomplete + warning ─────────────

#[test]
fn case5_dynamic_dispatch_marks_incomplete() {
    let source = r#"
        const chooseRouter = (intent) => "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5";
        const execute = (ctx) => [
          ctx.actions.contractCall({
            address: chooseRouter(),
            function: "swap",
            args: [],
          })
        ];
    "#;
    let r = extract(source);
    assert_eq!(
        r.extraction_status,
        ExtractionStatus::Incomplete,
        "dynamic dispatch must mark Incomplete; warnings={:?}",
        r.warnings
    );
    assert!(
        !r.warnings.is_empty(),
        "dynamic dispatch must record a warning"
    );
    // The router call itself shouldn't appear because the address didn't resolve.
    assert!(
        !r.contracts
            .iter()
            .any(|(_, sel)| sel.contains("swap")),
        "unresolved dynamic call must not surface in contracts map; got {:?}",
        r.contracts
    );
}

// ─── Case 6 — Malformed source: garbage address → skip + warn, no crash ───

#[test]
fn case6_malformed_source_skips_with_warning() {
    let source = r#"
        ctx.actions.contractCall({
          address: "not-an-address",
          function: "supply",
          args: [],
        })
        ctx.actions.contractCall({
          address: "0xZZZZ",
          function: "supply",
          args: [],
        })
    "#;
    // Must not panic, must not crash, must produce empty extraction.
    let r = extract(source);
    assert!(
        r.contracts.is_empty(),
        "malformed addresses should NOT appear in the map; got {:?}",
        r.contracts
    );
    assert!(
        !r.warnings.is_empty(),
        "expected at least one warning for malformed address"
    );
    // We didn't extract anything but at least one call site was unresolved,
    // so status is Incomplete (we know we missed at least one site).
    assert_eq!(
        r.extraction_status,
        ExtractionStatus::Incomplete,
        "unresolved call sites must mark extraction Incomplete"
    );
}
