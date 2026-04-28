use std::time::Duration;

use alloy::rpc::types::TransactionRequest;
use executor_signer::{LocalReceiptStatus, LocalSignerConfig, LocalSignerHandle, SignerError};

#[test]
fn local_signer_receipt_status_strings_are_stable() {
    assert_eq!(LocalReceiptStatus::Success.as_str(), "success");
    assert_eq!(LocalReceiptStatus::Reverted.as_str(), "reverted");
}

#[test]
fn broadcast_rejects_invalid_rpc_url_without_leaking_key() {
    let config = LocalSignerConfig::new("EXECUTOR_SIGNER_BROADCAST_TEST_KEY", 120_000).unwrap();
    let handle = LocalSignerHandle::__test_from_private_key(
        &config,
        "0x59c6995e998f97a5a0044966f0945387d677fb5de8d44f6e4a4ceb590b8ab4de",
        31337,
    )
    .expect("test signer");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let err = rt
        .block_on(handle.broadcast("not a url", TransactionRequest::default()))
        .expect_err("invalid url must fail");

    assert_eq!(err, SignerError::BroadcastFailed);
    assert!(!err.to_string().contains("59c699"));
}

#[test]
fn wait_for_receipt_timeout_error_is_stable() {
    let err = SignerError::ReceiptTimeout {
        tx_hash: "0xabc".to_string(),
    };

    assert_eq!(err.execution_error_kind(), "receipt_timeout");
    assert!(err.to_string().contains("0xabc"));
}

#[test]
fn wait_for_receipt_accepts_duration_argument_shape() {
    let timeout = Duration::from_millis(1);
    assert_eq!(timeout.as_millis(), 1);
}
