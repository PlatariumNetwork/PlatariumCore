//! CLI helpers for consensus wiring (L1 verify, vote aggregation, block assembly).

use crate::core::block_assembly::{assemble_block, process_l2_block_votes, BlockConfirmationResult};
use crate::core::confirmation_layer::{process_l1_confirmation, verify_tx_for_l1, ConfirmationResult, Vote};
use crate::core::state_file::load_state_file;
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use std::path::Path;

/// Verify all transactions for L1 against current state (signature, fee, balance, nonce).
/// Returns per-transaction results in `tx_results` for multi-TX blocks.
pub fn l1_verify_txs_json(path: &Path, txs_json: &str) -> Result<String> {
    let state = load_state_file(path)?;
    let txs: Vec<String> = serde_json::from_str(txs_json)
        .map_err(|e| PlatariumError::State(format!("invalid txs JSON array: {}", e)))?;
    let mut tx_results = Vec::new();
    for tx_json in &txs {
        let tx = Transaction::from_gateway_json(tx_json)?;
        let valid = verify_tx_for_l1(&state, &tx)?;
        tx_results.push(serde_json::json!({
            "hash": tx.hash,
            "valid": valid,
        }));
        if !valid {
            return Ok(serde_json::json!({
                "valid": false,
                "error": format!("L1 verification failed for tx {}", tx.hash),
                "tx_results": tx_results,
            })
            .to_string());
        }
    }
    Ok(serde_json::json!({
        "valid": true,
        "tx_results": tx_results,
    })
    .to_string())
}

/// Aggregate L1 votes. Input: JSON array of {"node_id":"...","yes":true|false}.
pub fn l1_process_votes_json(votes_json: &str) -> Result<String> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(votes_json)
        .map_err(|e| PlatariumError::State(format!("invalid votes JSON: {}", e)))?;
    let mut votes: Vec<(String, Vote)> = Vec::new();
    for v in raw {
        let id = v
            .get("node_id")
            .or_else(|| v.get("nodeId"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| PlatariumError::State("vote missing node_id".into()))?
            .to_string();
        let yes = v
            .get("yes")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        votes.push((id, if yes { Vote::Confirm } else { Vote::Reject }));
    }
    match process_l1_confirmation(&votes) {
        Ok((result, to_penalize)) => Ok(serde_json::json!({
            "confirmed": result == ConfirmationResult::Confirmed,
            "to_penalize": to_penalize,
        })
        .to_string()),
        Err(e) => Ok(serde_json::json!({
            "confirmed": false,
            "error": e.to_string(),
        })
        .to_string()),
    }
}

/// Aggregate L2 block votes. Input: same shape as L1 votes.
pub fn l2_process_votes_json(votes_json: &str) -> Result<String> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(votes_json)
        .map_err(|e| PlatariumError::State(format!("invalid votes JSON: {}", e)))?;
    let mut votes: Vec<(String, Vote)> = Vec::new();
    for v in raw {
        let id = v
            .get("node_id")
            .or_else(|| v.get("nodeId"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| PlatariumError::State("vote missing node_id".into()))?
            .to_string();
        let yes = v
            .get("yes")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        votes.push((id, if yes { Vote::Confirm } else { Vote::Reject }));
    }
    match process_l2_block_votes(&votes) {
        Ok((result, to_penalize)) => Ok(serde_json::json!({
            "confirmed": result == BlockConfirmationResult::Confirmed,
            "to_penalize": to_penalize,
        })
        .to_string()),
        Err(e) => Ok(serde_json::json!({
            "confirmed": false,
            "error": e.to_string(),
        })
        .to_string()),
    }
}

/// Assemble a Core block header from state file and transaction hashes.
pub fn assemble_block_json(
    path: &Path,
    block_number: u64,
    previous_hash: &str,
    timestamp: i64,
    tx_hashes_json: &str,
    producer_id: &str,
) -> Result<String> {
    let state = load_state_file(path)?;
    let tx_hashes: Vec<String> = serde_json::from_str(tx_hashes_json)
        .map_err(|e| PlatariumError::State(format!("invalid tx_hashes JSON: {}", e)))?;
    let snapshot = state.create_snapshot();
    let block = assemble_block(
        block_number,
        previous_hash.to_string(),
        timestamp,
        tx_hashes,
        &snapshot,
        producer_id.to_string(),
        String::new(),
    );
    Ok(serde_json::json!({
        "block_number": block.block_number,
        "previous_hash": block.previous_hash,
        "timestamp": block.timestamp,
        "transaction_hashes": block.transaction_hashes,
        "merkle_root": block.merkle_root,
        "state_root": block.state_root,
        "block_hash": block.block_hash,
        "producer_id": block.producer_id,
    })
    .to_string())
}
