//! End-to-end block formation tests: proposal, packing, atomic RocksDB commit, reopen, snapshots.

use platarium_core::storage::schema::{key_account, key_state_root};
use platarium_core::{
    block_proposal_status, bootstrap_from_snapshot, build_commit_batch, calculate_fee_from_load,
    commit_block, create_snapshot_if_due, get_account, get_block, get_head, get_receipt,
    get_state_root, get_tx, list_snapshots, mempool_admit, parse_mempool_snapshot, select_block_txs,
    AccountRecord,
    BlockCommit, BlockRecordStored, ReceiptRecord, RocksStore, State, BLOCK_GAS_CAP_UPLP,
    BLOCK_MAX_WAIT_SEC, SNAPSHOT_INTERVAL,
};
use tempfile::TempDir;

fn entry_json(hash: &str, from: &str, nonce: u64, fee: u64, amount: u64, idx: u64) -> String {
    format!(
        r#"{{"hash":"{hash}","from":"{from}","to":"PxRecv","asset":"PLP","amount":{amount},"fee_uplp":{fee},"nonce":{nonce},"reads":[],"writes":[],"sig_main":"{sig}","sig_derived":"{sig}","arrival_index":{idx},"timestamp":100}}"#,
        hash = hash,
        from = from,
        amount = amount,
        fee = fee,
        nonce = nonce,
        sig = "aa".repeat(64),
        idx = idx,
    )
}

#[test]
fn select_respects_gas_cap() {
    let state = State::new();
    let mempool_json = format!(
        "[{},{},{}]",
        entry_json("a", "PxA", 0, 2000, 1, 0),
        entry_json("b", "PxB", 0, 2000, 1, 1),
        entry_json("c", "PxC", 0, 2000, 1, 2),
    );
    let mempool = parse_mempool_snapshot(&mempool_json).unwrap();
    let r = select_block_txs(&state, &mempool);
    assert_eq!(r.tx_count, 2);
    assert!(r.gas_used <= BLOCK_GAS_CAP_UPLP);
    assert_eq!(r.hashes, vec!["a", "b"]);
}

#[test]
fn select_nonce_order_and_gap() {
    let state = State::new();
    let mempool_json = format!(
        "[{},{}]",
        entry_json("late", "PxA", 1, 1, 1, 0),
        entry_json("first", "PxA", 0, 1, 1, 1),
    );
    let mempool = parse_mempool_snapshot(&mempool_json).unwrap();
    let r = select_block_txs(&state, &mempool);
    assert_eq!(r.hashes, vec!["first"]);
}

#[test]
fn should_propose_min_tx_and_max_wait() {
    let mempool = parse_mempool_snapshot(&format!("[{}]", entry_json("x", "PxA", 0, 1, 1, 0))).unwrap();
    assert!(block_proposal_status(&mempool, 100).should_propose);

    // Empty gas edge: use fee 0 entries only for wait path — status uses fee sum; wait still triggers.
    let mut wait_entry = entry_json("w", "PxA", 0, 1, 1, 0);
    wait_entry = wait_entry.replace("\"timestamp\":100", "\"timestamp\":1");
    let mempool = parse_mempool_snapshot(&format!("[{}]", wait_entry)).unwrap();
    let st = block_proposal_status(&mempool, 1 + BLOCK_MAX_WAIT_SEC);
    assert!(st.should_propose);
    assert!(st.oldest_mempool_wait_sec >= BLOCK_MAX_WAIT_SEC);
}

#[test]
fn min_fee_from_load_buckets() {
    assert_eq!(calculate_fee_from_load(0), 1);
    assert_eq!(calculate_fee_from_load(300), 1);
    assert_eq!(calculate_fee_from_load(310), 2);
    assert_eq!(calculate_fee_from_load(610), 3);
    assert_eq!(calculate_fee_from_load(810), 5);
}

#[test]
fn mempool_admit_rejects_bad_nonce() {
    let state = State::new();
    state.set_balance(&"PxA".to_string(), 1000);
    let tx = entry_json("t1", "PxA", 5, 1, 1, 0);
    // Strip arrival_index for from_gateway_json — use raw tx fields via admit with empty mempool.
    let tx_only = r#"{"hash":"t1","from":"PxA","to":"PxRecv","asset":"PLP","amount":1,"fee_uplp":1,"nonce":5,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#;
    let r = mempool_admit(&state, tx_only, &[]);
    assert!(!r.accepted);
    assert_eq!(r.expected_nonce, 0);
    let _ = tx;
}

#[test]
fn same_mempool_same_select() {
    let state = State::new();
    let mempool_json = format!(
        "[{},{}]",
        entry_json("a", "PxA", 0, 1, 1, 0),
        entry_json("b", "PxB", 0, 1, 1, 1),
    );
    let m = parse_mempool_snapshot(&mempool_json).unwrap();
    let r1 = select_block_txs(&state, &m);
    let r2 = select_block_txs(&state, &m);
    assert_eq!(r1.hashes, r2.hashes);
}

fn make_commit(height: u64, hashes: &[&str], fees: &[u64]) -> BlockCommit {
    let tx_jsons: Vec<String> = hashes
        .iter()
        .enumerate()
        .map(|(i, h)| {
            format!(
                r#"{{"hash":"{}","from":"PxA","to":"PxB","asset":"PLP","amount":1,"fee_uplp":{},"nonce":{},"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}}"#,
                h, fees.get(i).copied().unwrap_or(1), i
            )
        })
        .collect();
    BlockCommit {
        block: BlockRecordStored {
            height,
            previous_hash: if height <= 1 {
                "0".into()
            } else {
                format!("bh{}", height - 1)
            },
            timestamp: height as i64,
            tx_hashes: hashes.iter().map(|s| (*s).to_string()).collect(),
            merkle_root: "m".into(),
            state_root: format!("root{}", height),
            block_hash: format!("bh{}", height),
            producer_id: "n1".into(),
        },
        tx_jsons,
        accounts: vec![
            AccountRecord {
                address: "PxA".into(),
                balance: "100".into(),
                uplp_balance: "0".into(),
                nonce: hashes.len() as u64,
            },
            AccountRecord {
                address: "PxB".into(),
                balance: hashes.len().to_string(),
                uplp_balance: "0".into(),
                nonce: 0,
            },
        ],
        receipts: hashes
            .iter()
            .enumerate()
            .map(|(i, h)| ReceiptRecord {
                tx_hash: (*h).to_string(),
                status: "ok".into(),
                fee_uplp: fees.get(i).copied().unwrap_or(1),
                block_height: height,
            })
            .collect(),
        state_root: format!("root{}", height),
    }
}

#[test]
fn e2e_single_tx_block() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    commit_block(&store, &make_commit(1, &["tx1"], &[1])).unwrap();
    assert_eq!(get_head(&store).unwrap(), 1);
    assert!(get_tx(&store, "tx1").unwrap().is_some());
    assert!(get_block(&store, 1).unwrap().is_some());
    assert_eq!(get_receipt(&store, "tx1").unwrap().unwrap().status, "ok");
    assert_eq!(get_account(&store, "PxA").unwrap().unwrap().nonce, 1);
}

#[test]
fn e2e_multi_tx_same_block() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    commit_block(&store, &make_commit(1, &["a", "b", "c"], &[1, 1, 1])).unwrap();
    assert_eq!(get_head(&store).unwrap(), 1);
    let block = get_block(&store, 1).unwrap().unwrap();
    assert_eq!(block.tx_hashes.len(), 3);
}

#[test]
fn e2e_atomic_commit_visible() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    commit_block(&store, &make_commit(1, &["tx1"], &[1])).unwrap();
    assert_eq!(get_head(&store).unwrap(), 1);
    assert!(get_tx(&store, "tx1").unwrap().is_some());
    assert!(get_block(&store, 1).unwrap().is_some());
    assert!(get_account(&store, "PxA").unwrap().is_some());
    assert!(get_receipt(&store, "tx1").unwrap().is_some());
    assert_eq!(get_state_root(&store, 1).unwrap().unwrap(), "root1");
}

#[test]
fn crash_after_commit_durable() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    commit_block(&store, &make_commit(1, &["tx1"], &[1])).unwrap();
    let store = store.reopen().unwrap();
    assert_eq!(get_head(&store).unwrap(), 1);
    assert!(get_block(&store, 1).unwrap().is_some());
}

#[test]
fn e2e_gas_cap_spill_two_blocks() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    // Simulate two blocks packing 2000+2000 then 2000
    commit_block(&store, &make_commit(1, &["a", "b"], &[2000, 2000])).unwrap();
    commit_block(&store, &make_commit(2, &["c"], &[2000])).unwrap();
    assert_eq!(get_head(&store).unwrap(), 2);
    assert!(get_tx(&store, "a").unwrap().is_some());
    assert!(get_tx(&store, "b").unwrap().is_some());
    assert!(get_tx(&store, "c").unwrap().is_some());
}

#[test]
fn e2e_restart_reopen_db() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    commit_block(&store, &make_commit(1, &["tx1"], &[1])).unwrap();
    let store = store.reopen().unwrap();
    assert_eq!(get_head(&store).unwrap(), 1);
    assert!(get_tx(&store, "tx1").unwrap().is_some());
    assert_eq!(get_account(&store, "PxA").unwrap().unwrap().balance, "100");
}

#[test]
fn crash_no_partial_block() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    let _batch = build_commit_batch(&make_commit(1, &["tx1"], &[1])).unwrap();
    assert_eq!(get_head(&store).unwrap(), 0);
    assert!(get_tx(&store, "tx1").unwrap().is_none());
}

#[test]
fn no_orphan_tx_without_block() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    commit_block(&store, &make_commit(1, &["tx1"], &[1])).unwrap();
    let block = get_block(&store, 1).unwrap().unwrap();
    assert!(block.tx_hashes.contains(&"tx1".to_string()));
    assert!(get_tx(&store, "tx1").unwrap().is_some());
}

#[test]
fn eventual_inclusion_two_rounds() {
    let state = State::new();
    let mempool_json = format!(
        "[{},{},{}]",
        entry_json("a", "PxA", 0, 2000, 1, 0),
        entry_json("b", "PxB", 0, 2000, 1, 1),
        entry_json("c", "PxC", 0, 2000, 1, 2),
    );
    let mut mempool = parse_mempool_snapshot(&mempool_json).unwrap();
    let mut included = Vec::new();
    for _ in 0..3 {
        let r = select_block_txs(&state, &mempool);
        if r.hashes.is_empty() {
            break;
        }
        included.extend(r.hashes.clone());
        mempool.retain(|e| !r.hashes.contains(&e.tx.hash));
    }
    assert_eq!(included.len(), 3);
}

#[test]
fn snapshot_at_interval_and_bootstrap() {
    let dir = TempDir::new().unwrap();
    let store = RocksStore::open(dir.path().join("db")).unwrap();
    store
        .put(&key_state_root(SNAPSHOT_INTERVAL), b"rootSnap")
        .unwrap();
    store
        .put(
            &key_account("PxA"),
            &serde_json::to_vec(&AccountRecord {
                address: "PxA".into(),
                balance: "42".into(),
                uplp_balance: "0".into(),
                nonce: 3,
            })
            .unwrap(),
        )
        .unwrap();
    let snap = create_snapshot_if_due(&store, SNAPSHOT_INTERVAL)
        .unwrap()
        .expect("due");
    assert_eq!(list_snapshots(&store).unwrap().len(), 1);

    let dir2 = TempDir::new().unwrap();
    let store2 = RocksStore::open(dir2.path().join("db")).unwrap();
    bootstrap_from_snapshot(&store2, &snap).unwrap();
    assert_eq!(store2.head_height().unwrap(), SNAPSHOT_INTERVAL);
    assert_eq!(
        get_account(&store2, "PxA").unwrap().unwrap().balance,
        "42"
    );
}
