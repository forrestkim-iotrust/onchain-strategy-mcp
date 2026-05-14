//! v1.8 integration: register a bundle whose view calls `ctx.evm.getLogs`
//! and assert the resource body shape.
//!
//! The test server boots without a configured devnet (no provider), so the
//! view function detects this gracefully via try/catch and returns
//! `{ count: 0, available: false }`. This proves three things:
//!
//! 1. `ctx.evm.getLogs` is wired (the strategy doesn't ReferenceError on it).
//! 2. The typed no-provider envelope reaches JS as a catchable Error.
//! 3. The view → resource round-trip preserves the user's data shape.
//!
//! When anvil + a real provider are available, the same view returns the
//! actual log count instead of `available: false`.

mod common;

use anyhow::Result;
use serde_json::{Value, json};

use common::{initialize, recv, send, spawn_server_with_state};

fn read_resource_body(r: &Value) -> Value {
    let text = r["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("resources/read missing contents[0].text: {r}"));
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("resource body is not JSON: {e} — text={text}"))
}

#[tokio::test]
async fn view_calling_get_logs_round_trips_typed_envelope() -> Result<()> {
    let tmp = tempfile::NamedTempFile::new()?;
    let db_path = tmp.path().to_path_buf();
    let _ = tmp.into_temp_path().keep()?;

    // The view function:
    //   - sanity-checks `typeof ctx.evm.getLogs === "function"` (proves the
    //     binding is installed),
    //   - sanity-checks `typeof ctx.abi.decodeUint256 === "function"`,
    //   - tries a real getLogs call; if no provider is configured, the
    //     try/catch maps to `{ available: false }`.
    let sid = {
        use executor_state::{RegisterOutcome, StateStore};
        let mut store = StateStore::open(&db_path)?;
        let view_source = r#"(ctx, _records) => {
            const wired = (typeof ctx.evm.getLogs === "function")
                       && (typeof ctx.abi.decodeUint256 === "function");
            let count = 0;
            let available = false;
            let error_msg = null;
            try {
                const logs = ctx.evm.getLogs({
                    address: "0x0000000000000000000000000000000000000001",
                    fromBlock: "earliest",
                    toBlock: "latest",
                    topics: [
                        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
                    ]
                });
                count = logs.length;
                available = true;
            } catch (e) {
                error_msg = String(e.message || e);
            }
            return { wired, count, available, error_msg };
        }"#;
        let sid = match store.register_strategy_bundle(
            "view-getlogs", "(ctx) => 'noop'", None, None,
            None, Some(view_source), None,
        )? {
            RegisterOutcome::Created(s)
            | RegisterOutcome::AlreadyExists(s)
            | RegisterOutcome::ReplacedVersion { created: s, .. } => s.id,
        };
        sid
    };

    let db_str = db_path.to_string_lossy().to_string();
    let mut proc = spawn_server_with_state(&db_str).await?;
    let _ = initialize(&mut proc).await?;

    send(
        &mut proc,
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/read",
            "params": { "uri": format!("strategy://{sid}/view") }
        }),
    )
    .await?;
    let r = recv(&mut proc).await?;
    assert!(r.get("error").is_none(), "expected success, got {r}");
    let body = read_resource_body(&r);

    // Either confidence:"full" + data.wired === true, or partial when the
    // sandbox itself failed before evaluating the view body.
    assert_eq!(body["confidence"], json!("full"), "body={body}");
    let data = &body["data"];
    assert_eq!(data["wired"], json!(true), "ctx.evm.getLogs and ctx.abi.decodeUint256 must both be wired; body={body}");
    // Without anvil the call returns available=false + an error message
    // that mentions getLogs.
    let available = data["available"].as_bool().unwrap_or(false);
    if !available {
        let msg = data["error_msg"].as_str().unwrap_or("");
        assert!(
            msg.contains("getLogs") || msg.contains("no provider") || msg.contains("evm"),
            "expected getLogs/no-provider/evm in error_msg, got {msg:?}"
        );
        assert_eq!(data["count"], json!(0));
    } else {
        // Provider WAS configured — accept whatever count came back.
        let _ = data["count"].as_u64().expect("count is a number when available");
    }

    proc.child.kill().await?;
    Ok(())
}
