//! v1.13 P5 — pin the `strategy://{id}/view` response shape that the
//! frontend `renderObject` consumes.
//!
//! The web UI (crates/executor-mcp/src/web_assets/app.js) routes the
//! strategy-detail "view output" panel through `window.osmcpRenderObject`.
//! `renderObject` discovers panels from top-level keys of `body.data` and
//! infers shape per panel (`$assets` → object-array table; `earnings` /
//! `activity` → key/value table).
//!
//! This test protects against silent backend changes that would break the
//! renderer's input contract by asserting that a bundle strategy whose
//! `view(ctx, records)` returns `{ $assets: [...], earnings: {...} }`
//! produces a response whose `data` is an object containing those keys
//! with the expected shapes.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{extract_resource_json, initialize, read_resource, spawn_server_with_state};

#[tokio::test]
async fn view_response_data_carries_assets_array_and_earnings_object() -> Result<()> {
    // Seed a bundle strategy whose view returns the canonical
    // `{ $assets: [...], earnings: {...} }` shape that the v1.13
    // renderObject is designed to render as a table + KV panel.
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    let sid = {
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        // Hand-authored `$assets` literal + `earnings` object. Two
        // entries in `$assets` (principal + accrued interest) mirror the
        // live `eth-funnel-bundle-v3` shape called out in the P5 brief.
        let view_source = r#"(ctx, records) => ({
            "$assets": [
                {
                    "chain_id": 8453,
                    "venue": "aave",
                    "asset": "USDC",
                    "amount": "10.0",
                    "usd": 10.0,
                    "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bda02913"
                },
                {
                    "chain_id": 8453,
                    "venue": "aave",
                    "asset": "aUSDC",
                    "amount": "1.563687",
                    "usd": 1.563687,
                    "address": "0x4e65fE4DBa92790696d040ac24aa414708F5c0AB"
                }
            ],
            earnings: { total_usd: 11.563687, principal_usd: 10.0, accrued_usd: 1.563687 },
            activity: { last_supply_at: "2025-05-01T00:00:00Z", run_count: 3 }
        })"#;
        let outcome = store.register_strategy_bundle(
            "render-shape-fixture",
            "(ctx) => 'noop'",
            None,
            None,
            None,
            Some(view_source),
            None,
            None,
        )?;
        match outcome {
            RegisterOutcome::Created(s) | RegisterOutcome::AlreadyExists(s) => s.id,
            RegisterOutcome::ReplacedVersion { created, .. } => created.id,
        }
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    let r = read_resource(&mut proc, 2, &format!("strategy://{sid}/view")).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = extract_resource_json(&r);

    // Honesty envelope intact — confidence must be `full` when view runs
    // without error. (Anything else means the renderer's else-branch
    // would kick in, which is also fine — but we're pinning the happy
    // path here.)
    assert_eq!(body["confidence"], json!("full"), "body={body}");

    let data = &body["data"];
    assert!(data.is_object(), "data must be an object — body={body}");

    // `$assets` must be an array of objects so renderObject's shape
    // inference picks `object-table` (Layer 2) and renders the table.
    let assets = data.get("$assets").and_then(Value::as_array).unwrap_or_else(|| {
        panic!("data.$assets must be an array; body={body}");
    });
    assert!(
        assets.len() >= 2,
        "expected at least two $assets entries; got {} body={body}",
        assets.len()
    );
    for (i, a) in assets.iter().enumerate() {
        assert!(
            a.is_object(),
            "$assets[{i}] must be an object (so renderObject infers object-table); body={body}"
        );
        // Field-name conventions the renderer's default kindOf relies on.
        assert!(
            a.get("chain_id").is_some(),
            "$assets[{i}] should carry chain_id (renderer formats as CHAIN_LABELS + id); body={body}"
        );
        assert!(
            a.get("address").is_some(),
            "$assets[{i}] should carry address (renderer formats as shortened addr-copy); body={body}"
        );
    }

    // `earnings` must be a plain object so renderObject picks
    // `object-kv` and renders a key/value table.
    let earnings = data.get("earnings").unwrap_or_else(|| {
        panic!("data.earnings must be present; body={body}");
    });
    assert!(
        earnings.is_object() && !earnings.is_null(),
        "data.earnings must be an object; body={body}"
    );

    // Sanity: also exercise the `activity` panel so the contract pins
    // multiple object-kv panels co-existing with the assets table.
    let activity = data.get("activity").unwrap_or_else(|| {
        panic!("data.activity must be present; body={body}");
    });
    assert!(
        activity.is_object(),
        "data.activity must be an object; body={body}"
    );

    proc.child.kill().await?;
    Ok(())
}
