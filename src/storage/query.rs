//! Read APIs over RocksDB.

use crate::error::{PlatariumError, Result};
use crate::storage::commit::{AccountRecord, BlockRecordStored, ReceiptRecord};
use crate::storage::rocks::RocksStore;
use crate::storage::schema::{
    KEY_META_HEAD, PREFIX_IDX_ADDR, decode_u64, key_account, key_block, key_receipt, key_state_root,
    key_tx,
};

pub fn get_head(store: &RocksStore) -> Result<u64> {
    store.head_height()
}

pub fn get_tx(store: &RocksStore, tx_hash: &str) -> Result<Option<String>> {
    Ok(store
        .get(&key_tx(tx_hash))?
        .map(|b| String::from_utf8_lossy(&b).into_owned()))
}

pub fn get_block(store: &RocksStore, height: u64) -> Result<Option<BlockRecordStored>> {
    match store.get(&key_block(height))? {
        Some(bytes) => {
            let b: BlockRecordStored = serde_json::from_slice(&bytes)
                .map_err(|e| PlatariumError::State(format!("decode block: {}", e)))?;
            Ok(Some(b))
        }
        None => Ok(None),
    }
}

pub fn get_account(store: &RocksStore, address: &str) -> Result<Option<AccountRecord>> {
    match store.get(&key_account(address))? {
        Some(bytes) => {
            let a: AccountRecord = serde_json::from_slice(&bytes)
                .map_err(|e| PlatariumError::State(format!("decode account: {}", e)))?;
            Ok(Some(a))
        }
        None => Ok(None),
    }
}

pub fn get_receipt(store: &RocksStore, tx_hash: &str) -> Result<Option<ReceiptRecord>> {
    match store.get(&key_receipt(tx_hash))? {
        Some(bytes) => {
            let r: ReceiptRecord = serde_json::from_slice(&bytes)
                .map_err(|e| PlatariumError::State(format!("decode receipt: {}", e)))?;
            Ok(Some(r))
        }
        None => Ok(None),
    }
}

pub fn get_state_root(store: &RocksStore, height: u64) -> Result<Option<String>> {
    Ok(store
        .get(&key_state_root(height))?
        .map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// List tx hashes for an address from the index (unordered by scan; sorted for determinism).
pub fn list_tx_hashes_for_address(store: &RocksStore, address: &str) -> Result<Vec<String>> {
    let mut prefix = PREFIX_IDX_ADDR.to_vec();
    prefix.extend_from_slice(address.as_bytes());
    prefix.push(b'/');

    let mut hashes = Vec::new();
    let iter = store.db().prefix_iterator(&prefix);
    for item in iter {
        let (key, _) = item.map_err(|e| PlatariumError::State(format!("iter: {}", e)))?;
        if !key.starts_with(&prefix) {
            break;
        }
        // key: i/a/{addr}/{height_be}/{tx_hash}
        if let Some(pos) = key.iter().rposition(|&b| b == b'/') {
            let hash = String::from_utf8_lossy(&key[pos + 1..]).into_owned();
            if !hash.is_empty() {
                hashes.push(hash);
            }
        }
    }
    hashes.sort();
    hashes.dedup();
    Ok(hashes)
}

pub fn head_meta_json(store: &RocksStore) -> Result<String> {
    let head = match store.get(KEY_META_HEAD)? {
        Some(b) => decode_u64(&b).unwrap_or(0),
        None => 0,
    };
    Ok(serde_json::json!({ "head": head }).to_string())
}
