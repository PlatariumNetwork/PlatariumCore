//! Slashing & Stability Engine (Module 5).
//!
//! Nodes are penalized for: failing to vote, voting against the majority, equivocation (signing two different blocks at the same height), or confirming an invalid transaction.
//! Penalties: reputation is reduced by a reason-dependent amount; stake is reduced by a minor slash. If reputation falls below the threshold, the node is suspended.
//!
//! **Integration:** For “vote against majority” (L1/L2), call `apply_slash(registry, node_id, SlashingReason::AgainstMajority)` or `apply_slash_batch` on the list of nodes to penalize for full penalty (reputation, stake, and suspension check).
//!
//! # Determinism
//! Same (node_id, reason) yields the same penalty amounts; all arithmetic is integer-only.

use crate::core::node_registry::{NodeId, NodeRegistry, SCORE_SCALE};
use crate::error::{PlatariumError, Result};
use thiserror::Error;

/// Reputation score below this value results in node suspension (approximately 10% of SCORE_SCALE).
pub const SUSPENSION_THRESHOLD: u64 = 100_000;

/// Reason for slashing; determines the reputation penalty and stake slash amounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashingReason {
    /// Node did not submit a vote when selected.
    NoVote,
    /// Node voted against the majority (L1 or L2).
    AgainstMajority,
    /// Node signed two different blocks at the same height (equivocation).
    Equivocation,
    /// Node confirmed an invalid transaction.
    InvalidTx,
}

/// Returns the reputation penalty (subtracted from ReputationScore) for the given reason.
fn reputation_penalty_for(reason: SlashingReason) -> u64 {
    match reason {
        SlashingReason::NoVote => (SCORE_SCALE * 2) / 100,
        SlashingReason::AgainstMajority => (SCORE_SCALE * 3) / 100,
        SlashingReason::Equivocation => (SCORE_SCALE * 15) / 100,
        SlashingReason::InvalidTx => (SCORE_SCALE * 10) / 100,
    }
}

/// Returns the stake slash (subtracted from stake) for the given reason.
fn stake_slash_for(reason: SlashingReason) -> u128 {
    match reason {
        SlashingReason::NoVote => 1,
        SlashingReason::AgainstMajority => 2,
        SlashingReason::Equivocation => 100,
        SlashingReason::InvalidTx => 50,
    }
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SlashingError {
    #[error("Node not found: {0}")]
    NodeNotFound(NodeId),

    #[error("Slashing error: {0}")]
    Other(String),
}

impl From<SlashingError> for PlatariumError {
    fn from(e: SlashingError) -> Self {
        PlatariumError::State(format!("Slashing: {}", e))
    }
}

/// Applies slashing for a single node and reason: reduces reputation and stake by the configured amounts. Sets status to Suspended if reputation falls below the threshold.
pub fn apply_slash(
    registry: &NodeRegistry,
    node_id: &NodeId,
    reason: SlashingReason,
) -> Result<()> {
    apply_slash_with_threshold(registry, node_id, reason, SUSPENSION_THRESHOLD)
}

/// Applies slashing with a custom suspension threshold (otherwise identical to `apply_slash`).
pub fn apply_slash_with_threshold(
    registry: &NodeRegistry,
    node_id: &NodeId,
    reason: SlashingReason,
    suspension_threshold: u64,
) -> Result<()> {
    let node = registry
        .get(node_id)
        .ok_or_else(|| SlashingError::NodeNotFound(node_id.clone()))?;

    let rep_penalty = reputation_penalty_for(reason);
    let stake_slash = stake_slash_for(reason);

    let new_stake = node.stake.saturating_sub(stake_slash);
    registry.set_stake(node_id, new_stake)?;
    registry.apply_reputation_penalty(node_id, rep_penalty, suspension_threshold)?;

    Ok(())
}

/// Applies slashing to multiple nodes (e.g. all that voted against the majority). Uses the default suspension threshold.
pub fn apply_slash_batch(
    registry: &NodeRegistry,
    node_ids: &[NodeId],
    reason: SlashingReason,
) -> Result<()> {
    for node_id in node_ids {
        let _ = apply_slash(registry, node_id, reason);
    }
    Ok(())
}

/// Returns the (reputation_penalty, stake_slash) for the given reason (for display or off-chain logic).
pub fn penalty_amounts(reason: SlashingReason) -> (u64, u128) {
    (reputation_penalty_for(reason), stake_slash_for(reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::node_registry::NodeStatus;

    #[test]
    fn test_penalty_amounts() {
        let (rep, stake) = penalty_amounts(SlashingReason::NoVote);
        assert!(rep > 0 && rep < SCORE_SCALE);
        assert_eq!(stake, 1);

        let (rep_eq, stake_eq) = penalty_amounts(SlashingReason::Equivocation);
        assert!(rep_eq > rep);
        assert!(stake_eq > stake);
    }

    #[test]
    fn test_apply_slash_reduces_reputation_and_stake() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        let before = reg.get(&"n1".into()).unwrap();
        apply_slash(&reg, &"n1".into(), SlashingReason::AgainstMajority).unwrap();
        let after = reg.get(&"n1".into()).unwrap();
        assert!(after.reputation_score < before.reputation_score);
        assert_eq!(after.stake, before.stake.saturating_sub(2));
    }

    #[test]
    fn test_suspension_below_threshold() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        // Equivocation = 15% each; 7 * 150_000 = 1_050_000 > 1_000_000 → reputation 0, below 100_000
        for _ in 0..7 {
            apply_slash(&reg, &"n1".into(), SlashingReason::Equivocation).unwrap();
        }
        let node = reg.get(&"n1".into()).unwrap();
        assert_eq!(node.status, NodeStatus::Suspended);
    }
}
