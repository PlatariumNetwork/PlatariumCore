//! Integration tests for persistent state file CLI helpers.

use platarium_core::*;
use std::collections::HashSet;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn enable_testnet() {
    std::env::set_var("PLATARIUM_CORE_TESTNET", "1");
}

fn temp_state_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("platarium-state-test-{}-{}.json", name, std::process::id()))
}

#[test]
fn state_file_init_query_credit_root() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    let path = temp_state_path("init");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");

    state_credit_json(&path, "PxAlice", 1_000_000, 10_000, true).expect("credit");
    let query = state_query_json(&path, "PxAlice", "PLP").expect("query");
    assert!(query.contains("\"balance\":\"1000000\""));
    assert!(query.contains("\"nonce\":0"));

    let root1 = state_root_json(&path).expect("root");
    assert!(root1.contains("state_root"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_credit_token_xp_accumulates_and_query_exposes_tokens() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    let path = temp_state_path("xp");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");

    state_credit_token_json(&path, "PxAlice", "Token:XP", 100, true).expect("credit1");
    state_credit_token_json(&path, "PxAlice", "XP", 50, true).expect("credit2");
    let query = state_query_json(&path, "PxAlice", "PLP").expect("query");
    assert!(query.contains("\"xp\":\"150\""), "got {query}");
    assert!(query.contains("Token:XP"), "got {query}");

    let deny = state_credit_token_json(&path, "PxAlice", "USDT", 1, true).unwrap_err();
    assert!(deny.to_string().contains("accumulate-only"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_validate_rejects_wrong_nonce() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    let path = temp_state_path("nonce");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");
    state_credit_json(&path, "sender", 1000, 10, true).expect("credit");

    let tx = Transaction::new(
        "sender".to_string(),
        "receiver".to_string(),
        Asset::PLP,
        100,
        1,
        5,
        HashSet::new(),
        HashSet::new(),
        "00".repeat(64),
        "11".repeat(64),
    )
    .expect("tx");
    let tx_json = serde_json::json!({
        "hash": tx.hash,
        "from": tx.from,
        "to": tx.to,
        "asset": "PLP",
        "amount": tx.amount,
        "fee_uplp": tx.fee_uplp,
        "nonce": tx.nonce,
        "reads": [],
        "writes": [],
        "sig_main": tx.sig_main,
        "sig_derived": tx.sig_derived,
    })
    .to_string();

    let out = state_validate_tx_json(&path, &tx_json).expect("validate");
    assert!(out.contains("\"valid\":false"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_file_roundtrip_preserves_root() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    let path = temp_state_path("roundtrip");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");
    state_credit_json(&path, "a1", 500, 5, true).expect("credit");
    let root1 = state_root_json(&path).expect("root1");

    let state = load_state_file(&path).expect("load");
    save_state_file(&path, &state).expect("save");
    let root2 = state_root_json(&path).expect("root2");
    assert_eq!(root1, root2);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_credit_accumulates_balance() {
    let _g = ENV_LOCK.lock().unwrap();
    enable_testnet();
    let path = temp_state_path("accumulate");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");

    state_credit_json(&path, "PxAlice", 5000, 0, true).expect("credit1");
    state_credit_json(&path, "PxAlice", 5000, 0, true).expect("credit2");
    let query = state_query_json(&path, "PxAlice", "PLP").expect("query");
    assert!(
        query.contains("\"balance\":\"10000\""),
        "expected cumulative 10000 PLP, got {query}"
    );

    let _ = std::fs::remove_file(&path);
}
