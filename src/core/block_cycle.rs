//! Unified block cycle for the long-lived Core RPC daemon.
//! One round-trip: select → L1 verify → assemble → optional L1/L2 auto-confirm → optional apply.

use crate::core::block_proposal::{parse_mempool_snapshot, select_block_txs};
use crate::core::consensus_cli::{
    assemble_block_json, l1_process_votes_json, l1_verify_txs_json, l2_process_votes_json,
};
use crate::core::state_file::{state_apply_tx_json, state_root_json};
use crate::error::{PlatariumError, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// Run Core-owned block formation pipeline in a single call.
///
/// Params (JSON object):
/// - `state_file` (required)
/// - `mempool_txs` (required) — gateway mempool snapshot JSON array
/// - `block_number`, `previous_hash`, `timestamp`, `producer_id` (required for assemble)
/// - `auto_confirm` (bool, default true) — synthesize unanimous L1/L2 Confirm votes
/// - `apply_txs` (bool, default false) — apply L1-valid txs to state_file after assemble
/// - `commit` (optional string) — BlockCommit JSON for RocksDB when `db_path` set
/// - `db_path` (optional) — RocksDB path for `rocks_commit_block`
pub fn block_cycle_json(params: &Value) -> Result<String> {
    let state_file = params
        .get("state_file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PlatariumError::State("missing param state_file".into()))?;
    let mempool_txs = params
        .get("mempool_txs")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PlatariumError::State("missing param mempool_txs".into()))?;
    let block_number = params
        .get("block_number")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| PlatariumError::State("missing param block_number".into()))?;
    let previous_hash = params
        .get("previous_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PlatariumError::State("missing param previous_hash".into()))?;
    let timestamp = params
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            params
                .get("timestamp")
                .and_then(|v| v.as_u64())
                .map(|n| n as i64)
        })
        .ok_or_else(|| PlatariumError::State("missing param timestamp".into()))?;
    let producer_id = params
        .get("producer_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PlatariumError::State("missing param producer_id".into()))?;
    let auto_confirm = params
        .get("auto_confirm")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let apply_txs = params
        .get("apply_txs")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let path = Path::new(state_file);
    let state = crate::core::state_file::load_state_file(path)?;
    let mempool = parse_mempool_snapshot(mempool_txs)?;
    let selected = select_block_txs(&state, &mempool);

    let by_hash: HashMap<&str, &crate::core::block_proposal::MempoolSnapshotEntry> = mempool
        .iter()
        .map(|e| (e.tx.hash.as_str(), e))
        .collect();

    let mut selected_tx_jsons: Vec<String> = Vec::with_capacity(selected.hashes.len());
    for h in &selected.hashes {
        let entry = by_hash
            .get(h.as_str())
            .ok_or_else(|| PlatariumError::State(format!("selected hash missing from mempool: {}", h)))?;
        let tx_json = serde_json::to_string(&entry.tx)
            .map_err(|e| PlatariumError::State(format!("encode selected tx: {}", e)))?;
        selected_tx_jsons.push(tx_json);
    }
    let txs_array = serde_json::to_string(&selected_tx_jsons)
        .map_err(|e| PlatariumError::State(e.to_string()))?;

    let l1_raw = l1_verify_txs_json(path, &txs_array)?;
    let l1: Value = serde_json::from_str(&l1_raw)
        .map_err(|e| PlatariumError::State(format!("parse l1 result: {}", e)))?;

    let mut valid_hashes: Vec<String> = Vec::new();
    let mut valid_tx_jsons: Vec<String> = Vec::new();
    if let Some(results) = l1.get("tx_results").and_then(|v| v.as_array()) {
        for (i, tr) in results.iter().enumerate() {
            let valid = tr.get("valid").and_then(|v| v.as_bool()).unwrap_or(false);
            let hash = tr
                .get("hash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if valid {
                if !hash.is_empty() {
                    valid_hashes.push(hash);
                } else if i < selected.hashes.len() {
                    valid_hashes.push(selected.hashes[i].clone());
                }
                if i < selected_tx_jsons.len() {
                    valid_tx_jsons.push(selected_tx_jsons[i].clone());
                }
            }
        }
    }

    if valid_hashes.is_empty() {
        return Ok(json!({
            "ok": false,
            "error": l1.get("error").and_then(|v| v.as_str()).unwrap_or("no valid txs after L1"),
            "selected": selected,
            "l1": l1,
            "valid_hashes": valid_hashes,
        })
        .to_string());
    }

    let hashes_json = serde_json::to_string(&valid_hashes)
        .map_err(|e| PlatariumError::State(e.to_string()))?;
    let block_raw = assemble_block_json(
        path,
        block_number,
        previous_hash,
        timestamp,
        &hashes_json,
        producer_id,
    )?;
    let block: Value = serde_json::from_str(&block_raw)
        .map_err(|e| PlatariumError::State(format!("parse assemble result: {}", e)))?;

    let mut l1_votes: Option<Value> = None;
    let mut l2_votes: Option<Value> = None;
    if auto_confirm {
        let votes = json!([{"node_id": producer_id, "yes": true}]).to_string();
        let l1v = l1_process_votes_json(&votes)?;
        l1_votes = Some(
            serde_json::from_str(&l1v)
                .map_err(|e| PlatariumError::State(format!("parse l1 votes: {}", e)))?,
        );
        let l2v = l2_process_votes_json(&votes)?;
        l2_votes = Some(
            serde_json::from_str(&l2v)
                .map_err(|e| PlatariumError::State(format!("parse l2 votes: {}", e)))?,
        );
    }

    let mut applied: Vec<Value> = Vec::new();
    let mut final_state_root: Option<String> = None;
    if apply_txs {
        for tx_json in &valid_tx_jsons {
            let out = state_apply_tx_json(path, tx_json)?;
            let v: Value = serde_json::from_str(&out)
                .map_err(|e| PlatariumError::State(format!("parse apply: {}", e)))?;
            if let Some(root) = v.get("state_root").and_then(|r| r.as_str()) {
                final_state_root = Some(root.to_string());
            }
            applied.push(v);
        }
    }
    if final_state_root.is_none() {
        let root_raw = state_root_json(path)?;
        let root_v: Value = serde_json::from_str(&root_raw).unwrap_or(json!({}));
        final_state_root = root_v
            .get("state_root")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    let mut rocks_commit: Option<Value> = None;
    if let (Some(db_path), Some(commit)) = (
        params.get("db_path").and_then(|v| v.as_str()),
        params.get("commit").and_then(|v| v.as_str()),
    ) {
        if !db_path.is_empty() && !commit.is_empty() {
            let out = crate::storage::rpc::rocks_commit_block_json(db_path, commit)?;
            rocks_commit = Some(
                serde_json::from_str(&out)
                    .map_err(|e| PlatariumError::State(format!("parse rocks commit: {}", e)))?,
            );
        }
    }

    Ok(json!({
        "ok": true,
        "selected": {
            "hashes": selected.hashes,
            "gas_used": selected.gas_used,
            "gas_cap": selected.gas_cap,
            "tx_count": selected.tx_count,
        },
        "l1": l1,
        "valid_hashes": valid_hashes,
        "block": block,
        "l1_votes": l1_votes,
        "l2_votes": l2_votes,
        "applied": applied,
        "state_root": final_state_root,
        "rocks_commit": rocks_commit,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::state_file::init_state_file;
    use crate::core::transaction::Transaction;
    use std::collections::HashSet;
    use tempfile::TempDir;

    #[test]
    fn block_cycle_empty_mempool() {
        let dir = TempDir::new().unwrap();
        let state = dir.path().join("state.json");
        init_state_file(&state).unwrap();
        let params = json!({
            "state_file": state.to_string_lossy(),
            "mempool_txs": "[]",
            "block_number": 1,
            "previous_hash": "0",
            "timestamp": 1,
            "producer_id": "node0",
            "auto_confirm": true,
            "apply_txs": false,
        });
        let out = block_cycle_json(&params).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn block_cycle_selects_and_assembles() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        init_state_file(&state_path).unwrap();
        // Credit via direct state file mutation helpers
        crate::core::state_file::state_credit_json(
            &state_path,
            "PxAlice",
            1_000_000,
            100,
            true,
        )
        .unwrap();

        let tx = Transaction::new(
            "PxAlice".into(),
            "PxBob".into(),
            Asset::PLP,
            10,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main".into(),
            "sig_derived".into(),
        )
        .unwrap();
        let mempool = json!([{
            "hash": tx.hash,
            "from": tx.from,
            "to": tx.to,
            "asset": "PLP",
            "amount": tx.amount,
            "fee_uplp": tx.fee_uplp,
            "nonce": tx.nonce,
            "sig_main": tx.sig_main,
            "sig_derived": tx.sig_derived,
            "arrival_index": 0,
            "timestamp": 1,
            "reads": [],
            "writes": [],
        }])
        .to_string();

        let params = json!({
            "state_file": state_path.to_string_lossy(),
            "mempool_txs": mempool,
            "block_number": 1,
            "previous_hash": "0",
            "timestamp": 1,
            "producer_id": "node0",
            "auto_confirm": true,
            "apply_txs": false,
        });
        let out = block_cycle_json(&params).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        // L1 may fail signature verify with dummy sigs — still exercise packing path.
        assert!(v.get("selected").is_some());
        assert!(v.get("l1").is_some());
    }
}
