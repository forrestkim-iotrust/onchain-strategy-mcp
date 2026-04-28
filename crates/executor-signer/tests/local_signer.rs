use executor_signer::{LocalSignerConfig, LocalSignerHandle, SignerError};
use std::process::Command;

const FIXTURE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const FIXTURE_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const CHILD_ENV: &str = "EXECUTOR_SIGNER_CHILD_PRIVATE_KEY";
const CHILD_CASE: &str = "EXECUTOR_SIGNER_CHILD_CASE";

fn unique_env(name: &str) -> String {
    format!("EXECUTOR_SIGNER_TEST_{name}_{}", std::process::id())
}

fn maybe_run_child_case() {
    let Ok(case) = std::env::var(CHILD_CASE) else {
        return;
    };

    match case.as_str() {
        "invalid" => {
            let cfg = LocalSignerConfig::new(CHILD_ENV, 120_000).unwrap();
            let err = LocalSignerHandle::from_env(&cfg, 31_337).unwrap_err();
            assert_eq!(
                err,
                SignerError::InvalidPrivateKey {
                    env: CHILD_ENV.to_string(),
                }
            );
            let msg = err.to_string();
            assert!(msg.contains(CHILD_ENV));
            assert!(!msg.contains("not-a-private-key-sentinel"));
        }
        "valid" => {
            let cfg = LocalSignerConfig::new(CHILD_ENV, 120_000).unwrap();
            let handle = LocalSignerHandle::from_env(&cfg, 31_337).unwrap();
            assert_eq!(handle.signer_address_string(), FIXTURE_ADDRESS);
            let debug = format!("{handle:?}");
            assert!(debug.contains("signer_address"));
            assert!(!debug.contains(FIXTURE_KEY));
        }
        other => panic!("unknown child signer test case: {other}"),
    }
    std::process::exit(0);
}

fn run_child_case(case: &str, key: &str) {
    let current_exe = std::env::current_exe().unwrap();
    let status = Command::new(current_exe)
        .env(CHILD_CASE, case)
        .env(CHILD_ENV, key)
        .arg("--exact")
        .arg("child_env_case")
        .status()
        .unwrap();
    assert!(status.success(), "child case {case} failed: {status}");
}

#[test]
fn child_env_case() {
    maybe_run_child_case();
}

#[test]
fn missing_env_var_name_is_not_configured() {
    let err = LocalSignerConfig::new("  ", 120_000).unwrap_err();
    assert_eq!(err, SignerError::NotConfigured);
}

#[test]
fn configured_env_name_with_absent_var_fails_closed() {
    let env = unique_env("ABSENT");
    let cfg = LocalSignerConfig::new(env.clone(), 120_000).unwrap();
    let err = LocalSignerHandle::from_env(&cfg, 31_337).unwrap_err();
    assert_eq!(err, SignerError::MissingPrivateKeyEnv { env });
}

#[test]
fn invalid_env_value_omits_raw_secret_from_error() {
    run_child_case("invalid", "not-a-private-key-sentinel");
}

#[test]
fn valid_key_derives_address_without_debug_leaking_key() {
    run_child_case("valid", FIXTURE_KEY);
}
