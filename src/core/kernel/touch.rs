//! Conflict touch sets for scheduling.

use crate::core::state::{State, TREASURY_ADDRESS};
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

/// Like [`touch_set`], but also merges lock-time escrow bindings from `state` (R2-M7).
pub fn touch_set_with_state(tx: &Transaction, state: Option<&State>) -> BTreeSet<String> {
    let mut set = conflict_touch_set_with_state(tx, state);
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
    conflict_touch_set_with_state(tx, None)
}

/// Conflict touch set plus lock-time escrow creator/beneficiary/node from `state` (R2-M7).
///
/// When settle omits `settle_payee`/`settle_node` (R2-H5 lock-time bindings), scheduling
/// still conflicts with transfers to those effect addresses.
pub fn conflict_touch_set_with_state(tx: &Transaction, state: Option<&State>) -> BTreeSet<String> {
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
    // Lock-time bindings for escrow effect txs (settle may omit payee/node fields).
    if is_escrow_effect_kind(tx) {
        if let (Some(st), Some(eid)) = (state, tx.escrow_id()) {
            if let Some(entry) = st.get_escrow(eid) {
                if !entry.creator.is_empty() {
                    set.insert(entry.creator);
                }
                if !entry.beneficiary.is_empty() {
                    set.insert(entry.beneficiary);
                }
                if !entry.node.is_empty() {
                    set.insert(entry.node);
                }
            }
        }
        // Escrow lock: settle_payee/settle_node on the lock tx are the bindings being fixed.
        if matches!(tx.effective_tx_kind().unwrap_or(""), TX_KIND_ESCROW_LOCK) {
            if let Some(payee) = tx.settle_payee.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                set.insert(payee.to_string());
            }
            if let Some(node) = tx.settle_node.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                set.insert(node.to_string());
            }
        }
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

    #[test]
    fn lock_time_bindings_touched_when_settle_fields_omitted() {
        use crate::modules::contacteconomy::PURPOSE_CONTACT;
        let state = State::new();
        let creator = "Creator".to_string();
        let payee = "PayeeP".to_string();
        let node = "NodeN".to_string();
        state.set_balance(&creator, 1_000_000);
        state.set_uplp_balance(&creator, 10);
        state
            .escrow_lock(
                &creator,
                "eid-touch",
                &payee,
                &node,
                100_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();

        let mut settle = Transaction::new(
            creator.clone(),
            "DummyTo".into(),
            Asset::PLP,
            100_000,
            1,
            1,
            HashSet::new(),
            HashSet::new(),
            "s1".into(),
            "s2".into(),
        )
        .unwrap();
        settle.tx_kind = Some(TX_KIND_ESCROW_SETTLE.into());
        settle.escrow_id = Some("eid-touch".into());
        settle.settle_outcome_key = Some("timeout".into());
        // R2-H5: omit settle_payee / settle_node — credits come from lock bindings.
        settle.settle_payee = None;
        settle.settle_node = None;

        let without = conflict_touch_set(&settle);
        assert!(!without.contains(&payee));
        assert!(!without.contains(&node));

        let with = conflict_touch_set_with_state(&settle, Some(&state));
        assert!(with.contains(&payee), "lock-time payee must conflict");
        assert!(with.contains(&node), "lock-time node must conflict");
        assert!(with.contains(&creator));
    }
}
