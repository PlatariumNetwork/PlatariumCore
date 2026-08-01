//! Conflict touch sets for scheduling.

use crate::core::state::TREASURY_ADDRESS;
use crate::core::transaction::Transaction;
use std::collections::BTreeSet;

/// Addresses touched by a tx (for StateDiff account collection).
/// Includes treasury because fees credit the fee sink.
pub fn touch_set(tx: &Transaction) -> BTreeSet<String> {
    let mut set = conflict_touch_set(tx);
    set.insert(TREASURY_ADDRESS.to_string());
    set
}

/// Addresses that conflict if shared across concurrent txs in a wave.
/// Treasury is intentionally **excluded**: fee credits are commutative and
/// applied in batch-index order during merge (see `execute_parallel_waves`).
pub fn conflict_touch_set(tx: &Transaction) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    set.insert(tx.from.clone());
    set.insert(tx.to.clone());
    for a in &tx.reads {
        set.insert(a.clone());
    }
    for a in &tx.writes {
        set.insert(a.clone());
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use std::collections::HashSet;

    #[test]
    fn includes_endpoints_and_treasury() {
        let tx = Transaction::new(
            "Alice".into(),
            "Bob".into(),
            Asset::PLP,
            10,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "s1".into(),
            "s2".into(),
        )
        .unwrap();
        let t = touch_set(&tx);
        assert!(t.contains("Alice"));
        assert!(t.contains("Bob"));
        assert!(t.contains(TREASURY_ADDRESS));
    }
}
