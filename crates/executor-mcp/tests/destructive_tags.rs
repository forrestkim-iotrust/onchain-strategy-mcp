//! v1.4 Track G + v1.11 Track D: destructive-op tagging.
//!
//! Two channels, both checked:
//!
//! 1. Legacy human-readable: `[DESTRUCTIVE]` prefix in the tool description
//!    (v1.4 Track G / v1.5 Track 1A). Backwards-compatible.
//!
//! 2. v1.11 structural: `_meta.osmcp.mutation` field on each `Tool` in
//!    `tools/list`, carrying either `"destructive"` or `"safe-side-effects"`.
//!    This is the consent-flow contract — clients gate user prompts on
//!    `_meta.osmcp.mutation == "destructive"` and may auto-allow
//!    `"safe-side-effects"` once policy / dry-run hatches have been used.
//!
//! See `crates/executor-mcp/src/tools.rs` "MUTATION TAG CHANNEL" header
//! and the `INSTRUCTIONS` constant in `crates/executor-mcp/src/server.rs`.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server};

const MARKER: &str = "[DESTRUCTIVE]";

// v1.5 Track 1A: `policy_set` joins the destructive set — it replaces the
// active policy revision, which can revoke capabilities and break
// strategies in flight. The diff/impact response is the consent surface.
const DESTRUCTIVE_TOOLS: &[&str] = &[
    "strategy_run",
    "strategy_delete",
    "trigger_delete",
    "policy_set",
];

/// Representative non-destructive tools. `strategy_register` is the most
/// important one to guard — it mutates state but only after explicit
/// agent-authored input, and v1.4 already exposes `dry_run` as the consent
/// hatch. `strategy_list` and `policy_get` are pure reads.
// v1.4 Track B dropped `strategy_list` and `policy_get` as tools (now
// `strategy://list` and `policy://current` resources). Use the surviving
// non-destructive tools to anchor the negative side of the marker contract.
const NON_DESTRUCTIVE_TOOLS: &[&str] =
    &["strategy_register", "trigger_register", "trigger_set_enabled", "evm_view"];

async fn fetch_tools(proc: &mut common::ServerProc) -> Result<Vec<Value>> {
    send(
        proc,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .await?;
    let r = recv(proc).await?;
    Ok(r["result"]["tools"]
        .as_array()
        .expect("tools array")
        .clone())
}

fn description_of<'a>(tools: &'a [Value], name: &str) -> &'a str {
    tools
        .iter()
        .find(|t| t["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("tool {name} not in tools/list"))
        ["description"]
        .as_str()
        .unwrap_or_else(|| panic!("tool {name} has no description"))
}

#[tokio::test]
async fn destructive_tools_carry_marker_prefix() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    let tools = fetch_tools(&mut proc).await?;
    for name in DESTRUCTIVE_TOOLS {
        let desc = description_of(&tools, name);
        assert!(
            desc.starts_with(MARKER),
            "tool {name} description must start with the {MARKER} marker (P5 consent contract); got: {desc:?}"
        );
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn non_destructive_tools_lack_marker() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    let tools = fetch_tools(&mut proc).await?;
    for name in NON_DESTRUCTIVE_TOOLS {
        let desc = description_of(&tools, name);
        assert!(
            !desc.contains(MARKER),
            "tool {name} description must NOT contain the {MARKER} marker — applying it indiscriminately defeats the consent signal; got: {desc:?}"
        );
    }

    proc.child.kill().await?;
    Ok(())
}

/// Cross-check that every tool in the surface is either explicitly destructive
/// or explicitly non-destructive, so a newly-added mutating tool can't slip
/// past consent gating by being un-flagged. This is a tripwire — when a new
/// tool lands, the author is forced to come back here and decide.
#[tokio::test]
async fn marker_distribution_matches_known_destructive_set() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;

    let tools = fetch_tools(&mut proc).await?;
    let mut tagged: Vec<String> = tools
        .iter()
        .filter_map(|t| {
            let name = t["name"].as_str()?;
            let desc = t["description"].as_str()?;
            if desc.contains(MARKER) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();
    tagged.sort();

    let mut expected: Vec<String> = DESTRUCTIVE_TOOLS.iter().map(|s| (*s).to_string()).collect();
    expected.sort();

    assert_eq!(
        tagged, expected,
        "set of tools carrying {MARKER} drifted from the known destructive set. \
If you added or removed a mutating tool, update DESTRUCTIVE_TOOLS in this test \
AND the `Destructive ops` section in tools.rs / server.rs INSTRUCTIONS."
    );

    proc.child.kill().await?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────
// v1.11 Track D — structural `_meta.osmcp.mutation` channel.
//
// The legacy `[DESTRUCTIVE]` prefix is a string heuristic; v1.11 promotes the
// tag to the MCP-spec blessed `_meta` extension channel so consent-flow
// clients can read it without regex. Every mutating tool MUST carry
// `_meta.osmcp.mutation`; read-only tools MUST NOT.
// ──────────────────────────────────────────────────────────────────────────

/// Every mutating tool and its expected `_meta.osmcp.mutation` value.
///
/// `"destructive"` — irreversible / signs onchain / drops data.
/// `"safe-side-effects"` — server-local state mutation, reversible or
/// idempotent, suitable for auto-allow once dry-run / diff hatches are wired.
const MUTATION_TAG_EXPECTED: &[(&str, &str)] = &[
    ("strategy_run", "destructive"),
    ("strategy_delete", "destructive"),
    ("trigger_delete", "destructive"),
    ("strategy_register", "safe-side-effects"),
    ("policy_set", "safe-side-effects"),
    ("trigger_register", "safe-side-effects"),
    ("trigger_set_enabled", "safe-side-effects"),
];

/// Read-only tools that MUST NOT carry the mutation tag — picking the tag up
/// on a pure observation tool would defeat the consent signal.
const READ_ONLY_TOOLS: &[&str] = &["evm_view", "evm_receipt"];

fn mutation_tag_of<'a>(tools: &'a [Value], name: &str) -> Option<&'a str> {
    let t = tools
        .iter()
        .find(|t| t["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("tool {name} not in tools/list"));
    t.get("_meta")
        .and_then(|m| m.get("osmcp"))
        .and_then(|o| o.get("mutation"))
        .and_then(|v| v.as_str())
}

#[tokio::test]
async fn mutating_tools_carry_meta_osmcp_mutation() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;
    let tools = fetch_tools(&mut proc).await?;

    for (name, expected) in MUTATION_TAG_EXPECTED {
        let got = mutation_tag_of(&tools, name).unwrap_or_else(|| {
            panic!(
                "tool {name} is missing `_meta.osmcp.mutation` (expected {expected:?}). \
v1.11 Track D requires every mutating tool to carry the structural tag."
            )
        });
        assert_eq!(
            got, *expected,
            "tool {name} `_meta.osmcp.mutation` mismatch: want {expected:?}, got {got:?}"
        );
    }

    proc.child.kill().await?;
    Ok(())
}

#[tokio::test]
async fn read_only_tools_lack_meta_osmcp_mutation() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;
    let tools = fetch_tools(&mut proc).await?;

    for name in READ_ONLY_TOOLS {
        let got = mutation_tag_of(&tools, name);
        assert!(
            got.is_none(),
            "read-only tool {name} MUST NOT carry `_meta.osmcp.mutation` \
(got {got:?}); the tag is the consent-flow gate."
        );
    }

    proc.child.kill().await?;
    Ok(())
}

/// Tripwire — any tool advertised with `_meta.osmcp.mutation` must be in the
/// explicit MUTATION_TAG_EXPECTED set. If you add a new mutating tool, you
/// MUST update this table AND the channel doc in `tools.rs`.
#[tokio::test]
async fn meta_mutation_tag_distribution_matches_known_set() -> Result<()> {
    let mut proc = spawn_server().await?;
    let _ = initialize(&mut proc).await?;
    let tools = fetch_tools(&mut proc).await?;

    let mut tagged: Vec<String> = tools
        .iter()
        .filter_map(|t| {
            let name = t["name"].as_str()?.to_string();
            t.get("_meta")
                .and_then(|m| m.get("osmcp"))
                .and_then(|o| o.get("mutation"))
                .and_then(|v| v.as_str())
                .map(|_| name)
        })
        .collect();
    tagged.sort();

    let mut expected: Vec<String> = MUTATION_TAG_EXPECTED
        .iter()
        .map(|(n, _)| (*n).to_string())
        .collect();
    expected.sort();

    assert_eq!(
        tagged, expected,
        "set of tools carrying `_meta.osmcp.mutation` drifted from MUTATION_TAG_EXPECTED. \
If you added or removed a mutating tool, update this table AND the channel header in tools.rs."
    );

    proc.child.kill().await?;
    Ok(())
}
