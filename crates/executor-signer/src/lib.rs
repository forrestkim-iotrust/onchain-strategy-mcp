#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]
//! Signer boundary — Phase 6에서 local signer 구현.

use executor_core::schema::execution::SignedTransaction;

/// Signer trait — Phase 1 시점에서는 경계만 잡아둔다. Phase 6가 실제 메서드를
/// 추가한다. `SignedTransaction` import는 downstream crate가 이 trait를 구현할
/// 때 바로 사용할 수 있도록 의존성을 미리 연결해 둔 것이다.
pub trait Signer: Send + Sync {
    // 실제 sign 메서드는 Phase 6 연구 후 확정.
}

// `SignedTransaction`이 아직 Phase 1에서는 사용되지 않지만 import 경로를
// lock-in하기 위해 유지한다. 컴파일러가 unused import로 경고하지 않도록
// 타입 alias로 노출.
#[doc(hidden)]
pub type _SignedTransactionAlias = SignedTransaction;
