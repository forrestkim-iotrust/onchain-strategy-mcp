//! Phase 1 integration tests — 각 Plan이 함수를 추가한다.
//! - Plan 02: tools_list_emits_full_surface, unimplemented_tools_return_phase_hint, readonly_tools_return_placeholder
//! - Plan 03: resources_surface_matches_contract, prompts_surface_matches_contract, stdout_is_strict_jsonrpc, schema_contract_round_trip

mod common;

// Plan 02/03이 이 파일에 #[tokio::test] 함수를 추가한다.
// Wave 0 단계: 파일 존재만 확인 — 실제 테스트는 후속 plan이 작성.

#[tokio::test]
async fn harness_compiles() -> anyhow::Result<()> {
    // Plan 01 확인용: common module이 컴파일되고 bin이 spawn 가능한지만 확인.
    let _ = common::spawn_server().await?;
    Ok(())
}
