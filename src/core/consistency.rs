//! Ledger consistency diagnostics (issue #61).
//!
//! Read-only checks over RocksDB canonical tip — never mutates ledgers.

use crate::error::{PlatariumError, Result};
use crate::storage::cache::open_cached;
use crate::storage::query::{get_block, get_head, get_state_root};
use crate::storage::rocks::RocksStore;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Result of [`check_consistency`] / [`check_consistency_store`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsistencyReport {
    pub ok: bool,
    pub head: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl ConsistencyReport {
    fn ok_report(head: u64) -> Self {
        Self {
            ok: true,
            head,
            errors: Vec::new(),
        }
    }
}

/// Diagnostic consistency check for a RocksDB path (does not mutate).
pub fn check_consistency(db_path: impl AsRef<Path>) -> Result<ConsistencyReport> {
    let store = open_cached(db_path.as_ref())?;
    check_consistency_store(store.as_ref())
}

/// Same as [`check_consistency`] against an already-open store.
pub fn check_consistency_store(store: &RocksStore) -> Result<ConsistencyReport> {
    let head = get_head(store)?;
    if head == 0 {
        return Ok(ConsistencyReport::ok_report(0));
    }

    let mut errors = Vec::new();
    for h in 1..=head {
        match get_block(store, h)? {
            None => errors.push(format!("missing block at height {h}")),
            Some(block) => {
                if block.height != h {
                    errors.push(format!(
                        "block height mismatch at key {h}: record says {}",
                        block.height
                    ));
                }
                match get_state_root(store, h)? {
                    None => errors.push(format!("missing state_root at height {h}")),
                    Some(root) => {
                        if root != block.state_root {
                            errors.push(format!(
                                "state_root mismatch at height {h}: meta={root} block={}",
                                block.state_root
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(ConsistencyReport {
        ok: errors.is_empty(),
        head,
        errors,
    })
}

/// JSON wrapper for RPC/CLI (issue #61).
pub fn check_consistency_json(db_path: &str) -> Result<String> {
    let report = check_consistency(Path::new(db_path))?;
    serde_json::to_string(&report).map_err(|e| {
        PlatariumError::State(format!("encode consistency report: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::commit::{
        commit_block, AccountRecord, BlockCommit, BlockRecordStored, ReceiptRecord,
    };
    use crate::storage::cache::evict_cached;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn sample_commit(height: u64, state_root: &str, block_hash: &str) -> BlockCommit {
        BlockCommit {
            block: BlockRecordStored {
                height,
                previous_hash: "0".into(),
                timestamp: 1,
                tx_hashes: vec!["aabb".into()],
                merkle_root: "m".into(),
                state_root: state_root.into(),
                block_hash: block_hash.into(),
                producer_id: "n1".into(),
            },
            tx_jsons: vec![
                r#"{"hash":"aabb","from":"PxA","to":"PxB","asset":"PLP","amount":10,"fee_uplp":1,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
            ],
            accounts: vec![AccountRecord {
                address: "PxA".into(),
                balance: "90".into(),
                uplp_balance: "0".into(),
                nonce: 1,
                tokens: BTreeMap::new(),
                xp: "0".into(),
            }],
            receipts: vec![ReceiptRecord {
                tx_hash: "aabb".into(),
                status: "ok".into(),
                fee_uplp: 1,
                block_height: height,
            }],
            state_root: state_root.into(),
        }
    }

    #[test]
    fn empty_db_is_consistent_and_does_not_mutate() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let before = check_consistency(&db_path).unwrap();
        assert!(before.ok);
        assert_eq!(before.head, 0);
        let after = check_consistency(&db_path).unwrap();
        assert_eq!(before, after);
        evict_cached(&db_path);
    }

    #[test]
    fn committed_chain_passes_consistency() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        {
            let store = crate::storage::rocks::RocksStore::open(&db_path).unwrap();
            commit_block(&store, &sample_commit(1, "root1", "bh1")).unwrap();
        }
        let report = check_consistency(&db_path).unwrap();
        assert!(report.ok, "{:?}", report.errors);
        assert_eq!(report.head, 1);
        evict_cached(&db_path);
    }
}
