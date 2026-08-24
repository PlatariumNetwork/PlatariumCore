//! High (H1–H11) security/correctness coverage.

use platarium_core::core::core_rpc::{dispatch_rpc, handle_rpc_line};
use platarium_core::core::rpc_security::authorize_rpc_method;
use platarium_core::*;
use serde_json::json;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn h1_h2_validate_basic_requires_dual_keys_and_hash() {
    let tx = Transaction::new(
        "a".into(),
        "b".into(),
        Asset::PLP,
        1,
        1,
        0,
        Default::default(),
        Default::default(),
        "s1".into(),
        "s2".into(),
    )
    .unwrap();
    assert!(matches!(
        tx.validate_basic(),
        Err(TransactionValidationError::MissingPubMain)
    ));
}

#[test]
fn h5_state_root_includes_uplp() {
    let s = State::new();
    let a = "PxA".to_string();
    s.set_balance(&a, 100);
    let r1 = s.create_snapshot().compute_state_root();
    s.set_uplp_balance(&a, 7);
    let r2 = s.create_snapshot().compute_state_root();
    assert_ne!(r1, r2, "μPLP change must change state root");
}

#[test]
fn h5_state_root_includes_token_balance() {
    let s = State::new();
    let a = "PxA".to_string();
    s.set_balance(&a, 100);
    let r1 = s.create_snapshot().compute_state_root();
    s.credit_asset(&a, &Asset::Token("USDT".into()), 5);
    let r2 = s.create_snapshot().compute_state_root();
    assert_ne!(r1, r2, "token balance must change state root");
}

#[test]
fn h4_committee_ignores_client_f_via_from_authors() {
    use platarium_core::core::dag::CommitteeConfig;
    let c = CommitteeConfig::from_authors(vec![
        "n0".into(),
        "n1".into(),
        "n2".into(),
        "n3".into(),
    ])
    .unwrap();
    assert_eq!(c.f, 1);
    assert_eq!(c.quorum(), 3);
}

#[test]
fn h7_remote_sign_blocked_without_allow() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    std::env::remove_var("PLATARIUM_CORE_ALLOW_REMOTE_SIGN");
    std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "tok");
    let err = authorize_rpc_method("sign_transaction", Some("tok"));
    assert!(err.is_err(), "{err:?}");
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("remote sign") || msg.contains("blocked"), "{msg}");
    std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
}

#[test]
fn h7_handle_rpc_line_blocks_generate_keys() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    std::env::remove_var("PLATARIUM_CORE_ALLOW_REMOTE_SIGN");
    std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "tok");
    let resp = handle_rpc_line(
        r#"{"jsonrpc":"2.0","id":1,"auth_token":"tok","method":"generate_keys","params":{"mnemonic":"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about","alphanumeric":"x","seed_index":0}}"#,
    );
    assert!(resp.contains("-32001") || resp.contains("blocked") || resp.contains("remote sign"), "{resp}");
    std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
}

#[test]
fn h3_confirm_rejects_bad_tx() {
    let state = State::new();
    let tx = Transaction::new(
        "a".into(),
        "b".into(),
        Asset::PLP,
        1,
        1,
        0,
        Default::default(),
        Default::default(),
        "s1".into(),
        "s2".into(),
    )
    .unwrap();
    let votes = vec![("n1".into(), Vote::Confirm)];
    assert!(confirm_transaction_l1(&state, &tx, &votes).is_err());
}

#[test]
fn h6_faucet_does_not_bypass_admit_without_valid_tx() {
    use platarium_core::core::block_proposal::mempool_admit;
    let state = State::new();
    // FAUCET-like from with invalid sigs/pubs must not be auto-accepted.
    let tx = r#"{"hash":"t1","from":"faucet","to":"PxB","asset":"PLP","amount":1,"fee_uplp":1,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#;
    let r = mempool_admit(&state, tx, &[]);
    assert!(!r.accepted, "H6: faucet shortcut removed");
}

#[test]
fn handshake_rpc_ok() {
    let out = dispatch_rpc("handshake", &json!({})).unwrap();
    assert!(out.contains("\"protocol\":2") || out.contains("\"protocol\": 2"), "{out}");
}
