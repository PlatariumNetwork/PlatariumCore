//! Critical security fixes C1–C6 coverage.

use platarium_core::core::core_rpc::{dispatch_rpc, handle_rpc_line};
use platarium_core::core::rpc_security::{
    authorize_rpc_method, external_kernel_commit_allowed, server_testnet_enabled,
};
use platarium_core::*;
use serde_json::json;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn enable_testnet() {
    std::env::set_var("PLATARIUM_CORE_TESTNET", "1");
}

fn disable_testnet() {
    // Explicit deny for C4 coverage (cfg(test) defaults to allow when unset).
    std::env::set_var("PLATARIUM_CORE_TESTNET", "0");
    std::env::remove_var("PLATARIUM_TESTNET");
}

fn temp_state(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "platarium-crit-{}-{}.json",
        name,
        std::process::id()
    ))
}

#[test]
fn c4_state_credit_requires_server_testnet_env() {
    let _g = ENV_LOCK.lock().unwrap();
    disable_testnet();
    let path = temp_state("c4");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).unwrap();
    let err = state_credit_json(&path, "PxA", 100, 0, true).unwrap_err();
    assert!(
        err.to_string().contains("PLATARIUM_CORE_TESTNET")
            || err.to_string().contains("denied"),
        "{err}"
    );
    enable_testnet();
    state_credit_json(&path, "PxA", 100, 0, true).expect("credit with server flag");
    let deny_client = state_credit_json(&path, "PxA", 1, 0, false).unwrap_err();
    assert!(deny_client.to_string().contains("testnet"));
    disable_testnet();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn c2_rpc_auth_rejects_privileged_without_token() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "crit-token");
    assert!(authorize_rpc_method("ping", None).is_ok());
    assert!(authorize_rpc_method("state_credit", None).is_err());
    assert!(authorize_rpc_method("state_credit", Some("wrong")).is_err());
    assert!(authorize_rpc_method("state_credit", Some("crit-token")).is_ok());
    let denied = handle_rpc_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"state_query","params":{"state_file":"x","address":"a"}}"#,
    );
    assert!(denied.contains("-32001") || denied.contains("unauthorized") || denied.contains("auth"));
    let ok_ping = handle_rpc_line(r#"{"jsonrpc":"2.0","id":2,"method":"ping","params":{}}"#);
    assert!(ok_ping.contains("\"result\""));
    let ok_auth = handle_rpc_line(
        r#"{"jsonrpc":"2.0","id":3,"auth_token":"crit-token","method":"ping","params":{}}"#,
    );
    assert!(ok_auth.contains("\"result\""));
    std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
}

#[test]
fn c3_kernel_commit_diff_disabled_by_default() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("PLATARIUM_CORE_ALLOW_EXTERNAL_COMMIT");
    assert!(!external_kernel_commit_allowed());
    std::env::set_var("PLATARIUM_CORE_RPC_INSECURE", "1");
    let path = temp_state("c3");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).unwrap();
    let err = dispatch_rpc(
        "kernel_commit_diff",
        &json!({
            "state_file": path.to_string_lossy(),
            "diff": {
                "schema_version": 1,
                "batch_id": "x",
                "receipts": [],
                "accounts": [],
                "post_state_root": "00"
            }
        }),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("disabled") || err.to_string().contains("kernel_apply_batch"),
        "{err}"
    );
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn c3_kernel_apply_batch_executes_and_commits() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    std::env::set_var("PLATARIUM_CORE_RPC_INSECURE", "1");
    let path = temp_state("c3apply");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).unwrap();

    let (mnemonic, alpha) = generate_mnemonic().unwrap();
    let keys_out = dispatch_rpc(
        "generate_keys",
        &json!({
            "mnemonic": mnemonic,
            "alphanumeric": alpha,
            "seed_index": 0
        }),
    )
    .unwrap();
    let keys: serde_json::Value = serde_json::from_str(&keys_out).unwrap();
    let from = keys["publicKey"].as_str().unwrap();
    state_credit_json(&path, from, 10_000, 100, true).unwrap();

    let signed = dispatch_rpc(
        "sign_transaction",
        &json!({
            "from": from,
            "to": "PxBobCrit0000000000000000000000000000000000000000000000000001",
            "asset": "PLP",
            "amount": 50u64,
            "fee_uplp": 1u64,
            "nonce": 0u64,
            "reads": "[]",
            "writes": "[]",
            "mnemonic": mnemonic,
            "alphanumeric": alpha
        }),
    )
    .unwrap();
    let tx_val: serde_json::Value = serde_json::from_str(&signed).unwrap();

    let out = dispatch_rpc(
        "kernel_apply_batch",
        &json!({
            "state_file": path.to_string_lossy(),
            "parallel": false,
            "batch": {
                "batch_id": "crit",
                "height": 1,
                "transactions": [tx_val]
            }
        }),
    )
    .expect("kernel_apply_batch");
    assert!(out.contains("\"ok\":true") || out.contains("\"ok\": true"), "{out}");
    let q = state_query_json(&path, from, "PLP").unwrap();
    assert!(q.contains("\"nonce\":1") || q.contains("\"nonce\": 1"), "{q}");

    disable_testnet();
    std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn c1_signed_tx_with_stolen_from_rejected_on_apply() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    let path = temp_state("c1");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).unwrap();

    let (mnemonic, alpha) = generate_mnemonic().unwrap();
    let keys_out = dispatch_rpc(
        "generate_keys",
        &json!({
            "mnemonic": mnemonic,
            "alphanumeric": alpha,
            "seed_index": 0
        }),
    )
    .unwrap();
    let keys: serde_json::Value = serde_json::from_str(&keys_out).unwrap();
    let attacker_from = keys["publicKey"].as_str().unwrap().to_string();
    let victim = "PxVictimCrit000000000000000000000000000000000000000000000000001";
    state_credit_json(&path, victim, 50_000, 10, true).unwrap();
    state_credit_json(&path, &attacker_from, 1_000, 10, true).unwrap();

    let signed = dispatch_rpc(
        "sign_transaction",
        &json!({
            "from": attacker_from,
            "to": "PxBobCrit0000000000000000000000000000000000000000000000000002",
            "asset": "PLP",
            "amount": 10u64,
            "fee_uplp": 1u64,
            "nonce": 0u64,
            "reads": "[]",
            "writes": "[]",
            "mnemonic": mnemonic,
            "alphanumeric": alpha
        }),
    )
    .unwrap();
    let mut tx: serde_json::Value = serde_json::from_str(&signed).unwrap();
    // Steal victim address while keeping attacker signatures/pubs.
    tx["from"] = json!(victim);
    let bad = tx.to_string();
    let parsed = Transaction::from_gateway_json(&bad).unwrap();
    assert!(matches!(
        parsed.validate_basic(),
        Err(TransactionValidationError::FromPubkeyMismatch)
    ));
    let apply = state_apply_tx_json(&path, &bad);
    assert!(apply.is_err() || apply.as_ref().unwrap().contains("error") || apply.as_ref().unwrap().contains("false") || apply.is_err());
    // state_apply_tx_json returns Err on validation failure
    assert!(apply.is_err(), "{apply:?}");
    let q = state_query_json(&path, victim, "PLP").unwrap();
    assert!(q.contains("\"balance\":\"50000\""), "victim untouched: {q}");

    disable_testnet();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn server_testnet_helper_reads_env() {
    let _g = ENV_LOCK.lock().unwrap();
    disable_testnet();
    assert!(!server_testnet_enabled());
    enable_testnet();
    assert!(server_testnet_enabled());
    std::env::remove_var("PLATARIUM_CORE_TESTNET");
}
