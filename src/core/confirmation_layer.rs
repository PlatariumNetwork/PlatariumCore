//! Transaction Confirmation Layer - L1 (Module 3).
//!
//! **Validation Modules Analysis - Step 3:** Integrates transaction verification by validator groups.
//! - Select **10–30%** of validators per TX (via Step 2: `select_validators` / `selection_percent_from_load`).
//! - Validators verify **balance**, **nonce**, **signature**, **fee** (μPLP); then vote Confirm/Reject.
//! - **`process_l1_confirmation(votes)`** → returns **(Confirmed | Rejected, to_penalize)**.
//!   Confirmed if ≥67% vote Confirm; nodes that voted against the majority are in `to_penalize`.
//!
//! Flow: `verify_tx_for_l1(state, tx)` (balance/nonce/sig/fee) → collect votes → `process_l1_confirmation(votes)` → `apply_l1_penalties(registry, to_penalize)`.
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

/// Performs L1 verification (Step 3): balance, nonce, signature, and fee (μPLP).
/// Returns `Ok(true)` if the transaction is valid for L1, `Ok(false)` otherwise (no error, only failed checks).
pub fn verify_tx_for_l1(state: &State, tx: &Transaction) -> Result<bool> {
    let valid_sig_and_fee = ExecutionLogic::validate_transaction(tx).is_ok();
    if !valid_sig_and_fee {
        return Ok(false);
    }
    let applicable = ExecutionLogic::check_transaction_applicability(state, tx).is_ok();
    Ok(applicable)
}

/// Aggregates L1 votes and returns **(Confirmed | Rejected, to_penalize)** (Step 3).
/// Confirmed if (confirm_count × 100) ≥ (total_votes × L1_CONFIRM_THRESHOLD_PCT); otherwise Rejected.
/// `to_penalize` = node ids that voted against the majority (for `apply_l1_penalties`).
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

/// Full L1 flow: verifies the transaction against state, then aggregates votes.
/// H3: invalid tx cannot confirm — verify must succeed before vote aggregation.
pub fn confirm_transaction_l1(
    state: &State,
    tx: &Transaction,
    votes: &[(NodeId, Vote)],
) -> Result<(ConfirmationResult, Vec<NodeId>)> {
    let valid = verify_tx_for_l1(state, tx)?;
    if !valid {
        return Err(ConfirmationError::Other(
            "L1 rejected: transaction failed verify_tx_for_l1 (sig/fee/balance/nonce)".into(),
        )
        .into());
    }
    process_l1_confirmation(votes)
}

/// Signed L1 vote (H3): signature over canonical payload binds node identity to ballot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedL1Vote {
    pub node_id: NodeId,
    pub vote: Vote,
    pub tx_hash: String,
    pub pub_key: String,
    pub signature: String,
}

#[derive(serde::Serialize)]
struct L1VoteSignPayload<'a> {
    node_id: &'a str,
    vote: u8,
    tx_hash: &'a str,
}

impl SignedL1Vote {
    fn vote_byte(v: Vote) -> u8 {
        match v {
            Vote::Confirm => 1,
            Vote::Reject => 0,
        }
    }

    /// Verify ECDSA signature over (node_id, vote, tx_hash).
    pub fn verify(&self) -> Result<bool> {
        use crate::signature::verify_signature;
        let payload = L1VoteSignPayload {
            node_id: &self.node_id,
            vote: Self::vote_byte(self.vote),
            tx_hash: &self.tx_hash,
        };
        verify_signature(&payload, &self.signature, &self.pub_key)
    }
}

/// Aggregate signed L1 votes: drop/fail invalid signatures, then same threshold rules (H3).
pub fn process_l1_confirmation_signed(
    expected_tx_hash: &str,
    votes: &[SignedL1Vote],
) -> Result<(ConfirmationResult, Vec<NodeId>)> {
    if votes.is_empty() {
        return Err(ConfirmationError::NoVotes.into());
    }
    let mut verified: Vec<(NodeId, Vote)> = Vec::with_capacity(votes.len());
    for v in votes {
        if v.tx_hash != expected_tx_hash {
            return Err(ConfirmationError::Other(format!(
                "vote tx_hash mismatch for node {}",
                v.node_id
            ))
            .into());
        }
        if !v.verify()? {
            return Err(ConfirmationError::Other(format!(
                "invalid L1 vote signature for node {}",
                v.node_id
            ))
            .into());
        }
        verified.push((v.node_id.clone(), v.vote));
    }
    process_l1_confirmation(&verified)
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
        // 6/10 = 60% < 67% → Rejected; simple majority still Confirm → penalize Reject voters.
        let votes: Vec<(NodeId, Vote)> = (0..10)
            .map(|i| (format!("n{}", i), if i < 6 { Vote::Confirm } else { Vote::Reject }))
            .collect();
        let (res, penalize) = process_l1_confirmation(&votes).unwrap();
        assert_eq!(res, ConfirmationResult::Rejected);
        assert_eq!(penalize.len(), 4);
    }

    #[test]
    fn test_penalize_minority() {
        // Need ≥67%: 7/10 confirm.
        let votes: Vec<(NodeId, Vote)> = (0..10)
            .map(|i| (format!("n{}", i), if i < 7 { Vote::Confirm } else { Vote::Reject }))
            .collect();
        let (res, penalize) = process_l1_confirmation(&votes).unwrap();
        assert_eq!(res, ConfirmationResult::Confirmed);
        assert_eq!(penalize.len(), 3);
    }

    #[test]
    fn confirm_transaction_l1_rejects_invalid_tx() {
        let state = State::new();
        let tx = crate::core::transaction::Transaction::new(
            "a".into(),
            "b".into(),
            crate::core::asset::Asset::PLP,
            1,
            1,
            0,
            std::collections::HashSet::new(),
            std::collections::HashSet::new(),
            "aa".into(),
            "bb".into(),
        )
        .unwrap();
        let votes = vec![("n1".into(), Vote::Confirm), ("n2".into(), Vote::Confirm)];
        let err = confirm_transaction_l1(&state, &tx, &votes);
        assert!(err.is_err(), "H3: invalid tx must not confirm");
    }
}
