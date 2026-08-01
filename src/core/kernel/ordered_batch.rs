//! Ordered batch of transactions (ordering layer output).

use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};

/// Consensus/ordering output: digests in commit order with resolved payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderedBatch {
    pub batch_id: String,
    #[serde(default)]
    pub height: u64,
    pub tx_digests: Vec<String>,
    pub transactions: Vec<Transaction>,
}

impl OrderedBatch {
    /// Build and validate digest↔hash alignment.
    pub fn new(
        batch_id: String,
        height: u64,
        transactions: Vec<Transaction>,
    ) -> Result<Self> {
        let tx_digests: Vec<String> = transactions.iter().map(|t| t.hash.clone()).collect();
        let batch = Self {
            batch_id,
            height,
            tx_digests,
            transactions,
        };
        batch.validate()?;
        Ok(batch)
    }

    /// Ensure `tx_digests[i] == transactions[i].hash` and lengths match.
    pub fn validate(&self) -> Result<()> {
        if self.tx_digests.len() != self.transactions.len() {
            return Err(PlatariumError::State(format!(
                "OrderedBatch length mismatch: digests={} txs={}",
                self.tx_digests.len(),
                self.transactions.len()
            )));
        }
        for (i, (d, tx)) in self
            .tx_digests
            .iter()
            .zip(self.transactions.iter())
            .enumerate()
        {
            if d != &tx.hash {
                return Err(PlatariumError::State(format!(
                    "OrderedBatch digest mismatch at {}: digest={} hash={}",
                    i, d, tx.hash
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use std::collections::HashSet;

    #[test]
    fn rejects_digest_mismatch() {
        let tx = Transaction::new(
            "a".into(),
            "b".into(),
            Asset::PLP,
            1,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "aa".into(),
            "bb".into(),
        )
        .unwrap();
        let batch = OrderedBatch {
            batch_id: "b1".into(),
            height: 1,
            tx_digests: vec!["wrong".into()],
            transactions: vec![tx],
        };
        assert!(batch.validate().is_err());
    }
}
