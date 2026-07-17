//! Atomic block commit via RocksDB WriteBatch.

use crate::error::{PlatariumError, Result};
use crate::storage::rocks::RocksStore;
use crate::storage::schema::{
    KEY_META_HEAD, encode_u64, key_account, key_block, key_idx_addr, key_idx_block, key_receipt,
    key_state_root, key_tx,
};
use crate::storage::snapshot::create_snapshot_if_due;
use rocksdb::WriteBatch;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRecord {
    pub address: String,
    pub balance: String,
    pub uplp_balance: String,
    pub nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiptRecord {
    pub tx_hash: String,
    pub status: String,
    pub fee_uplp: u64,
    pub block_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockRecordStored {
    pub height: u64,
    pub previous_hash: String,
    pub timestamp: i64,
    pub tx_hashes: Vec<String>,
    pub merkle_root: String,
    pub state_root: String,
    pub block_hash: String,
    pub producer_id: String,
}

/// Full atomic commit payload for one finalized block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockCommit {
    pub block: BlockRecordStored,
    /// Full transaction JSON (gateway/Core format) keyed by hash order in block.tx_hashes.
    pub tx_jsons: Vec<String>,
    pub accounts: Vec<AccountRecord>,
    pub receipts: Vec<ReceiptRecord>,
    pub state_root: String,
}

/// Commit block + txs + accounts + receipts + indexes in a single WriteBatch.
pub fn commit_block(store: &RocksStore, commit: &BlockCommit) -> Result<()> {
    if commit.block.tx_hashes.len() != commit.tx_jsons.len() {
        return Err(PlatariumError::State(
            "tx_hashes and tx_jsons length mismatch".into(),
        ));
    }
    if commit.block.state_root != commit.state_root {
        return Err(PlatariumError::State(
            "block.state_root != commit.state_root".into(),
        ));
    }

    let height = commit.block.height;
    let current_head = store.head_height()?;
    let expected = if current_head == 0 { 1 } else { current_head + 1 };
    if height != expected {
        return Err(PlatariumError::State(format!(
            "invalid block height: head={}, expected={}, got={}",
            current_head, expected, height
        )));
    }

    let mut batch = WriteBatch::default();

    let block_bytes = serde_json::to_vec(&commit.block)
        .map_err(|e| PlatariumError::State(format!("encode block: {}", e)))?;
    batch.put(key_block(height), block_bytes);
    batch.put(key_state_root(height), commit.state_root.as_bytes());
    batch.put(KEY_META_HEAD, encode_u64(height));

    for (i, (hash, tx_json)) in commit
        .block
        .tx_hashes
        .iter()
        .zip(commit.tx_jsons.iter())
        .enumerate()
    {
        batch.put(key_tx(hash), tx_json.as_bytes());
        batch.put(key_idx_block(height, i as u32), hash.as_bytes());

        // Index by from/to if present in JSON.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(tx_json) {
            if let Some(from) = v.get("from").and_then(|x| x.as_str()) {
                batch.put(key_idx_addr(from, height, hash), b"1");
            }
            if let Some(to) = v.get("to").and_then(|x| x.as_str()) {
                batch.put(key_idx_addr(to, height, hash), b"1");
            }
        }
    }

    for acct in &commit.accounts {
        let bytes = serde_json::to_vec(acct)
            .map_err(|e| PlatariumError::State(format!("encode account: {}", e)))?;
        batch.put(key_account(&acct.address), bytes);
    }

    for receipt in &commit.receipts {
        let bytes = serde_json::to_vec(receipt)
            .map_err(|e| PlatariumError::State(format!("encode receipt: {}", e)))?;
        batch.put(key_receipt(&receipt.tx_hash), bytes);
    }

    store.write_batch(batch)?;
    create_snapshot_if_due(store, height)?;
    Ok(())
}

/// Build a WriteBatch without writing (for crash-simulation tests).
pub fn build_commit_batch(commit: &BlockCommit) -> Result<WriteBatch> {
    let height = commit.block.height;
    let mut batch = WriteBatch::default();
    let block_bytes = serde_json::to_vec(&commit.block)
        .map_err(|e| PlatariumError::State(format!("encode block: {}", e)))?;
    batch.put(key_block(height), block_bytes);
    batch.put(key_state_root(height), commit.state_root.as_bytes());
    batch.put(KEY_META_HEAD, encode_u64(height));
    for (i, (hash, tx_json)) in commit
        .block
        .tx_hashes
        .iter()
        .zip(commit.tx_jsons.iter())
        .enumerate()
    {
        batch.put(key_tx(hash), tx_json.as_bytes());
        batch.put(key_idx_block(height, i as u32), hash.as_bytes());
    }
    for acct in &commit.accounts {
        let bytes = serde_json::to_vec(acct)
            .map_err(|e| PlatariumError::State(format!("encode account: {}", e)))?;
        batch.put(key_account(&acct.address), bytes);
    }
    for receipt in &commit.receipts {
        let bytes = serde_json::to_vec(receipt)
            .map_err(|e| PlatariumError::State(format!("encode receipt: {}", e)))?;
        batch.put(key_receipt(&receipt.tx_hash), bytes);
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::query::{get_account, get_block, get_head, get_tx};
    use tempfile::TempDir;

    fn sample_commit(height: u64) -> BlockCommit {
        BlockCommit {
            block: BlockRecordStored {
                height,
                previous_hash: "0".into(),
                timestamp: 1,
                tx_hashes: vec!["aabb".into()],
                merkle_root: "m".into(),
                state_root: "root1".into(),
                block_hash: "bh1".into(),
                producer_id: "n1".into(),
            },
            tx_jsons: vec![
                r#"{"hash":"aabb","from":"PxA","to":"PxB","asset":"PLP","amount":10,"fee_uplp":1,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
            ],
            accounts: vec![
                AccountRecord {
                    address: "PxA".into(),
                    balance: "90".into(),
                    uplp_balance: "0".into(),
                    nonce: 1,
                },
                AccountRecord {
                    address: "PxB".into(),
                    balance: "10".into(),
                    uplp_balance: "0".into(),
                    nonce: 0,
                },
            ],
            receipts: vec![ReceiptRecord {
                tx_hash: "aabb".into(),
                status: "ok".into(),
                fee_uplp: 1,
                block_height: height,
            }],
            state_root: "root1".into(),
        }
    }

    #[test]
    fn commit_and_reopen() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        commit_block(&store, &sample_commit(1)).unwrap();
        assert_eq!(get_head(&store).unwrap(), 1);
        let store = store.reopen().unwrap();
        assert_eq!(get_head(&store).unwrap(), 1);
        assert!(get_tx(&store, "aabb").unwrap().is_some());
        assert!(get_block(&store, 1).unwrap().is_some());
        let a = get_account(&store, "PxA").unwrap().unwrap();
        assert_eq!(a.balance, "90");
        assert_eq!(a.nonce, 1);
    }

    #[test]
    fn crash_before_write_leaves_head_zero() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        let _batch = build_commit_batch(&sample_commit(1)).unwrap();
        // Intentionally do not write — simulates crash before commit.
        assert_eq!(store.head_height().unwrap(), 0);
        assert!(get_tx(&store, "aabb").unwrap().is_none());
    }
}
