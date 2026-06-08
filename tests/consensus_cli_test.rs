//! Integration tests for consensus CLI helpers.

use platarium_core::*;
use std::collections::HashSet;

fn temp_state_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("platarium-consensus-test-{}-{}.json", name, std::process::id()))
}

#[test]
fn l1_process_votes_confirms_at_67_percent() {
    let votes = r#"[
        {"node_id":"a","yes":true},
        {"node_id":"b","yes":true}
    ]"#;
    let out = l1_process_votes_json(votes).expect("l1 votes");
    assert!(out.contains("\"confirmed\":true"));
}

#[test]
fn assemble_block_returns_deterministic_hash() {
    let path = temp_state_path("assemble");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");
    state_credit_json(&path, "alice", 1000, 10, true).expect("credit");
    let tx_hashes = r#"["abc123"]"#;
    let out1 = assemble_block_json(&path, 1, "00", 1700000000, tx_hashes, "node-1").expect("assemble");
    let out2 = assemble_block_json(&path, 1, "00", 1700000000, tx_hashes, "node-1").expect("assemble");
    assert_eq!(out1, out2);
    assert!(out1.contains("block_hash"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn l1_verify_txs_rejects_bad_nonce() {
    let path = temp_state_path("l1verify");
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
    let txs = format!("[{}]", serde_json::to_string(&tx_json).unwrap());
    let out = l1_verify_txs_json(&path, &txs).expect("verify");
    assert!(out.contains("\"valid\":false"));
    assert!(out.contains("tx_results"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn l1_verify_txs_returns_per_tx_results() {
    let path = temp_state_path("l1verify-ok");
    let _ = std::fs::remove_file(&path);
    init_state_file(&path).expect("init");
    let out = l1_verify_txs_json(&path, "[]").expect("verify");
    assert!(out.contains("\"valid\":true"));
    assert!(out.contains("tx_results"));
    let _ = std::fs::remove_file(&path);
}
