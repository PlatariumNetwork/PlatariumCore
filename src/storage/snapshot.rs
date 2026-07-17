//! Periodic state snapshots for fast node bootstrap (every SNAPSHOT_INTERVAL blocks).

use crate::error::{PlatariumError, Result};
use crate::storage::commit::AccountRecord;
use crate::storage::rocks::RocksStore;
use crate::storage::schema::{PREFIX_ACCOUNT, key_snapshot};
use rocksdb::WriteBatch;
use serde::{Deserialize, Serialize};

/// Consensus constant: create a snapshot every N blocks.
pub const SNAPSHOT_INTERVAL: u64 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub height: u64,
    pub state_root: String,
    pub account_count: u64,
    /// Embedded account records for bootstrap (compact dump).
    pub accounts: Vec<AccountRecord>,
}

/// If `height` is a multiple of SNAPSHOT_INTERVAL (>0), persist snapshot meta + accounts.
pub fn create_snapshot_if_due(store: &RocksStore, height: u64) -> Result<Option<SnapshotMeta>> {
    if height == 0 || height % SNAPSHOT_INTERVAL != 0 {
        return Ok(None);
    }
    let state_root = store
        .get(&crate::storage::schema::key_state_root(height))?
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();

    let mut accounts = Vec::new();
    let iter = store.db().prefix_iterator(PREFIX_ACCOUNT);
    for item in iter {
        let (key, value) = item.map_err(|e| PlatariumError::State(format!("iter: {}", e)))?;
        if !key.starts_with(PREFIX_ACCOUNT) {
            break;
        }
        let acct: AccountRecord = serde_json::from_slice(&value)
            .map_err(|e| PlatariumError::State(format!("decode account: {}", e)))?;
        accounts.push(acct);
    }
    accounts.sort_by(|a, b| a.address.cmp(&b.address));

    let meta = SnapshotMeta {
        height,
        state_root,
        account_count: accounts.len() as u64,
        accounts,
    };
    let bytes = serde_json::to_vec(&meta)
        .map_err(|e| PlatariumError::State(format!("encode snapshot: {}", e)))?;
    let mut batch = WriteBatch::default();
    batch.put(key_snapshot(height), bytes);
    store.write_batch(batch)?;
    Ok(Some(meta))
}

pub fn list_snapshots(store: &RocksStore) -> Result<Vec<SnapshotMeta>> {
    use crate::storage::schema::PREFIX_SNAPSHOT;
    let mut out = Vec::new();
    let iter = store.db().prefix_iterator(PREFIX_SNAPSHOT);
    for item in iter {
        let (key, value) = item.map_err(|e| PlatariumError::State(format!("iter: {}", e)))?;
        if !key.starts_with(PREFIX_SNAPSHOT) {
            break;
        }
        let meta: SnapshotMeta = serde_json::from_slice(&value)
            .map_err(|e| PlatariumError::State(format!("decode snapshot: {}", e)))?;
        out.push(meta);
    }
    out.sort_by_key(|m| m.height);
    Ok(out)
}

pub fn get_snapshot(store: &RocksStore, height: u64) -> Result<Option<SnapshotMeta>> {
    match store.get(&key_snapshot(height))? {
        Some(bytes) => {
            let m: SnapshotMeta = serde_json::from_slice(&bytes)
                .map_err(|e| PlatariumError::State(format!("decode snapshot: {}", e)))?;
            Ok(Some(m))
        }
        None => Ok(None),
    }
}

/// Load accounts from a snapshot into an empty (or existing) store and set head metadata lightly.
/// Catch-up blocks after snapshot height must be applied separately via commit_block.
pub fn bootstrap_from_snapshot(store: &RocksStore, meta: &SnapshotMeta) -> Result<()> {
    let mut batch = WriteBatch::default();
    for acct in &meta.accounts {
        let bytes = serde_json::to_vec(acct)
            .map_err(|e| PlatariumError::State(format!("encode account: {}", e)))?;
        batch.put(crate::storage::schema::key_account(&acct.address), bytes);
    }
    batch.put(
        crate::storage::schema::KEY_META_HEAD,
        crate::storage::schema::encode_u64(meta.height),
    );
    batch.put(
        crate::storage::schema::key_state_root(meta.height),
        meta.state_root.as_bytes(),
    );
    store.write_batch(batch)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::commit::{AccountRecord, BlockCommit, BlockRecordStored, commit_block};
    use tempfile::TempDir;

    #[test]
    fn snapshot_at_interval() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        // Force interval check with a custom height by writing state root first.
        store
            .put(
                &crate::storage::schema::key_state_root(SNAPSHOT_INTERVAL),
                b"rootX",
            )
            .unwrap();
        store
            .put(
                &crate::storage::schema::key_account("PxA"),
                &serde_json::to_vec(&AccountRecord {
                    address: "PxA".into(),
                    balance: "1".into(),
                    uplp_balance: "0".into(),
                    nonce: 0,
                })
                .unwrap(),
            )
            .unwrap();
        let snap = create_snapshot_if_due(&store, SNAPSHOT_INTERVAL)
            .unwrap()
            .expect("snapshot");
        assert_eq!(snap.height, SNAPSHOT_INTERVAL);
        assert_eq!(snap.account_count, 1);

        let list = list_snapshots(&store).unwrap();
        assert_eq!(list.len(), 1);

        let dir2 = TempDir::new().unwrap();
        let store2 = RocksStore::open(dir2.path().join("db")).unwrap();
        bootstrap_from_snapshot(&store2, &snap).unwrap();
        assert_eq!(store2.head_height().unwrap(), SNAPSHOT_INTERVAL);
    }

    #[test]
    fn no_snapshot_off_interval() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        let commit = BlockCommit {
            block: BlockRecordStored {
                height: 1,
                previous_hash: "0".into(),
                timestamp: 1,
                tx_hashes: vec![],
                merkle_root: "0".into(),
                state_root: "r".into(),
                block_hash: "h".into(),
                producer_id: "p".into(),
            },
            tx_jsons: vec![],
            accounts: vec![],
            receipts: vec![],
            state_root: "r".into(),
        };
        commit_block(&store, &commit).unwrap();
        assert!(list_snapshots(&store).unwrap().is_empty());
    }
}
