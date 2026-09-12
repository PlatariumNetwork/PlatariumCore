//! Conflict touch sets for scheduling.

use crate::core::state::TREASURY_ADDRESS;
use crate::core::transaction::Transaction;
use crate::modules::escrow::types::{
    BURN_ROLE, TX_KIND_ESCROW_CANCEL, TX_KIND_ESCROW_LOCK, TX_KIND_ESCROW_REFUND,
    TX_KIND_ESCROW_SETTLE,
};
use std::collections::BTreeSet;

fn is_escrow_effect_kind(tx: &Transaction) -> bool {
    matches!(
        tx.effective_tx_kind().unwrap_or(""),
        TX_KIND_ESCROW_LOCK
            | TX_KIND_ESCROW_SETTLE
            | TX_KIND_ESCROW_REFUND
            | TX_KIND_ESCROW_CANCEL
    )
}

/// Addresses touched by a tx (for StateDiff account collection).
/// Includes treasury because fees credit the fee sink.
/// R2-M7: escrow settle/refund/cancel also touch the burn role sink.
pub fn touch_set(tx: &Transaction) -> BTreeSet<String> {
    let mut set = conflict_touch_set(tx);
    set.insert(TREASURY_ADDRESS.to_string());
    if is_escrow_effect_kind(tx) {
        set.insert(BURN_ROLE.to_string());
    }
    set
}

/// Addresses that conflict if shared across concurrent txs in a wave.
/// Treasury/burn are intentionally **excluded**: fee/burn credits are commutative and
/// applied in batch-index order during merge (see `execute_parallel_waves`).
///
/// R2-M7: include `settle_payee` / `settle_node` so escrow settle credits cannot
/// drop out of StateDiff / parallel conflict detection.
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
    if let Some(payee) = tx.settle_payee.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        set.insert(payee.to_string());
    }
    if let Some(node) = tx.settle_node.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        set.insert(node.to_string());
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

    #[test]
    fn settle_payee_and_node_are_touched() {
        let mut tx = Transaction::new(
            "Settler".into(),
            "EscrowTo".into(),
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
        tx.tx_kind = Some(TX_KIND_ESCROW_SETTLE.into());
        tx.settle_payee = Some("PayeeAddr".into());
        tx.settle_node = Some("NodeAddr".into());
        let t = conflict_touch_set(&tx);
        assert!(t.contains("PayeeAddr"));
        assert!(t.contains("NodeAddr"));
        assert!(t.contains("Settler"));
        let full = touch_set(&tx);
        assert!(full.contains(BURN_ROLE));
        assert!(full.contains(TREASURY_ADDRESS));
        assert!(!t.contains(BURN_ROLE), "burn must not serialize parallel waves");
    }
}
