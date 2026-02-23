//! Level 1 Transaction Confirmation Layer (Module 3).
//!
//! The first validator layer: a subset of nodes (10–25%) verifies each transaction (balance, nonce, signature, fee in μPLP).
//! If at least 67% vote Confirm, the transaction is confirmed; otherwise it is rejected. Nodes that vote against the majority are penalized.
//!
//! # Determinism
//! Same transaction, state, and votes yield the same `ConfirmationResult` and list of nodes to penalize. Verification reuses `ExecutionLogic` (signature, fee, balance, nonce).

use crate::core::execution::ExecutionLogic;
use crate::core::node_registry::{NodeId, NodeRegistry};
use crate::core::state::State;
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use thiserror::Error;

/// L1 confirmation threshold: at least this percentage of validators must vote Confirm.
pub const L1_CONFIRM_THRESHOLD_PCT: u64 = 67;

/// A validator’s vote on a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vote {
    Confirm,
    Reject,
}

/// L1 confirmation result for the transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationResult {
    Confirmed,
    Rejected,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ConfirmationError {
    #[error("No votes provided")]
    NoVotes,

    #[error("Confirmation error: {0}")]
    Other(String),
}

impl From<ConfirmationError> for PlatariumError {
    fn from(e: ConfirmationError) -> Self {
        PlatariumError::State(format!("ConfirmationLayer: {}", e))
    }
}

/// Performs L1 verification: balance, nonce, signature, and fee (μPLP). Returns `Ok(true)` if the transaction is valid, `Ok(false)` otherwise (no error, only failed checks).
pub fn verify_tx_for_l1(state: &State, tx: &Transaction) -> Result<bool> {
    let valid_sig_and_fee = ExecutionLogic::validate_transaction(tx).is_ok();
    if !valid_sig_and_fee {
        return Ok(false);
    }
    let applicable = ExecutionLogic::check_transaction_applicability(state, tx).is_ok();
    Ok(applicable)
}

/// Aggregates L1 votes and returns the confirmation result plus the list of node ids to penalize (those that voted against the majority).
/// Confirmed if (confirm_count × 100) ≥ (total_votes × L1_CONFIRM_THRESHOLD_PCT); otherwise Rejected. Majority is Confirm when confirm_count > total/2.
pub fn process_l1_confirmation(
    votes: &[(NodeId, Vote)],
) -> Result<(ConfirmationResult, Vec<NodeId>)> {
    if votes.is_empty() {
        return Err(ConfirmationError::NoVotes.into());
    }
    let total = votes.len() as u64;
    let confirm_count = votes
        .iter()
        .filter(|(_, v)| *v == Vote::Confirm)
        .count() as u64;

    let result = if (confirm_count * 100) >= (total * L1_CONFIRM_THRESHOLD_PCT) {
        ConfirmationResult::Confirmed
    } else {
        ConfirmationResult::Rejected
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

/// Full L1 flow: verifies the transaction against state, then aggregates votes. Returns the result and the list of nodes to penalize.
pub fn confirm_transaction_l1(
    state: &State,
    tx: &Transaction,
    votes: &[(NodeId, Vote)],
) -> Result<(ConfirmationResult, Vec<NodeId>)> {
    let _valid = verify_tx_for_l1(state, tx)?;
    process_l1_confirmation(votes)
}

/// Applies a rating penalty to each node that voted against the majority by recording a missed vote.
pub fn apply_l1_penalties(registry: &NodeRegistry, to_penalize: &[NodeId]) -> Result<()> {
    for node_id in to_penalize {
        registry.record_vote(node_id, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_67() {
        let votes_ok: Vec<(NodeId, Vote)> = (0..10)
            .map(|i| (format!("n{}", i), if i < 7 { Vote::Confirm } else { Vote::Reject }))
            .collect();
        let (res, penalize) = process_l1_confirmation(&votes_ok).unwrap();
        assert_eq!(res, ConfirmationResult::Confirmed);
        assert_eq!(penalize.len(), 3);
    }

    #[test]
    fn test_below_threshold_rejected() {
        let votes: Vec<(NodeId, Vote)> = (0..10)
            .map(|i| (format!("n{}", i), if i < 6 { Vote::Confirm } else { Vote::Reject }))
            .collect();
        let (res, penalize) = process_l1_confirmation(&votes).unwrap();
        assert_eq!(res, ConfirmationResult::Rejected);
        assert_eq!(penalize.len(), 6);
    }

    #[test]
    fn test_penalize_minority() {
        let votes: Vec<(NodeId, Vote)> = [
            ("n1", Vote::Confirm),
            ("n2", Vote::Confirm),
            ("n3", Vote::Reject),
        ]
        .iter()
        .map(|(id, v)| (id.to_string(), *v))
        .collect();
        let (res, penalize) = process_l1_confirmation(&votes).unwrap();
        assert_eq!(res, ConfirmationResult::Confirmed);
        assert_eq!(penalize, vec!["n3"]);
    }

    #[test]
    fn test_no_votes_error() {
        assert!(process_l1_confirmation(&[]).is_err());
    }
}
