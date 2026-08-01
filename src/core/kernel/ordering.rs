//! Ordering facade: digests + payloads → OrderedBatch (no ledger reads).

use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use std::collections::HashMap;

/// Build an OrderedBatch from digest order and payload map.
/// Missing payload → Availability error (does not skip).
pub fn build_ordered_batch(
    batch_id: String,
    height: u64,
    digests: &[String],
    payloads: &HashMap<String, Transaction>,
) -> Result<OrderedBatch> {
    let mut transactions = Vec::with_capacity(digests.len());
    for d in digests {
        let tx = payloads.get(d).ok_or_else(|| {
            PlatariumError::State(format!("availability: missing payload for digest {}", d))
        })?;
        if &tx.hash != d {
            return Err(PlatariumError::State(format!(
                "availability: payload hash {} != digest {}",
                tx.hash, d
            )));
        }
        transactions.push(tx.clone());
    }
    OrderedBatch::new(batch_id, height, transactions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use std::collections::HashSet;

    #[test]
    fn missing_payload_errors() {
        let err = build_ordered_batch("b".into(), 1, &["h1".into()], &HashMap::new());
        assert!(err.unwrap_err().to_string().contains("availability"));
    }

    #[test]
    fn builds_in_digest_order() {
        let t1 = Transaction::new(
            "a".into(),
            "b".into(),
            Asset::PLP,
            1,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "s".into(),
            "s".into(),
        )
        .unwrap();
        let t2 = Transaction::new(
            "c".into(),
            "d".into(),
            Asset::PLP,
            1,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "s".into(),
            "s".into(),
        )
        .unwrap();
        let mut map = HashMap::new();
        map.insert(t1.hash.clone(), t1.clone());
        map.insert(t2.hash.clone(), t2.clone());
        let batch = build_ordered_batch(
            "b".into(),
            2,
            &[t2.hash.clone(), t1.hash.clone()],
            &map,
        )
        .unwrap();
        assert_eq!(batch.transactions[0].hash, t2.hash);
        assert_eq!(batch.transactions[1].hash, t1.hash);
    }
}
