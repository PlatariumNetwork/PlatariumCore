//! Network-wide shared genesis (identical on every node).

use crate::core::dag::types::DagVertex;

/// Fixed author so all nodes compute the same genesis vertex id.
pub const SHARED_GENESIS_AUTHOR: &str = "platarium-genesis";

/// Deterministic shared genesis vertex (round 0, empty parents/payloads).
pub fn shared_genesis() -> DagVertex {
    DagVertex::genesis(SHARED_GENESIS_AUTHOR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_genesis_id_stable() {
        let a = shared_genesis();
        let b = shared_genesis();
        assert_eq!(a.id, b.id);
        assert_eq!(a.round, 0);
        assert_eq!(a.author, SHARED_GENESIS_AUTHOR);
        assert!(a.parents.is_empty());
        assert!(a.tx_digests.is_empty());
    }
}
