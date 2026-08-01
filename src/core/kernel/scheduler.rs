//! Deterministic conflict waves for parallel execution.

use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::kernel::touch::conflict_touch_set;
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
    let n = batch.transactions.len();
    if n == 0 {
        return Vec::new();
    }
    let touches: Vec<_> = batch.transactions.iter().map(conflict_touch_set).collect();
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
}
