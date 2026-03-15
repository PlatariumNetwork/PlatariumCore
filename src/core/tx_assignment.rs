//! Dynamic Group-Based TX Assignment and Capacity Filtering (Module 7).
//!
//! **Validation Modules Analysis — Step 7:**
//! - For each transaction, compute the **required total stake** for safe verification.
//! - Form **one or more validator groups** dynamically so that each group's total stake ≥ required stake for the TX.
//! - Each validator in a group must have **stake ≥ TX amount** (risk coverage); otherwise they cannot verify that TX.
//! - Groups are formed using **load and reputation** (L1/L2 eligible nodes, sorted by selection weight).
//! - **Distribution of TX between groups** optimizes verification speed and allows parallel processing of large TX.
//! - After verification, apply **slashing/penalty** for validators on errors (use existing `process_l1_confirmation` → `apply_l1_penalties` / `apply_slash_batch`).
//!
//! # Determinism
//! Same (registry state, TX set, params) yields the same group formation and assignment; integer-only, no RNG.

use crate::core::node_registry::{Node, NodeId, NodeRegistry};
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use thiserror::Error;

/// Minimum required total stake for a group to verify any TX (floor when TX amount is tiny).
pub const DEFAULT_MIN_REQUIRED_STAKE: u128 = 1;

/// Minimum stake per validator to participate in a group (floor).
pub const DEFAULT_MIN_VALIDATOR_STAKE: u128 = 1;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum TxAssignmentError {
    #[error("Insufficient eligible validators: need stake >= {0}, have {1}")]
    InsufficientValidators(u128, u128),

    #[error("TX assignment error: {0}")]
    Other(String),
}

impl From<TxAssignmentError> for PlatariumError {
    fn from(e: TxAssignmentError) -> Self {
        PlatariumError::State(format!("TxAssignment: {}", e))
    }
}

/// Returns the required **total stake** for a group to safely verify this TX (e.g. cover risk).
/// Policy: at least the TX amount, with a configurable floor.
#[inline]
pub fn required_stake_for_tx(tx: &Transaction) -> u128 {
    required_stake_for_amount(tx.amount)
}

/// Returns the required total stake for a given TX amount (same policy as `required_stake_for_tx`).
pub fn required_stake_for_amount(amount: u128) -> u128 {
    amount.max(DEFAULT_MIN_REQUIRED_STAKE)
}

/// Returns the **minimum stake per validator** for this TX: each validator in the group must have stake ≥ this (risk coverage).
#[inline]
pub fn min_validator_stake_for_tx(tx: &Transaction) -> u128 {
    min_validator_stake_for_amount(tx.amount)
}

/// Returns the minimum stake per validator for a given TX amount.
pub fn min_validator_stake_for_amount(amount: u128) -> u128 {
    amount.max(DEFAULT_MIN_VALIDATOR_STAKE)
}

/// Filters eligible nodes to those with stake ≥ `min_stake`, sorted by selection weight (descending) for load/reputation awareness.
fn eligible_sorted_by_weight(
    registry: &NodeRegistry,
    min_stake: u128,
) -> Vec<Node> {
    let mut nodes: Vec<Node> = registry
        .get_eligible()
        .into_iter()
        .filter(|n| n.stake >= min_stake)
        .collect();
    nodes.sort_by(|a, b| {
        let wa = a.selection_weight_ratio();
        let wb = b.selection_weight_ratio();
        wb.cmp(&wa)
    });
    nodes
}

/// Forms one or more verifier groups so that each group's total stake ≥ `required_total_stake`, and each member has stake ≥ `min_validator_stake`.
/// Groups are filled in order of selection weight (best load/reputation first). Returns list of groups (each group = list of NodeIds).
pub fn form_verifier_groups(
    registry: &NodeRegistry,
    required_total_stake: u128,
    min_validator_stake: u128,
    max_groups: usize,
) -> Result<Vec<Vec<NodeId>>> {
    let nodes = eligible_sorted_by_weight(registry, min_validator_stake);
    if nodes.is_empty() {
        return Err(TxAssignmentError::InsufficientValidators(
            min_validator_stake,
            0,
        )
        .into());
    }

    let total_available: u128 = nodes.iter().map(|n| n.stake).sum();
    if total_available < required_total_stake {
        return Err(TxAssignmentError::InsufficientValidators(
            required_total_stake,
            total_available,
        )
        .into());
    }

    let mut groups: Vec<Vec<NodeId>> = Vec::new();
    let mut used = 0usize;

    for _ in 0..max_groups {
        let mut group_stake = 0u128;
        let mut group: Vec<NodeId> = Vec::new();
        for i in used..nodes.len() {
            if group_stake >= required_total_stake {
                break;
            }
            let n = &nodes[i];
            if n.stake >= min_validator_stake {
                group_stake += n.stake;
                group.push(n.node_id.clone());
                used = i + 1;
            }
        }
        if group.is_empty() {
            break;
        }
        if group_stake >= required_total_stake {
            groups.push(group);
        } else {
            break;
        }
    }

    Ok(groups)
}

/// Assignment of a single TX to a verifier group (group index and node ids).
#[derive(Debug, Clone)]
pub struct TxGroupAssignment {
    pub tx_hash: String,
    pub group_index: usize,
    pub verifier_ids: Vec<NodeId>,
}

/// Assigns each transaction to a verifier group. Each TX gets a group whose total stake ≥ required_stake_for_tx and each member has stake ≥ tx amount.
/// Groups are formed considering load and reputation. Returns one assignment per TX (optimizes parallel verification).
pub fn assign_transactions_to_groups(
    txs: &[Transaction],
    registry: &NodeRegistry,
    max_groups: usize,
) -> Result<Vec<TxGroupAssignment>> {
    if txs.is_empty() {
        return Ok(Vec::new());
    }

    let mut required_max = 0u128;
    let mut min_stake_max = 0u128;
    for tx in txs {
        let r = required_stake_for_tx(tx);
        let m = min_validator_stake_for_tx(tx);
        if r > required_max {
            required_max = r;
        }
        if m > min_stake_max {
            min_stake_max = m;
        }
    }

    let groups = form_verifier_groups(registry, required_max, min_stake_max, max_groups)?;
    if groups.is_empty() {
        return Err(TxAssignmentError::Other("No verifier groups formed".to_string()).into());
    }

    let mut assignments = Vec::with_capacity(txs.len());
    for (tx_idx, tx) in txs.iter().enumerate() {
        let req = required_stake_for_tx(tx);
        let min_stake = min_validator_stake_for_tx(tx);
        let group_filtered: Vec<NodeId> = groups[tx_idx % groups.len()]
            .iter()
            .filter_map(|id| {
                registry.get(id).and_then(|n| {
                    if n.stake >= min_stake {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();
        let filtered_stake: u128 = group_filtered
            .iter()
            .filter_map(|id| registry.get(id).map(|n| n.stake))
            .sum();

        let (group_index, verifier_ids) = if filtered_stake >= req && !group_filtered.is_empty() {
            (tx_idx % groups.len(), group_filtered)
        } else {
            let mut fallback = None;
            for (gidx, g) in groups.iter().enumerate() {
                let ok: Vec<NodeId> = g
                    .iter()
                    .filter_map(|id| {
                        registry.get(id).and_then(|n| {
                            if n.stake >= min_stake {
                                Some(id.clone())
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
                let ok_stake: u128 = ok.iter().filter_map(|id| registry.get(id).map(|n| n.stake)).sum();
                if ok_stake >= req && !ok.is_empty() {
                    fallback = Some((gidx, ok));
                    break;
                }
            }
            match fallback {
                Some((gidx, ids)) => (gidx, ids),
                None => {
                    return Err(TxAssignmentError::InsufficientValidators(req, 0).into());
                }
            }
        };

        assignments.push(TxGroupAssignment {
            tx_hash: tx.hash.clone(),
            group_index,
            verifier_ids,
        });
    }

    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use std::collections::HashSet;

    fn dummy_tx(amount: u128) -> Transaction {
        Transaction::new(
            "from".to_string(),
            "to".to_string(),
            Asset::PLP,
            amount,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main".to_string(),
            "sig_derived".to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_required_stake_for_tx() {
        let tx = dummy_tx(1000);
        assert_eq!(required_stake_for_tx(&tx), 1000);
        let tx_small = dummy_tx(0);
        assert_eq!(required_stake_for_tx(&tx_small), DEFAULT_MIN_REQUIRED_STAKE);
    }

    #[test]
    fn test_min_validator_stake_for_tx() {
        let tx = dummy_tx(500);
        assert_eq!(min_validator_stake_for_tx(&tx), 500);
    }

    #[test]
    fn test_form_verifier_groups() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        reg.register("n2".into(), "pk2".into(), 1000, 10).unwrap();
        reg.register("n3".into(), "pk3".into(), 500, 10).unwrap();

        let groups = form_verifier_groups(&reg, 1500, 500, 2).unwrap();
        assert!(!groups.is_empty());
        let total: u128 = groups[0]
            .iter()
            .filter_map(|id| reg.get(id).map(|n| n.stake))
            .sum();
        assert!(total >= 1500);
    }

    #[test]
    fn test_form_verifier_groups_insufficient() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 100, 10).unwrap();
        let r = form_verifier_groups(&reg, 1000, 100, 1);
        assert!(r.is_err());
    }

    #[test]
    fn test_assign_transactions_to_groups() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 2000, 10).unwrap();
        reg.register("n2".into(), "pk2".into(), 2000, 10).unwrap();

        let txs = vec![dummy_tx(100), dummy_tx(200)];
        let assignments = assign_transactions_to_groups(&txs, &reg, 2).unwrap();
        assert_eq!(assignments.len(), 2);
        assert_eq!(assignments[0].tx_hash, txs[0].hash);
        assert!(!assignments[0].verifier_ids.is_empty());
    }
}
