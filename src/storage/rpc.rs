//! JSON CLI/RPC wrappers for RocksDB storage.

use crate::error::{PlatariumError, Result};
use crate::storage::commit::{BlockCommit, commit_block};
use crate::storage::query::{
    get_account, get_block, get_head, get_receipt, get_state_root, get_tx, head_meta_json,
    list_tx_hashes_for_address,
};
use crate::storage::rocks::RocksStore;
use crate::storage::snapshot::{bootstrap_from_snapshot, get_snapshot, list_snapshots};
use std::path::Path;

fn open(path: &str) -> Result<RocksStore> {
    RocksStore::open(Path::new(path))
}

pub fn rocks_get_head_json(db_path: &str) -> Result<String> {
    let store = open(db_path)?;
    head_meta_json(&store)
}

pub fn rocks_get_tx_json(db_path: &str, tx_hash: &str) -> Result<String> {
    let store = open(db_path)?;
    match get_tx(&store, tx_hash)? {
        Some(tx) => Ok(serde_json::json!({"found": true, "tx": serde_json::from_str::<serde_json::Value>(&tx).unwrap_or(serde_json::Value::String(tx))}).to_string()),
        None => Ok(serde_json::json!({"found": false}).to_string()),
    }
}

pub fn rocks_get_block_json(db_path: &str, height: u64) -> Result<String> {
    let store = open(db_path)?;
    match get_block(&store, height)? {
        Some(b) => Ok(serde_json::to_string(&serde_json::json!({"found": true, "block": b})).unwrap()),
        None => Ok(serde_json::json!({"found": false}).to_string()),
    }
}

pub fn rocks_get_account_json(db_path: &str, address: &str) -> Result<String> {
    let store = open(db_path)?;
    match get_account(&store, address)? {
        Some(a) => Ok(serde_json::to_string(&serde_json::json!({"found": true, "account": a})).unwrap()),
        None => Ok(serde_json::json!({"found": false}).to_string()),
    }
}

pub fn rocks_get_receipt_json(db_path: &str, tx_hash: &str) -> Result<String> {
    let store = open(db_path)?;
    match get_receipt(&store, tx_hash)? {
        Some(r) => Ok(serde_json::to_string(&serde_json::json!({"found": true, "receipt": r})).unwrap()),
        None => Ok(serde_json::json!({"found": false}).to_string()),
    }
}

pub fn rocks_list_address_txs_json(db_path: &str, address: &str) -> Result<String> {
    let store = open(db_path)?;
    let hashes = list_tx_hashes_for_address(&store, address)?;
    Ok(serde_json::json!({"address": address, "tx_hashes": hashes}).to_string())
}

pub fn rocks_commit_block_json(db_path: &str, commit_json: &str) -> Result<String> {
    let store = open(db_path)?;
    let commit: BlockCommit = serde_json::from_str(commit_json)
        .map_err(|e| PlatariumError::State(format!("invalid BlockCommit JSON: {}", e)))?;
    commit_block(&store, &commit)?;
    Ok(serde_json::json!({"ok": true, "height": commit.block.height}).to_string())
}

pub fn rocks_list_snapshots_json(db_path: &str) -> Result<String> {
    let store = open(db_path)?;
    let snaps = list_snapshots(&store)?;
    // Omit full account dumps in list for size.
    let thin: Vec<_> = snaps
        .iter()
        .map(|s| {
            serde_json::json!({
                "height": s.height,
                "state_root": s.state_root,
                "account_count": s.account_count,
            })
        })
        .collect();
    Ok(serde_json::json!({"snapshots": thin}).to_string())
}

pub fn rocks_get_snapshot_json(db_path: &str, height: u64) -> Result<String> {
    let store = open(db_path)?;
    match get_snapshot(&store, height)? {
        Some(s) => Ok(serde_json::to_string(&serde_json::json!({"found": true, "snapshot": s})).unwrap()),
        None => Ok(serde_json::json!({"found": false}).to_string()),
    }
}

pub fn rocks_bootstrap_snapshot_json(db_path: &str, snapshot_json: &str) -> Result<String> {
    let store = open(db_path)?;
    let meta = serde_json::from_str(snapshot_json)
        .map_err(|e| PlatariumError::State(format!("invalid snapshot JSON: {}", e)))?;
    bootstrap_from_snapshot(&store, &meta)?;
    Ok(serde_json::json!({"ok": true, "head": get_head(&store)?}).to_string())
}

pub fn rocks_get_state_root_json(db_path: &str, height: u64) -> Result<String> {
    let store = open(db_path)?;
    match get_state_root(&store, height)? {
        Some(r) => Ok(serde_json::json!({"found": true, "state_root": r}).to_string()),
        None => Ok(serde_json::json!({"found": false}).to_string()),
    }
}

/// One-shot import: JSON chain file (Gateway ChainFileData) + optional state into RocksDB.
pub fn migrate_json_to_rocks(
    db_path: &str,
    chain_json: &str,
    state_accounts_json: Option<&str>,
) -> Result<String> {
    let store = open(db_path)?;
    let chain: serde_json::Value = serde_json::from_str(chain_json)
        .map_err(|e| PlatariumError::State(format!("invalid chain JSON: {}", e)))?;

    if let Some(accounts_json) = state_accounts_json {
        let accounts: Vec<crate::storage::commit::AccountRecord> =
            serde_json::from_str(accounts_json)
                .map_err(|e| PlatariumError::State(format!("invalid accounts JSON: {}", e)))?;
        let mut batch = rocksdb::WriteBatch::default();
        for acct in &accounts {
            let bytes = serde_json::to_vec(acct)
                .map_err(|e| PlatariumError::State(format!("encode account: {}", e)))?;
            batch.put(crate::storage::schema::key_account(&acct.address), bytes);
        }
        store.write_batch(batch)?;
    }

    let blocks = chain
        .get("blocks")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();
    let txs = chain
        .get("transactions")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tx_by_hash = std::collections::HashMap::new();
    for t in &txs {
        if let Some(h) = t.get("hash").and_then(|x| x.as_str()) {
            tx_by_hash.insert(h.to_string(), t.clone());
        }
    }

    let mut imported = 0u64;
    for b in blocks {
        // Gateway blockNumber is 0-based; Core RocksDB height is 1-based.
        let height = if let Some(h) = b.get("height").and_then(|x| x.as_u64()) {
            h
        } else if let Some(bn) = b.get("blockNumber").and_then(|x| x.as_u64()) {
            bn + 1
        } else {
            0
        };
        if height == 0 {
            continue;
        }
        let tx_hashes: Vec<String> = b
            .get("txHashes")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let tx_jsons: Vec<String> = tx_hashes
            .iter()
            .map(|h| {
                tx_by_hash
                    .get(h)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| format!(r#"{{"hash":"{}"}}"#, h))
            })
            .collect();
        let state_root = b
            .get("stateRoot")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let commit = BlockCommit {
            block: crate::storage::commit::BlockRecordStored {
                height,
                previous_hash: b
                    .get("previousHash")
                    .and_then(|x| x.as_str())
                    .unwrap_or("0")
                    .to_string(),
                timestamp: b.get("timestamp").and_then(|x| x.as_i64()).unwrap_or(0),
                tx_hashes: tx_hashes.clone(),
                merkle_root: b
                    .get("merkleRoot")
                    .and_then(|x| x.as_str())
                    .unwrap_or("0")
                    .to_string(),
                state_root: state_root.clone(),
                block_hash: b
                    .get("blockHash")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                producer_id: b
                    .get("producerNodeId")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            },
            tx_jsons,
            accounts: vec![],
            receipts: tx_hashes
                .iter()
                .map(|h| crate::storage::commit::ReceiptRecord {
                    tx_hash: h.clone(),
                    status: "ok".into(),
                    fee_uplp: 0,
                    block_height: height,
                })
                .collect(),
            state_root,
        };
        // Bypass sequential height check for migration by writing batch directly when needed.
        if store.head_height()? + 1 != height && !(store.head_height()? == 0 && height == 1) {
            // Force head to height-1 for migration continuity.
            store.put(
                crate::storage::schema::KEY_META_HEAD,
                &crate::storage::schema::encode_u64(height.saturating_sub(1)),
            )?;
        }
        commit_block(&store, &commit)?;
        imported += 1;
    }

    Ok(serde_json::json!({
        "ok": true,
        "blocks_imported": imported,
        "head": get_head(&store)?,
    })
    .to_string())
}
