//! Block Assembly Logic & L2 Block Validator Voting (Module 4).
//!
//! **Validation Modules Analysis — Step 4:** Module that:
//! - **Forms a block** from a list of TX hashes: `assemble_block(block_number, previous_hash, timestamp, transaction_hashes, state_snapshot, producer_id, producer_sig)`.
//! - **L2 validators** are a separate group (from Step 2: `select_validators_l2` / `select_l1_l2_validators`).
//! - **L2 block voting** ≥70%: `process_l2_block_votes(votes)` → `(BlockConfirmationResult, to_penalize)`.
//! - **Finalize or reject block:** result is `BlockConfirmationResult::Confirmed` (finalize) or `Rejected` (reject); use `block_finalized(result)` to check.
//!
//! **Step 8 — Block Leader Rotation & BFT-style Finality:**
//! - **Leader rotation:** `block_leader_for_height(block_number, l2_validators)` returns the deterministic leader for that height (round-robin over the L2 set). The **leader proposes the block** (`producer_id` in `Block`).
//! - **L2 group** conducts **HotStuff-style voting:** validators vote Confirm/Reject on the proposed block; votes are aggregated with `process_l2_block_votes`.
//! - **Block is final** after **≥70%** Confirm votes (`L2_CONFIRM_THRESHOLD_PCT`). This provides **safety and deterministic finalization** (BFT-style finality).
//!
//! Assembles blocks with dynamic limits (max transactions, size, and time window 2–5 s) derived from mempool size, average TPS, and network load.
//! Block structure includes Merkle root, state root, block hash, and producer signature.

use sha2::{Sha256, Digest};
use crate::core::node_registry::{NodeId, NodeRegistry};
use crate::core::state::StateSnapshot;
use crate::error::{PlatariumError, Result};
use thiserror::Error;

/// L2 block confirmation threshold: at least this percentage of validators must vote Confirm (BFT-style finality: block is final after ≥70%).
pub const L2_CONFIRM_THRESHOLD_PCT: u64 = 70;

/// Block leader rotation (Step 8): returns the leader for the given height. Deterministic round-robin over `l2_validators` (must be in canonical order, e.g. from `select_validators_l2`). The leader proposes the block; L2 then votes (HotStuff-style); block is final after ≥70% votes.
#[inline]
pub fn block_leader_for_height(block_number: u64, l2_validators: &[NodeId]) -> Option<NodeId> {
    if l2_validators.is_empty() {
        return None;
    }
    let idx = (block_number as usize) % l2_validators.len();
    Some(l2_validators[idx].clone())
}

/// Returns the index of the block leader in the L2 validator list for the given height (for testing or display).
#[inline]
pub fn block_leader_index_for_height(block_number: u64, num_validators: usize) -> usize {
    if num_validators == 0 {
        return 0;
    }
    (block_number as usize) % num_validators
}

/// Block time bounds (seconds). Actual window is chosen dynamically in this range.
pub const BLOCK_TIME_MIN_SEC: u64 = 2;
pub const BLOCK_TIME_MAX_SEC: u64 = 5;

/// Default maximum transactions per block when load is low.
pub const DEFAULT_MAX_TXS_PER_BLOCK: usize = 500;
/// Default maximum block size in bytes when load is low.
pub const DEFAULT_MAX_BLOCK_SIZE: u64 = 256 * 1024;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum BlockAssemblyError {
    #[error("Block assembly error: {0}")]
    Other(String),
}

impl From<BlockAssemblyError> for PlatariumError {
    fn from(e: BlockAssemblyError) -> Self {
        PlatariumError::State(format!("BlockAssembly: {}", e))
    }
}

/// Block header and producer metadata. Transaction set is represented by hashes for Merkle root computation.
#[derive(Debug, Clone)]
pub struct Block {
    pub block_number: u64,
    pub previous_hash: String,
    pub timestamp: i64,
    pub transaction_hashes: Vec<String>,
    pub merkle_root: String,
    pub state_root: String,
    pub block_hash: String,
    pub producer_id: NodeId,
    pub producer_sig: String,
}

/// Computes the Merkle root from transaction hashes. Empty list yields "0". Deterministic: hashes are sorted before hashing.
pub fn compute_merkle_root(tx_hashes: &[String]) -> String {
    if tx_hashes.is_empty() {
        return "0".to_string();
    }
    let mut layer: Vec<[u8; 32]> = tx_hashes
        .iter()
        .filter_map(|h| hex::decode(h).ok())
        .filter_map(|v| v.try_into().ok())
        .collect();
    if layer.is_empty() {
        return "0".to_string();
    }
    layer.sort_by(|a, b| a.cmp(b));
    while layer.len() > 1 {
        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for chunk in layer.chunks(2) {
            let mut hasher = Sha256::new();
            hasher.update(&chunk[0]);
            if chunk.len() == 2 {
                hasher.update(&chunk[1]);
            } else {
                hasher.update(&chunk[0]);
            }
            let bytes = hasher.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            next.push(arr);
        }
        next.sort_by(|a, b| a.cmp(b));
        layer = next;
    }
    hex::encode(layer[0])
}

/// Computes the block hash from header fields (excluding producer signature). Deterministic.
fn compute_block_hash_inner(
    block_number: u64,
    previous_hash: &str,
    timestamp: i64,
    merkle_root: &str,
    state_root: &str,
    producer_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(block_number.to_le_bytes());
    hasher.update(previous_hash.as_bytes());
    hasher.update(timestamp.to_le_bytes());
    hasher.update(merkle_root.as_bytes());
    hasher.update(state_root.as_bytes());
    hasher.update(producer_id.as_bytes());
    hex::encode(hasher.finalize())
}

/// Returns the dynamic maximum number of transactions per block from mempool size, average TPS, and load.
pub fn max_transactions_per_block(
    mempool_size: usize,
    avg_tps: u64,
    _network_load_pct: u64,
) -> usize {
    if avg_tps == 0 {
        return DEFAULT_MAX_TXS_PER_BLOCK.min(mempool_size);
    }
    let cap = (avg_tps as usize).saturating_mul(BLOCK_TIME_MAX_SEC as usize);
    (DEFAULT_MAX_TXS_PER_BLOCK.min(cap)).min(mempool_size.max(1))
}

/// Returns the dynamic maximum block size in bytes; scales down with higher network load.
pub fn max_block_size_bytes(_mempool_size: usize, _avg_tps: u64, network_load_pct: u64) -> u64 {
    let base = DEFAULT_MAX_BLOCK_SIZE;
    if network_load_pct >= 80 {
        base / 2
    } else if network_load_pct >= 50 {
        (base * 3) / 4
    } else {
        base
    }
}

/// Returns the dynamic maximum block time in seconds (2–5 s). Higher load yields a shorter window.
pub fn max_block_time_sec(network_load_pct: u64) -> u64 {
    if network_load_pct >= 80 {
        BLOCK_TIME_MIN_SEC
    } else if network_load_pct >= 50 {
        (BLOCK_TIME_MIN_SEC + BLOCK_TIME_MAX_SEC) / 2
    } else {
        BLOCK_TIME_MAX_SEC
    }
}

/// Assembles a block with Merkle root, state root, and block hash. The producer must sign the block hash externally and set `producer_sig`.
pub fn assemble_block(
    block_number: u64,
    previous_hash: String,
    timestamp: i64,
    transaction_hashes: Vec<String>,
    state_snapshot: &StateSnapshot,
    producer_id: NodeId,
    producer_sig: String,
) -> Block {
    let merkle_root = compute_merkle_root(&transaction_hashes);
    let state_root = state_snapshot.compute_state_root();
    let block_hash = compute_block_hash_inner(
        block_number,
        &previous_hash,
        timestamp,
        &merkle_root,
        &state_root,
        &producer_id,
    );
    Block {
        block_number,
        previous_hash,
        timestamp,
        transaction_hashes,
        merkle_root,
        state_root,
        block_hash,
        producer_id,
        producer_sig,
    }
}

/// L2 block vote type (Confirm/Reject), re-exported from the confirmation layer.
pub use crate::core::confirmation_layer::Vote;

/// Outcome of L2 block confirmation: finalize (accept) or reject the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockConfirmationResult {
    /// Block is accepted; finalize it.
    Confirmed,
    /// Block is rejected; do not finalize.
    Rejected,
}

/// Returns true if the block should be finalized (accepted), false if rejected.
#[inline]
pub fn block_finalized(result: BlockConfirmationResult) -> bool {
    result == BlockConfirmationResult::Confirmed
}

/// Aggregates L2 block votes. If at least 70% vote Confirm, the block is confirmed; otherwise rejected. Returns the list of nodes to penalize (voted against the majority).
pub fn process_l2_block_votes(votes: &[(NodeId, Vote)]) -> Result<(BlockConfirmationResult, Vec<NodeId>)> {
    if votes.is_empty() {
        return Err(BlockAssemblyError::Other("No L2 votes".to_string()).into());
    }
    let total = votes.len() as u64;
    let confirm_count = votes
        .iter()
        .filter(|(_, v)| *v == Vote::Confirm)
        .count() as u64;

    let result = if (confirm_count * 100) >= (total * L2_CONFIRM_THRESHOLD_PCT) {
        BlockConfirmationResult::Confirmed
    } else {
        BlockConfirmationResult::Rejected
    };

    let majority = if confirm_count > total / 2 {
        Vote::Confirm
    } else {
        Vote::Reject
    };

    let to_penalize: Vec<NodeId> = votes
        .iter()
        .filter(|(_, v)| *v != majority)
        .map(|(id, _)| id.clone())
        .collect();

    Ok((result, to_penalize))
}

/// Applies L2 penalties to the given nodes (recorded as missed votes).
pub fn apply_l2_block_penalties(registry: &NodeRegistry, to_penalize: &[NodeId]) -> Result<()> {
    for node_id in to_penalize {
        registry.record_vote(node_id, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_empty() {
        assert_eq!(compute_merkle_root(&[]), "0");
    }

    #[test]
    fn test_merkle_one() {
        let h = hex::encode([1u8; 32]);
        assert_eq!(compute_merkle_root(&[h.clone()]), h);
    }

    #[test]
    fn test_block_hash_deterministic() {
        let h1 = compute_block_hash_inner(1, "prev", 1000, "merkle", "state", "producer");
        let h2 = compute_block_hash_inner(1, "prev", 1000, "merkle", "state", "producer");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_l2_threshold_70() {
        let votes: Vec<(NodeId, Vote)> = (0..10)
            .map(|i| (format!("n{}", i), if i < 7 { Vote::Confirm } else { Vote::Reject }))
            .collect();
        let (res, penalize) = process_l2_block_votes(&votes).unwrap();
        assert_eq!(res, BlockConfirmationResult::Confirmed);
        assert_eq!(penalize.len(), 3);
    }

    #[test]
    fn test_l2_below_70_rejected() {
        let votes: Vec<(NodeId, Vote)> = (0..10)
            .map(|i| (format!("n{}", i), if i < 7 { Vote::Reject } else { Vote::Confirm }))
            .collect();
        let (res, _) = process_l2_block_votes(&votes).unwrap();
        assert_eq!(res, BlockConfirmationResult::Rejected);
    }

    #[test]
    fn test_block_finalized() {
        assert!(block_finalized(BlockConfirmationResult::Confirmed));
        assert!(!block_finalized(BlockConfirmationResult::Rejected));
    }

    #[test]
    fn test_block_leader_rotation() {
        let l2: Vec<NodeId> = vec!["n0".into(), "n1".into(), "n2".into()];
        assert_eq!(block_leader_for_height(0, &l2).as_deref(), Some("n0"));
        assert_eq!(block_leader_for_height(1, &l2).as_deref(), Some("n1"));
        assert_eq!(block_leader_for_height(2, &l2).as_deref(), Some("n2"));
        assert_eq!(block_leader_for_height(3, &l2).as_deref(), Some("n0"));
        assert_eq!(block_leader_for_height(100, &l2).as_deref(), Some("n1"));
    }

    #[test]
    fn test_block_leader_empty() {
        let l2: Vec<NodeId> = vec![];
        assert!(block_leader_for_height(0, &l2).is_none());
    }

    #[test]
    fn test_block_leader_index_for_height() {
        assert_eq!(block_leader_index_for_height(0, 3), 0);
        assert_eq!(block_leader_index_for_height(3, 3), 0);
        assert_eq!(block_leader_index_for_height(5, 3), 2);
    }
}
