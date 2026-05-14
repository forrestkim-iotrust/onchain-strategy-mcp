//! v1.5 Track 1C — `policy_alignment` surface integration tests.
//!
//! Acceptance gate from `.planning/v1.5-ALIGNMENT-PLAN.md` §6:
//!
//!   1. `strategy_register` a strategy referencing an unknown contract →
//!      response has `policy_alignment: { verdict: "missing", remediation: ... }`.
//!   2. `policy_set` adding that contract → response has
//!      `impact.newly_satisfied_strategies` listing the strategy.
//!   3. Re-read `strategy://{id}` → `policy_alignment.verdict == "satisfied"`.
//!   4. `policy_set` narrowing the allowlist → response has
//!      `impact.newly_unsatisfied_strategies`.
//!   5. `strategy://list?summary=true` carries a one-line verdict per entry.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{
    call_tool, extract_json_result, extract_resource_json, initialize, read_resource,
    spawn_server_with_state,
};

const AAVE: &str = "0xaaaa000000000000000000000000000000000001";
const COMP: &str = "0xbbbb000000000000000000000000000000000002";

/// Minimal one-action strategy hitting a known contract. The regex
/// extractor picks up address + function name; chain ids are not extracted
/// (chain context is per-action runtime data), so policy alignment matches
/// against any chain that allow-lists the address.
fn strategy_referencing(contract: &str, function: &str) -> String {
    format!(
        "(ctx) => [ctx.actions.contractCall({{ chain: 31337, address: \"{contract}\", function: \"{function}\", args: [] }})]",
    )
}

fn policy_with_contracts(chain: u64, contracts: &[&str], with_any_selectors: bool) -> Value {
    let mut contracts_obj = serde_json::Map::new();
    contracts_obj.insert(
        chain.to_string(),
        json!({ "allow": contracts }),
    );
    let mut selectors_obj = serde_json::Map::new();
    if with_any_selectors {
        for c in contracts {
            selectors_obj.insert(
                format!("{chain}:{c}"),
                json!({ "allow": ["any"] }),
            );
        }
    }
    json!({
        "chains": { "allow": [chain] },
        "contracts": contracts_obj,
        "selectors": selectors_obj,
        "native_value": {},
        "erc20_spend": {},
        "raw_call": { "allow_global": false, "allow": [] },
    })
}

#[tokio::test]
async fn register_with_unknown_contract_yields_missing_verdict() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Install a policy that allows AAVE only.
    let policy = policy_with_contracts(31337, &[AAVE], true);
    let _ = call_tool(&mut proc, 10, "policy_set", json!({ "policy": policy })).await?;

    // Register a strategy that touches COMP — which is NOT in the policy.
    let r = call_tool(
        &mut proc,
        11,
        "strategy_register",
        json!({
            "name": "unknown-comp",
            "source": strategy_referencing(COMP, "supply"),
        }),
    )
    .await?;
    let resp = extract_json_result(&r);
    let alignment = &resp["policy_alignment"];
    assert_eq!(
        alignment["verdict"].as_str(),
        Some("missing"),
        "expected missing verdict, got: {alignment}",
    );
    assert!(
        alignment["missing"].as_array().is_some_and(|a| !a.is_empty()),
        "missing entries should list the unknown contract: {alignment}",
    );
    assert!(
        alignment["remediation"].as_str().is_some(),
        "remediation should be present for missing verdict: {alignment}",
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_set_widening_lists_newly_satisfied_strategy() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Initial policy allows AAVE only.
    let policy_a = policy_with_contracts(31337, &[AAVE], true);
    let _ = call_tool(&mut proc, 20, "policy_set", json!({ "policy": policy_a })).await?;

    // Register a strategy that touches COMP — starts unaligned.
    let reg = call_tool(
        &mut proc,
        21,
        "strategy_register",
        json!({
            "name": "comp-supply",
            "source": strategy_referencing(COMP, "supply"),
        }),
    )
    .await?;
    let reg_body = extract_json_result(&reg);
    let strategy_id = reg_body["strategy_id"].as_str().unwrap().to_string();
    assert_eq!(reg_body["policy_alignment"]["verdict"], json!("missing"));

    // Widen policy to allow both AAVE and COMP.
    let policy_b = policy_with_contracts(31337, &[AAVE, COMP], true);
    let r = call_tool(
        &mut proc,
        22,
        "policy_set",
        json!({ "policy": policy_b, "rationale": "add comp" }),
    )
    .await?;
    let body = extract_json_result(&r);
    let newly = body["impact"]["newly_satisfied_strategies"]
        .as_array()
        .expect("array");
    assert!(
        newly.iter().any(|s| s["id"].as_str() == Some(strategy_id.as_str())),
        "comp-supply strategy must appear in newly_satisfied_strategies: {newly:?}",
    );

    // Re-read strategy://{id} → now satisfied.
    let r2 = read_resource(&mut proc, 23, &format!("strategy://{strategy_id}")).await?;
    let body2 = extract_resource_json(&r2);
    assert_eq!(
        body2["policy_alignment"]["verdict"].as_str(),
        Some("satisfied"),
        "after widening, strategy://{strategy_id} must report satisfied: {body2}",
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn policy_set_narrowing_lists_newly_unsatisfied_strategy() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // Wide policy.
    let policy_a = policy_with_contracts(31337, &[AAVE, COMP], true);
    let _ = call_tool(&mut proc, 30, "policy_set", json!({ "policy": policy_a })).await?;

    let reg = call_tool(
        &mut proc,
        31,
        "strategy_register",
        json!({
            "name": "comp-supply-narrowing",
            "source": strategy_referencing(COMP, "supply"),
        }),
    )
    .await?;
    let reg_body = extract_json_result(&reg);
    let strategy_id = reg_body["strategy_id"].as_str().unwrap().to_string();
    assert_eq!(reg_body["policy_alignment"]["verdict"], json!("satisfied"));

    // Narrow policy: drop COMP.
    let policy_b = policy_with_contracts(31337, &[AAVE], true);
    let r = call_tool(
        &mut proc,
        32,
        "policy_set",
        json!({ "policy": policy_b, "rationale": "revoke comp" }),
    )
    .await?;
    let body = extract_json_result(&r);
    let lost = body["impact"]["newly_unsatisfied_strategies"]
        .as_array()
        .expect("array");
    assert!(
        lost.iter().any(|s| s["id"].as_str() == Some(strategy_id.as_str())),
        "narrowing must list comp-supply in newly_unsatisfied_strategies: {lost:?}",
    );

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn strategy_list_summary_includes_alignment_string() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    let policy = policy_with_contracts(31337, &[AAVE], true);
    let _ = call_tool(&mut proc, 40, "policy_set", json!({ "policy": policy })).await?;

    let _ = call_tool(
        &mut proc,
        41,
        "strategy_register",
        json!({
            "name": "aave-supply-list",
            "source": strategy_referencing(AAVE, "supply"),
        }),
    )
    .await?;
    let _ = call_tool(
        &mut proc,
        42,
        "strategy_register",
        json!({
            "name": "comp-supply-list",
            "source": strategy_referencing(COMP, "supply"),
        }),
    )
    .await?;

    let r = read_resource(&mut proc, 43, "strategy://list?summary=true").await?;
    let body = extract_resource_json(&r);
    let strategies = body["strategies"].as_array().expect("strategies array");
    assert_eq!(strategies.len(), 2);
    for s in strategies {
        let verdict = s["policy_alignment"].as_str().unwrap_or("");
        assert!(
            matches!(verdict, "satisfied" | "partial" | "missing" | "incomplete"),
            "each summary entry must carry a one-line policy_alignment string; got: {s}",
        );
    }
    // The aave row should be satisfied; comp row should be missing.
    let aave_row = strategies
        .iter()
        .find(|s| s["name"] == "aave-supply-list")
        .expect("aave row present");
    let comp_row = strategies
        .iter()
        .find(|s| s["name"] == "comp-supply-list")
        .expect("comp row present");
    assert_eq!(aave_row["policy_alignment"], json!("satisfied"));
    assert_eq!(comp_row["policy_alignment"], json!("missing"));

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn alignment_is_incomplete_when_no_policy_is_loaded() -> Result<()> {
    let mut proc = spawn_server_with_state(":memory:").await?;
    let _ = initialize(&mut proc).await?;

    // No policy_set called — DB has no active revision.
    let r = call_tool(
        &mut proc,
        50,
        "strategy_register",
        json!({
            "name": "no-policy-yet",
            "source": strategy_referencing(AAVE, "supply"),
        }),
    )
    .await?;
    let resp = extract_json_result(&r);
    assert_eq!(
        resp["policy_alignment"]["verdict"].as_str(),
        Some("incomplete"),
        "no active policy must surface as incomplete: {resp}",
    );
    let remediation = resp["policy_alignment"]["remediation"]
        .as_str()
        .unwrap_or("");
    assert!(
        remediation.contains("policy_set") || remediation.contains("no active policy"),
        "remediation should reference policy_set or no active policy; got: {remediation}",
    );

    proc.child.kill().await?;
    Ok(())
}
