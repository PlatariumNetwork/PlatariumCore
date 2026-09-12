//! Low (L1–L2) security/ops coverage.
//! L1: high-S reject lives in `signature` unit tests.
//! L2: daemon flock / socket ownership lives in Gateway `daemon_test.go`.
//! R2-L1 / R2-L3: token compare + handshake capability gating.

use platarium_core::core::core_rpc::{dispatch_rpc, handle_rpc_line};
use platarium_core::core::rpc_security::authorize_rpc_method;
use platarium_core::signature::{sign_message, verify_signature};
use secp256k1::SecretKey;
use serde_json::json;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn l1_low_s_roundtrip_accepted() {
    let sk = SecretKey::from_slice(&[7u8; 32]).unwrap();
    let msg = serde_json::json!({"low": true});
    let sig = sign_message(&sk, &msg).unwrap();
    assert!(verify_signature(&msg, &sig.signature_compact[..128], &sig.pub_key).unwrap());
}

/// R2-L1: equal-length wrong token is rejected (constant-time path exercised).
#[test]
fn r2_l1_token_compare_rejects_equal_length_mismatch() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    std::env::remove_var("PLATARIUM_CORE_RPC_READ_TOKEN");
    std::env::remove_var("PLATARIUM_CORE_RPC_MUTATE_TOKEN");
    std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "abcdefgh");
    assert!(authorize_rpc_method("state_apply_tx", Some("abcdefgx")).is_err());
    assert!(authorize_rpc_method("state_apply_tx", Some("abcdefgh")).is_ok());
    std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
}

/// R2-L3: unauthenticated handshake must not leak remote_sign / testnet flags.
#[test]
fn r2_l3_handshake_hides_capabilities_without_auth() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    std::env::remove_var("PLATARIUM_CORE_RPC_READ_TOKEN");
    std::env::remove_var("PLATARIUM_CORE_RPC_MUTATE_TOKEN");
    std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "hs-secret");
    std::env::set_var("PLATARIUM_CORE_ALLOW_REMOTE_SIGN", "1");
    std::env::set_var("PLATARIUM_CORE_TESTNET", "1");

    let bare = dispatch_rpc("handshake", &json!({})).unwrap();
    assert!(bare.contains("\"protocol\":2") || bare.contains("\"protocol\": 2"), "{bare}");
    assert!(
        !bare.contains("remote_sign") && !bare.contains("testnet"),
        "capabilities leaked: {bare}"
    );

    let line = handle_rpc_line(
        r#"{"jsonrpc":"2.0","id":1,"auth_token":"hs-secret","method":"handshake","params":{}}"#,
    );
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    let result = &resp["result"];
    assert_eq!(result["remote_sign"], true);
    assert_eq!(result["testnet"], true);

    std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
    std::env::remove_var("PLATARIUM_CORE_ALLOW_REMOTE_SIGN");
    std::env::remove_var("PLATARIUM_CORE_TESTNET");
}
