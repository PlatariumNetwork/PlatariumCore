//! Node Registry & Rating Engine (Module 1).
//!
//! Maintains the validator node registry with reputation, load, and uptime metrics.
//! All scoring uses integer arithmetic; no floating point or non-deterministic inputs.
//!
//! # Determinism
//! - Scores use a fixed scale of 1_000_000 (0 = 0.0, 1_000_000 = 1.0).
//! - No system time or randomness; same inputs yield the same ReputationScore, LoadScore, and selection weight.
//!
//! # Reputation formula (integer)
//! `ReputationScore = (UptimeScore×300 + LatencyScore×200 + VoteAccuracy×300 + StakeWeight×200) / 1000`.
//! All component scores lie in `0..=SCORE_SCALE`.
//!
//! # Load
//! `LoadScore = current_tasks × SCORE_SCALE / max_capacity` (capped at SCORE_SCALE).
//! Higher load reduces effective selection weight.

use std::collections::HashMap;
use std::sync::RwLock;
use crate::error::{PlatariumError, Result};
use thiserror::Error;

/// Normalized score scale. Values in 0..=SCORE_SCALE represent 0.0..=1.0.
pub const SCORE_SCALE: u64 = 1_000_000;

/// Reputation weight coefficients (sum 1000): Uptime 30%, Latency 20%, VoteAccuracy 30%, Stake 20%.
pub const WEIGHT_UPTIME: u64 = 300;
pub const WEIGHT_LATENCY: u64 = 200;
pub const WEIGHT_VOTE_ACCURACY: u64 = 300;
pub const WEIGHT_STAKE: u64 = 200;

/// Validator node status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Active,
    Suspended,
}

/// Node identifier (e.g. consensus address or peer id).
pub type NodeId = String;

/// A validator node with stake, reputation and load metrics.
#[derive(Debug, Clone)]
pub struct Node {
    pub node_id: NodeId,
    pub public_key: String,
    pub stake: u128,
    /// Cached reputation in 0..=SCORE_SCALE. Updated by `compute_reputation`.
    pub reputation_score: u64,
    /// Uptime score in 0..=SCORE_SCALE (1.0 = full uptime). Set by consensus layer.
    pub uptime_score: u64,
    /// Latency score in 0..=SCORE_SCALE (1.0 = best). Set by consensus layer.
    pub latency_score: u64,
    /// Load ratio in 0..=SCORE_SCALE (current_tasks / max_capacity). Recomputed when load changes.
    pub load_score: u64,
    pub missed_votes: u64,
    pub total_votes: u64,
    /// Number of tasks currently assigned; used to compute `load_score`.
    pub current_tasks: u64,
    /// Maximum task capacity; `load_score = current_tasks * SCORE_SCALE / max_capacity`.
    pub max_capacity: u64,
    pub status: NodeStatus,
}

impl Node {
    /// Constructs a new node. All scores default to 1.0 except `load_score`, which is 0.
    pub fn new(
        node_id: NodeId,
        public_key: String,
        stake: u128,
        max_capacity: u64,
    ) -> Self {
        Self {
            node_id: node_id.clone(),
            public_key,
            stake,
            reputation_score: SCORE_SCALE,
            uptime_score: SCORE_SCALE,
            latency_score: SCORE_SCALE,
            load_score: 0,
            missed_votes: 0,
            total_votes: 0,
            current_tasks: 0,
            max_capacity: if max_capacity == 0 { 1 } else { max_capacity },
            status: NodeStatus::Active,
        }
    }

    /// Returns vote accuracy in 0..=SCORE_SCALE: (total_votes - missed_votes) / total_votes, or 1.0 if no votes.
    pub fn vote_accuracy(&self) -> u64 {
        if self.total_votes == 0 {
            return SCORE_SCALE;
        }
        let correct = self.total_votes.saturating_sub(self.missed_votes);
        (correct * SCORE_SCALE) / self.total_votes
    }

    /// Recomputes `load_score` from `current_tasks` and `max_capacity`. Result is in 0..=SCORE_SCALE.
    pub fn recompute_load_score(&mut self) {
        let cap = self.max_capacity.max(1);
        self.load_score = if self.current_tasks >= cap {
            SCORE_SCALE
        } else {
            (self.current_tasks * SCORE_SCALE) / cap
        };
    }

    /// Recomputes reputation from component scores. Requires the maximum stake across all nodes for StakeWeight.
    /// Formula: (Uptime×300 + Latency×200 + VoteAccuracy×300 + StakeWeight×200) / 1000.
    /// StakeWeight = min(SCORE_SCALE, stake × SCORE_SCALE / max_stake).
    pub fn compute_reputation(&mut self, max_stake: u128) {
        let vote_acc = self.vote_accuracy();
        let stake_weight = if max_stake == 0 {
            SCORE_SCALE
        } else {
            let w = (self.stake * SCORE_SCALE as u128) / max_stake;
            w.min(SCORE_SCALE as u128) as u64
        };
        let sum = self.uptime_score * WEIGHT_UPTIME
            + self.latency_score * WEIGHT_LATENCY
            + vote_acc * WEIGHT_VOTE_ACCURACY
            + stake_weight * WEIGHT_STAKE;
        self.reputation_score = sum / 1000;
    }

    /// Selection weight for legacy path: reputation × (1 - load). Higher load reduces weight. Value in 0..=SCORE_SCALE.
    pub fn selection_weight(&self) -> u64 {
        let load_penalty = SCORE_SCALE.saturating_sub(self.load_score);
        (self.reputation_score * load_penalty) / SCORE_SCALE
    }

    /// Selection weight for dynamic validator selection: reputation / load. Higher load yields lower weight.
    /// Load score zero is treated as one to avoid division by zero. Used for integer-weighted sampling.
    pub fn selection_weight_ratio(&self) -> u64 {
        let denom = self.load_score.max(1);
        (self.reputation_score * SCORE_SCALE) / denom
    }
}

/// Errors produced by the node registry.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum NodeRegistryError {
    #[error("Node already exists: {0}")]
    DuplicateNode(NodeId),

    #[error("Node not found: {0}")]
    NodeNotFound(NodeId),

    #[error("Invalid score: must be 0..={0}, got {1}")]
    InvalidScore(u64, u64),

    #[error("Registry error: {0}")]
    Other(String),
}

impl From<NodeRegistryError> for PlatariumError {
    fn from(e: NodeRegistryError) -> Self {
        PlatariumError::State(format!("NodeRegistry: {}", e))
    }
}

/// Thread-safe node registry and rating engine.
#[derive(Debug)]
pub struct NodeRegistry {
    nodes: RwLock<HashMap<NodeId, Node>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a new node. Errors if `node_id` is already registered.
    pub fn register(
        &self,
        node_id: NodeId,
        public_key: String,
        stake: u128,
        max_capacity: u64,
    ) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        if nodes.contains_key(&node_id) {
            return Err(NodeRegistryError::DuplicateNode(node_id).into());
        }
        let mut node = Node::new(node_id.clone(), public_key, stake, max_capacity);
        let max_stake = nodes.values().map(|n| n.stake).max().unwrap_or(0).max(node.stake);
        node.compute_reputation(max_stake);
        nodes.insert(node_id, node);
        Ok(())
    }

    /// Removes a node from the registry.
    pub fn unregister(&self, node_id: &NodeId) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        nodes.remove(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        Ok(())
    }

    /// Returns the node for the given id, if registered.
    pub fn get(&self, node_id: &NodeId) -> Option<Node> {
        let nodes = self.nodes.read().unwrap();
        nodes.get(node_id).cloned()
    }

    /// Returns all registered nodes, sorted by `node_id` for deterministic ordering.
    pub fn get_all(&self) -> Vec<Node> {
        let nodes = self.nodes.read().unwrap();
        let mut v: Vec<Node> = nodes.values().cloned().collect();
        v.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        v
    }

    /// Returns the maximum stake among all nodes, or 0 if the registry is empty.
    fn max_stake(&self) -> u128 {
        let nodes = self.nodes.read().unwrap();
        nodes.values().map(|n| n.stake).max().unwrap_or(0)
    }

    /// Recomputes reputation for every node (e.g. after stake or score updates).
    pub fn recompute_all_reputations(&self) {
        let max_stake = self.max_stake();
        let mut nodes = self.nodes.write().unwrap();
        for node in nodes.values_mut() {
            node.compute_reputation(max_stake);
        }
    }

    /// Sets the uptime score for a node. Must be in 0..=SCORE_SCALE.
    pub fn set_uptime_score(&self, node_id: &NodeId, score: u64) -> Result<()> {
        if score > SCORE_SCALE {
            return Err(NodeRegistryError::InvalidScore(SCORE_SCALE, score).into());
        }
        let max_stake = self.max_stake();
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.uptime_score = score;
        node.compute_reputation(max_stake);
        Ok(())
    }

    /// Sets the latency score for a node. Must be in 0..=SCORE_SCALE.
    pub fn set_latency_score(&self, node_id: &NodeId, score: u64) -> Result<()> {
        if score > SCORE_SCALE {
            return Err(NodeRegistryError::InvalidScore(SCORE_SCALE, score).into());
        }
        let max_stake = self.max_stake();
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.latency_score = score;
        node.compute_reputation(max_stake);
        Ok(())
    }

    /// Records one vote for a node. Set `missed` to true if the node did not participate.
    pub fn record_vote(&self, node_id: &NodeId, missed: bool) -> Result<()> {
        let max_stake = self.max_stake();
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.total_votes = node.total_votes.saturating_add(1);
        if missed {
            node.missed_votes = node.missed_votes.saturating_add(1);
        }
        node.compute_reputation(max_stake);
        Ok(())
    }

    /// Updates current task count and max capacity for a node, then recomputes load score and reputation.
    pub fn set_load(&self, node_id: &NodeId, current_tasks: u64, max_capacity: u64) -> Result<()> {
        let max_stake = self.max_stake();
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.current_tasks = current_tasks;
        node.max_capacity = max_capacity.max(1);
        node.recompute_load_score();
        node.compute_reputation(max_stake);
        Ok(())
    }

    /// Updates a node’s stake and recomputes reputation for all nodes (StakeWeight depends on global max stake).
    pub fn set_stake(&self, node_id: &NodeId, stake: u128) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.stake = stake;
        let max_stake = nodes.values().map(|n| n.stake).max().unwrap_or(0);
        for n in nodes.values_mut() {
            n.compute_reputation(max_stake);
        }
        Ok(())
    }

    /// Sets the node’s status to Active or Suspended.
    pub fn set_status(&self, node_id: &NodeId, status: NodeStatus) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.status = status;
        Ok(())
    }

    /// Applies a reputation penalty by subtracting `amount` from the node’s score. Sets status to Suspended if score falls below `suspension_threshold`.
    pub fn apply_reputation_penalty(&self, node_id: &NodeId, amount: u64, suspension_threshold: u64) -> Result<()> {
        let mut nodes = self.nodes.write().unwrap();
        let node = nodes.get_mut(node_id).ok_or_else(|| NodeRegistryError::NodeNotFound(node_id.clone()))?;
        node.reputation_score = node.reputation_score.saturating_sub(amount);
        if node.reputation_score < suspension_threshold {
            node.status = NodeStatus::Suspended;
        }
        Ok(())
    }

    /// Returns all nodes with status Active, sorted by `node_id` for deterministic ordering.
    pub fn get_eligible(&self) -> Vec<Node> {
        let nodes = self.nodes.read().unwrap();
        let mut v: Vec<Node> = nodes
            .values()
            .filter(|n| n.status == NodeStatus::Active)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        v
    }

    /// Returns eligible nodes together with their selection weight (reputation reduced by load). Sorted by `node_id`.
    pub fn get_eligible_with_weights(&self) -> Vec<(Node, u64)> {
        let mut v: Vec<(Node, u64)> = self
            .get_eligible()
            .into_iter()
            .map(|n| {
                let w = n.selection_weight();
                (n, w)
            })
            .collect();
        v.sort_by(|a, b| a.0.node_id.cmp(&b.0.node_id));
        v
    }

    /// Returns the number of registered nodes.
    pub fn len(&self) -> usize {
        let nodes = self.nodes.read().unwrap();
        nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_vote_accuracy() {
        let mut n = Node::new("n1".into(), "pk1".into(), 1000, 10);
        assert_eq!(n.vote_accuracy(), SCORE_SCALE);
        n.total_votes = 10;
        n.missed_votes = 2;
        assert_eq!(n.vote_accuracy(), (8 * SCORE_SCALE) / 10);
    }

    #[test]
    fn test_load_score() {
        let mut n = Node::new("n1".into(), "pk1".into(), 1000, 10);
        n.current_tasks = 5;
        n.recompute_load_score();
        assert_eq!(n.load_score, (5 * SCORE_SCALE) / 10);
        n.current_tasks = 10;
        n.recompute_load_score();
        assert_eq!(n.load_score, SCORE_SCALE);
    }

    #[test]
    fn test_reputation_formula() {
        let max_stake = 10_000u128;
        let mut n = Node::new("n1".into(), "pk1".into(), 5_000, 10);
        n.uptime_score = SCORE_SCALE;
        n.latency_score = SCORE_SCALE;
        n.total_votes = 10;
        n.missed_votes = 0;
        n.compute_reputation(max_stake);
        let expected_stake_w = (5_000 * SCORE_SCALE as u128 / 10_000) as u64;
        let expected = (SCORE_SCALE * 300 + SCORE_SCALE * 200 + SCORE_SCALE * 300 + expected_stake_w * 200) / 1000;
        assert_eq!(n.reputation_score, expected);
    }

    #[test]
    fn test_selection_weight_reduced_by_load() {
        let mut n = Node::new("n1".into(), "pk1".into(), 1000, 10);
        n.reputation_score = SCORE_SCALE;
        n.load_score = 0;
        assert_eq!(n.selection_weight(), SCORE_SCALE);
        n.load_score = SCORE_SCALE;
        assert_eq!(n.selection_weight(), 0);
        n.load_score = SCORE_SCALE / 2;
        assert_eq!(n.selection_weight(), SCORE_SCALE / 2);
    }

    #[test]
    fn test_registry_register_get() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        let n = reg.get(&"n1".into()).unwrap();
        assert_eq!(n.node_id, "n1");
        assert_eq!(n.stake, 1000);
        assert_eq!(n.max_capacity, 10);
    }

    #[test]
    fn test_registry_duplicate() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        let r = reg.register("n1".into(), "pk2".into(), 2000, 20);
        assert!(r.is_err());
    }

    #[test]
    fn test_set_load_recomputes_load_score() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        reg.set_load(&"n1".into(), 5, 10).unwrap();
        let n = reg.get(&"n1".into()).unwrap();
        assert_eq!(n.current_tasks, 5);
        assert_eq!(n.load_score, (5 * SCORE_SCALE) / 10);
    }

    #[test]
    fn test_eligible_excludes_suspended() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        reg.register("n2".into(), "pk2".into(), 1000, 10).unwrap();
        reg.set_status(&"n2".into(), NodeStatus::Suspended).unwrap();
        let eligible = reg.get_eligible();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].node_id, "n1");
    }

    #[test]
    fn test_determinism_same_inputs_same_reputation() {
        let reg = NodeRegistry::new();
        reg.register("n1".into(), "pk1".into(), 1000, 10).unwrap();
        reg.set_uptime_score(&"n1".into(), 800_000).unwrap();
        reg.set_latency_score(&"n1".into(), 900_000).unwrap();
        let n1 = reg.get(&"n1".into()).unwrap();
        let n2 = reg.get(&"n1".into()).unwrap();
        assert_eq!(n1.reputation_score, n2.reputation_score);
        assert_eq!(n1.selection_weight(), n2.selection_weight());
    }
}
