//! v1.4 Track G: destructive-op tagging.
//!
//! rmcp 1.5 + schemars 1.x do not expose a portable hook for injecting
//! arbitrary `x-*` JSON-Schema extensions through the `#[tool(...)]` macro,
//! so we encode the mutation flag as a literal `[DESTRUCTIVE]` prefix in the
//! tool's `description` field. Clients gate user consent on
//! `^\[DESTRUCTIVE\]` against `tools/list` descriptions.
//!
//! This file asserts:
//!   1. Every destructive tool's description carries the marker.
//!   2. At least one representative non-destructive tool does NOT carry it
//!      (so the prefix isn't applied indiscriminately, which would defeat
//!      the consent signal).
//!
//! See `crates/executor-mcp/src/tools.rs` "Destructive ops" doc note and the
//! `INSTRUCTIONS` constant in `crates/executor-mcp/src/server.rs`.

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
