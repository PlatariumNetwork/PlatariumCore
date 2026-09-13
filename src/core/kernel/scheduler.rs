//! Deterministic conflict waves for parallel execution.

use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::kernel::touch::conflict_touch_set_with_state;
use crate::core::state::State;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionWave {
    pub wave_index: u32,
    /// Batch indices, sorted ascending for determinism.
    pub tx_indices: Vec<u32>,
}

/// Build undirected conflict graph and greedy waves.
/// Two txs conflict if their touch sets intersect.
pub fn compute_waves(batch: &OrderedBatch) -> Vec<ExecutionWave> {
    compute_waves_with_state(batch, None)
}

/// Like [`compute_waves`], using `state` so escrow settle conflicts include lock-time
/// beneficiary/node even when `settle_payee`/`settle_node` are omitted (R2-M7).
pub fn compute_waves_with_state(batch: &OrderedBatch, state: Option<&State>) -> Vec<ExecutionWave> {
    let n = batch.transactions.len();
    if n == 0 {
        return Vec::new();
    }
    let touches: Vec<_> = batch
        .transactions
        .iter()
        .map(|tx| conflict_touch_set_with_state(tx, state))
        .collect();
    let mut conflict = vec![vec![false; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let overlap = touches[i].intersection(&touches[j]).next().is_some();
            if overlap {
                conflict[i][j] = true;
                conflict[j][i] = true;
            }
        }
    }

    let mut assigned = vec![false; n];
    let mut waves = Vec::new();
    let mut wave_index = 0u32;
    let mut remaining = n;
    while remaining > 0 {
        let mut wave = Vec::new();
        for i in 0..n {
            if assigned[i] {
                continue;
            }
            let ok = wave.iter().all(|&j| !conflict[i][j as usize]);
            if ok {
                wave.push(i as u32);
                assigned[i] = true;
                remaining -= 1;
            }
        }
        wave.sort_unstable();
        waves.push(ExecutionWave {
            wave_index,
            tx_indices: wave,
        });
        wave_index += 1;
    }
    waves
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::transaction::Transaction;
    use crate::modules::contacteconomy::PURPOSE_CONTACT;
    use crate::modules::escrow::types::TX_KIND_ESCROW_SETTLE;
    use std::collections::HashSet;

    fn dummy_tx(from: &str, to: &str, nonce: u64) -> Transaction {
        Transaction::new(
            from.into(),
            to.into(),
            Asset::PLP,
            1,
            1,
            nonce,
            HashSet::new(),
            HashSet::new(),
            "aa".into(),
            "bb".into(),
        )
        .unwrap()
    }

    #[test]
    fn overlapping_from_never_same_wave() {
        let batch = OrderedBatch::new(
            "b".into(),
            1,
            vec![
                dummy_tx("A", "B", 0),
                dummy_tx("C", "D", 0),
                dummy_tx("A", "E", 1),
            ],
        )
        .unwrap();
        let waves = compute_waves(&batch);
        let mut wave_of = vec![0u32; 3];
        for w in &waves {
            for &i in &w.tx_indices {
                wave_of[i as usize] = w.wave_index;
            }
        }
        assert_ne!(wave_of[0], wave_of[2]);
        assert_eq!(wave_of[0], wave_of[1]); // A-B and C-D independent
    }

    #[test]
    fn settle_without_payee_fields_conflicts_with_transfer_to_lock_payee() {
        let state = State::new();
        let creator = "Lo".to_string();
        let payee = "PayeeP".to_string();
        let node = "NodeN".to_string();
        state.set_balance(&creator, 1_000_000);
        state.set_uplp_balance(&creator, 20);
        state.set_balance(&"SenderT".to_string(), 1_000);
        state.set_uplp_balance(&"SenderT".to_string(), 5);
        state
            .escrow_lock(
                &creator,
                "eid-wave",
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

        let mut settle = dummy_tx(&creator, "DummyTo", 1);
        settle.tx_kind = Some(TX_KIND_ESCROW_SETTLE.into());
        settle.escrow_id = Some("eid-wave".into());
        settle.settle_outcome_key = Some("timeout".into());
        settle.amount = 100_000;
        settle.settle_payee = None;
        settle.settle_node = None;

        let transfer = dummy_tx("SenderT", &payee, 0);
        let batch = OrderedBatch::new("b2".into(), 1, vec![settle, transfer]).unwrap();

        let naive = compute_waves(&batch);
        let mut naive_wave = [0u32; 2];
        for w in &naive {
            for &i in &w.tx_indices {
                naive_wave[i as usize] = w.wave_index;
            }
        }
        assert_eq!(
            naive_wave[0], naive_wave[1],
            "without state, settle+transfer-to-payee incorrectly share a wave"
        );

        let fixed = compute_waves_with_state(&batch, Some(&state));
        let mut fixed_wave = [0u32; 2];
        for w in &fixed {
            for &i in &w.tx_indices {
                fixed_wave[i as usize] = w.wave_index;
            }
        }
        assert_ne!(
            fixed_wave[0], fixed_wave[1],
            "with lock-time bindings, settle must not share a wave with transfer-to-payee"
        );
    }
}
