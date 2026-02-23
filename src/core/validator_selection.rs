//! Dynamic Validator Selection Engine (Module 2).
//!
//! Adjusts the fraction of selected validators according to system load (TPS vs capacity):
//! higher load yields fewer validators to reduce coordination and speed up finalization.
//! Selection is deterministic: `hash(block_number, global_entropy)` with weights
//! `ReputationScore / LoadScore`; verifiable by all, not manipulable by the current producer.
//!
//! # Entropy
//! Pass `global_entropy` from the **previous finalized block** (e.g. block hash or commitment)
//! so that selection is reproducible and independent of the current block producer.
//!
//! # Determinism
//! Same inputs (current_tps, system_capacity, eligible set, block_number, global_entropy)
//! produce the same selected set. Integer-only arithmetic; no floating point, system time, or RNG.
//! Selection uses SHA256(seed || round) for reproducible weighted sampling.
//!
//! # Scalability
//! Selection uses cumulative weights and binary search. For very large sets (e.g. 20k+ nodes),
//! a tree-based structure (e.g. Fenwick) can reduce cost to O(K log N) per batch.

use std::collections::HashSet;
use sha2::{Sha256, Digest};
use crate::core::node_registry::{Node, NodeId, NodeRegistry};
use crate::error::{PlatariumError, Result};
use thiserror::Error;

/// Load tier boundaries (TPS as % of capacity). Higher load selects fewer validators.
/// TPS &lt; 30% → 25%; &lt; 60% → 20%; &lt; 85% → 15%; otherwise → 10%.
pub const TIER_LOW_PCT: u64 = 30;
pub const TIER_MID_PCT: u64 = 60;
pub const TIER_HIGH_PCT: u64 = 85;
pub const SELECT_PCT_10: u64 = 10;
pub const SELECT_PCT_15: u64 = 15;
pub const SELECT_PCT_20: u64 = 20;
pub const SELECT_PCT_25: u64 = 25;

/// L2 block validator selection percentages (10–20%); pool is disjoint from L1.
pub const L2_SELECT_PCT_10: u64 = 10;
pub const L2_SELECT_PCT_12: u64 = 12;
pub const L2_SELECT_PCT_15: u64 = 15;
pub const L2_SELECT_PCT_20: u64 = 20;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    #[error("System capacity must be positive")]
    ZeroCapacity,

    #[error("Selection error: {0}")]
    Other(String),
}

impl From<SelectionError> for PlatariumError {
    fn from(e: SelectionError) -> Self {
        PlatariumError::State(format!("ValidatorSelection: {}", e))
    }
}

/// Derives the L1 selection percentage (10, 15, 20, or 25) from current TPS and system capacity.
/// Higher load yields a lower percentage. `system_capacity` must be positive.
pub fn selection_percent_from_load(current_tps: u64, system_capacity: u64) -> Result<u64> {
    if system_capacity == 0 {
        return Err(SelectionError::ZeroCapacity.into());
    }
    let load_pct = (current_tps * 100) / system_capacity;
    let pct = if load_pct < TIER_LOW_PCT {
        SELECT_PCT_25
    } else if load_pct < TIER_MID_PCT {
        SELECT_PCT_20
    } else if load_pct < TIER_HIGH_PCT {
        SELECT_PCT_15
    } else {
        SELECT_PCT_10
    };
    Ok(pct)
}

/// Computes the L1 selection seed: SHA256(block_number_le || global_entropy).
pub fn compute_seed(block_number: u64, global_entropy: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(block_number.to_le_bytes());
    hasher.update(global_entropy);
    let h = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Computes the L2 selection seed (distinct from L1 so L1 and L2 validator sets differ).
pub fn compute_seed_l2(block_number: u64, global_entropy: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"L2");
    hasher.update(block_number.to_le_bytes());
    hasher.update(global_entropy);
    let h = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Derives the L2 selection percentage (10–20%) from load using the same tier logic as L1.
pub fn selection_percent_from_load_l2(current_tps: u64, system_capacity: u64) -> Result<u64> {
    if system_capacity == 0 {
        return Err(SelectionError::ZeroCapacity.into());
    }
    let load_pct = (current_tps * 100) / system_capacity;
    let pct = if load_pct < TIER_LOW_PCT {
        L2_SELECT_PCT_20
    } else if load_pct < TIER_MID_PCT {
        L2_SELECT_PCT_15
    } else if load_pct < TIER_HIGH_PCT {
        L2_SELECT_PCT_12
    } else {
        L2_SELECT_PCT_10
    };
    Ok(pct)
}

/// Selects L2 block validators: 10–20% of eligible nodes, excluding the given set (e.g. L1 validators). Uses the L2 seed.
pub fn select_validators_l2(
    registry: &NodeRegistry,
    current_tps: u64,
    system_capacity: u64,
    block_number: u64,
    global_entropy: &[u8],
    exclude: &[NodeId],
) -> Result<Vec<NodeId>> {
    let percent = selection_percent_from_load_l2(current_tps, system_capacity)?;
    let exclude_set: HashSet<_> = exclude.iter().cloned().collect();
    let eligible: Vec<Node> = registry
        .get_eligible()
        .into_iter()
        .filter(|n| !exclude_set.contains(&n.node_id))
        .collect();
    let count = select_count(eligible.len(), percent);

    if count == 0 || eligible.is_empty() {
        return Ok(Vec::new());
    }

    let mut weighted: Vec<WeightedNode> = eligible
        .into_iter()
        .map(|n| {
            let w = n.selection_weight_ratio().max(1);
            WeightedNode {
                node_id: n.node_id,
                weight: w,
            }
        })
        .collect();
    weighted.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    let total_weight: u64 = weighted.iter().map(|w| w.weight).sum();
    if total_weight == 0 {
        return Ok(Vec::new());
    }

    let seed = compute_seed_l2(block_number, global_entropy);
    let selected = weighted_select_n(weighted, total_weight, count, &seed);
    Ok(selected)
}

/// Derives a u64 (little-endian) from SHA256(seed || round) for the given round; used for slot selection.
fn hash_for_round(seed: &[u8; 32], round: u32) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(round.to_le_bytes());
    let h = hasher.finalize();
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&h[..8]);
    u64::from_le_bytes(buf)
}

/// Returns the number of validators to select: at least 1 when eligible_count > 0, else 0. Uses (eligible_count * percent / 100) rounded.
pub fn select_count(eligible_count: usize, percent: u64) -> usize {
    if eligible_count == 0 {
        return 0;
    }
    let n = (eligible_count as u64 * percent) / 100;
    if n == 0 {
        1
    } else {
        n.min(eligible_count as u64) as usize
    }
}

/// Entry for weighted sampling: node id and weight (from `Node::selection_weight_ratio`).
#[derive(Clone)]
struct WeightedNode {
    node_id: NodeId,
    weight: u64,
}

/// Selects N distinct nodes by deterministic weighted sampling. Uses cumulative weights and binary search.
/// Result is sorted by node_id. For very large sets, a tree-based structure can reduce complexity (see module docs).
fn weighted_select_n(
    weighted: Vec<WeightedNode>,
    total_weight: u64,
    n: usize,
    seed: &[u8; 32],
) -> Vec<NodeId> {
    if n == 0 || total_weight == 0 || weighted.is_empty() {
        return Vec::new();
    }
    let mut selected = Vec::with_capacity(n);
    let mut list = weighted;
    let mut current_total = total_weight;

    for round in 0..n {
        if list.is_empty() {
            break;
        }
        let slot = hash_for_round(seed, round as u32) % current_total;
        let mut cum: Vec<u64> = Vec::with_capacity(list.len());
        let mut sum = 0u64;
        for w in &list {
            sum += w.weight;
            cum.push(sum);
        }
        let idx = match cum.binary_search_by(|c| c.cmp(&(slot + 1))) {
            Ok(i) => i,
            Err(i) => i,
        };
        let idx = idx.min(list.len().saturating_sub(1));
        let picked = list.remove(idx);
        current_total = current_total.saturating_sub(picked.weight);
        selected.push(picked.node_id);
    }
    selected.sort();
    selected
}

/// Performs dynamic validator selection: the number of validators is derived from TPS/capacity, then nodes are chosen by deterministic weighted sampling.
///
/// - `registry`: node registry (only Active nodes are eligible).
/// - `current_tps`: current transactions per second (or per slot).
/// - `system_capacity`: maximum TPS or capacity (must be positive).
/// - `block_number`: current block number, used in the seed.
/// - `global_entropy`: optional entropy (e.g. from previous block or beacon); may be empty.
///
/// Returns a sorted list of selected node ids. The result is deterministic and verifiable.
pub fn select_validators(
    registry: &NodeRegistry,
    current_tps: u64,
    system_capacity: u64,
    block_number: u64,
    global_entropy: &[u8],
) -> Result<Vec<NodeId>> {
    let percent = selection_percent_from_load(current_tps, system_capacity)?;
    let eligible = registry.get_eligible();
    let count = select_count(eligible.len(), percent);

    if count == 0 || eligible.is_empty() {
        return Ok(Vec::new());
    }

    let mut weighted: Vec<WeightedNode> = eligible
        .into_iter()
        .map(|n| {
            let w = n.selection_weight_ratio().max(1);
            WeightedNode {
                node_id: n.node_id,
                weight: w,
            }
        })
        .collect();
    weighted.sort_by(|a, b| a.node_id.cmp(&b.node_id));

    let total_weight: u64 = weighted.iter().map(|w| w.weight).sum();
    if total_weight == 0 {
        return Ok(Vec::new());
    }

    let seed = compute_seed(block_number, global_entropy);
    let selected = weighted_select_n(weighted, total_weight, count, &seed);
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_percent_tiers() {
        // Low load → most validators (25%)
        assert_eq!(selection_percent_from_load(20, 100).unwrap(), SELECT_PCT_25);
        assert_eq!(selection_percent_from_load(29, 100).unwrap(), SELECT_PCT_25);
        // < 60% → 20%
        assert_eq!(selection_percent_from_load(30, 100).unwrap(), SELECT_PCT_20);
        assert_eq!(selection_percent_from_load(59, 100).unwrap(), SELECT_PCT_20);
        // < 85% → 15%
        assert_eq!(selection_percent_from_load(60, 100).unwrap(), SELECT_PCT_15);
        assert_eq!(selection_percent_from_load(84, 100).unwrap(), SELECT_PCT_15);
        // High load → fewest validators (10%)
        assert_eq!(selection_percent_from_load(85, 100).unwrap(), SELECT_PCT_10);
        assert_eq!(selection_percent_from_load(100, 100).unwrap(), SELECT_PCT_10);
    }

    #[test]
    fn test_selection_percent_zero_capacity() {
        assert!(selection_percent_from_load(10, 0).is_err());
    }

    #[test]
    fn test_select_count() {
        assert_eq!(select_count(100, 10), 10);
        assert_eq!(select_count(100, 15), 15);
        assert_eq!(select_count(100, 25), 25);
        assert_eq!(select_count(10, 10), 1);
        assert_eq!(select_count(1, 10), 1);
        assert_eq!(select_count(0, 10), 0);
    }

    #[test]
    fn test_seed_deterministic() {
        let s1 = compute_seed(1, b"entropy");
        let s2 = compute_seed(1, b"entropy");
        assert_eq!(s1, s2);
        let s3 = compute_seed(2, b"entropy");
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_select_validators_deterministic() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        reg.register("n2".into(), "pk2".into(), 1000, 10).unwrap();
        reg.register("n3".into(), "pk3".into(), 1000, 10).unwrap();
        reg.register("n4".into(), "pk4".into(), 1000, 10).unwrap();
        reg.register("n5".into(), "pk5".into(), 1000, 10).unwrap();

        let a = select_validators(&reg, 10, 100, 1, b"entropy").unwrap();
        let b = select_validators(&reg, 10, 100, 1, b"entropy").unwrap();
        assert_eq!(a, b);

        let c = select_validators(&reg, 50, 100, 1, b"entropy").unwrap();
        assert!(c.len() <= a.len()); // higher load (50%) → fewer validators
    }

    #[test]
    fn test_select_validators_adaptive_n() {
        let reg = NodeRegistry::new();
        for i in 0..20 {
            reg.register(format!("n{}", i), format!("pk{}", i), 1000, 10).unwrap();
        }
        let low_load = select_validators(&reg, 10, 100, 1, b"").unwrap();
        let high_load = select_validators(&reg, 90, 100, 1, b"").unwrap();
        assert!(high_load.len() <= low_load.len()); // higher load → fewer validators
    }
}
