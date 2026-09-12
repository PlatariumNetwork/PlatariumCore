//! Atomic block commit via RocksDB WriteBatch.
//!
//! R2-H1: RPC must not accept arbitrary BlockCommit payloads with only the shared
//! Gateway token. Serve-path commits go through verified execution (`block_cycle`
//! after `apply_txs`) or an explicit admin ALLOW + admin token escape hatch.
//! See [`assert_commit_allowed_after_execution`] and `rpc_security`.

use crate::error::{PlatariumError, Result};
use crate::storage::rocks::RocksStore;
use crate::storage::schema::{
    KEY_META_HEAD, encode_u64, key_account, key_block, key_idx_addr, key_idx_block, key_receipt,
    key_state_root, key_tx,
};
use crate::storage::snapshot::create_snapshot_if_due;
use rocksdb::WriteBatch;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn default_xp() -> String {
    "0".into()
}

/// Gate for Rocks commits attached to a verified execution path (R2-H1).
///
/// Accepts when `apply_txs` ran and `commit.state_root` matches `executed_state_root`,
/// or when `external_allowed` (recovery ALLOW flag) is set.
pub fn assert_commit_allowed_after_execution(
    apply_txs: bool,
    executed_state_root: Option<&str>,
    commit_json: &str,
    external_allowed: bool,
) -> Result<()> {
    if external_allowed {
        return Ok(());
    }
    if !apply_txs {
        return Err(PlatariumError::State(
            "rocks commit requires verified execution: set apply_txs=true (or PLATARIUM_CORE_ALLOW_EXTERNAL_ROCKS_COMMIT=1)"
                .into(),
        ));
    }
    let commit: BlockCommit = serde_json::from_str(commit_json).map_err(|e| {
        PlatariumError::State(format!("invalid BlockCommit JSON for verified gate: {}", e))
    })?;
    match executed_state_root {
        Some(root) if root == commit.state_root && root == commit.block.state_root => Ok(()),
        Some(_) => Err(PlatariumError::State(
            "rocks commit rejected: commit state_root does not match verified execution root".into(),
        )),
        None => Err(PlatariumError::State(
            "rocks commit rejected: missing verified execution state_root".into(),
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRecord {
    pub address: String,
    pub balance: String,
    pub uplp_balance: String,
    pub nonce: u64,
    /// Non-PLP token balances (canonical asset key → decimal string). Sorted via BTreeMap.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
    /// Contributor XP balance as decimal string (`Token:XP`, default `"0"`).
    #[serde(default = "default_xp")]
    pub xp: String,
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
    use crate::storage::schema::key_account;
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
                    tokens: BTreeMap::new(),
                    xp: "0".into(),
                },
                AccountRecord {
                    address: "PxB".into(),
                    balance: "10".into(),
                    uplp_balance: "0".into(),
                    nonce: 0,
                    tokens: BTreeMap::new(),
                    xp: "0".into(),
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

    #[test]
    fn verified_execution_gate_rejects_unapplied_commit() {
        let json = serde_json::to_string(&sample_commit(1)).unwrap();
        let err = assert_commit_allowed_after_execution(false, Some("root1"), &json, false)
            .unwrap_err();
        assert!(
            err.to_string().contains("verified execution")
                || err.to_string().contains("apply_txs"),
            "{err}"
        );
    }

    #[test]
    fn verified_execution_gate_accepts_matching_root() {
        let json = serde_json::to_string(&sample_commit(1)).unwrap();
        assert!(assert_commit_allowed_after_execution(true, Some("root1"), &json, false).is_ok());
        let err = assert_commit_allowed_after_execution(true, Some("other"), &json, false)
            .unwrap_err();
        assert!(err.to_string().contains("state_root"), "{err}");
        assert!(assert_commit_allowed_after_execution(false, None, &json, true).is_ok());
    }

    #[test]
    fn account_record_serializes_tokens_and_xp() {
        let mut tokens = BTreeMap::new();
        tokens.insert("Token:XP".into(), "150".into());
        tokens.insert("Token:USDT".into(), "42".into());
        let rec = AccountRecord {
            address: "PxA".into(),
            balance: "100".into(),
            uplp_balance: "5".into(),
            nonce: 2,
            tokens,
            xp: "150".into(),
        };
        let json = serde_json::to_value(&rec).unwrap();
        assert_eq!(json["xp"], "150");
        assert_eq!(json["tokens"]["Token:XP"], "150");
        assert_eq!(json["tokens"]["Token:USDT"], "42");

        let round: AccountRecord = serde_json::from_value(json).unwrap();
        assert_eq!(round.xp, "150");
        assert_eq!(
            round.tokens.get("Token:XP").map(String::as_str),
            Some("150")
        );
        assert_eq!(
            round.tokens.get("Token:USDT").map(String::as_str),
            Some("42")
        );
    }

    /// Issue #32: old-shape Rocks JSON without tokens/xp must load with defaults
    /// and must not wipe balance/nonce.
    #[test]
    fn legacy_account_record_deserializes_preserving_balance_nonce() {
        const LEGACY: &str =
            r#"{"address":"PxLegacy","balance":"9876543210","uplp_balance":"42","nonce":7}"#;
        let legacy: AccountRecord = serde_json::from_str(LEGACY).unwrap();
        assert_eq!(legacy.address, "PxLegacy");
        assert_eq!(legacy.balance, "9876543210");
        assert_eq!(legacy.uplp_balance, "42");
        assert_eq!(legacy.nonce, 7);
        assert!(legacy.tokens.is_empty());
        assert_eq!(legacy.xp, "0");

        // Round-trip through Rocks get path: put raw legacy bytes, read back.
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        store
            .put(&key_account("PxLegacy"), LEGACY.as_bytes())
            .unwrap();
        let loaded = get_account(&store, "PxLegacy").unwrap().unwrap();
        assert_eq!(loaded.balance, "9876543210");
        assert_eq!(loaded.uplp_balance, "42");
        assert_eq!(loaded.nonce, 7);
        assert!(loaded.tokens.is_empty());
        assert_eq!(loaded.xp, "0");
    }
}
