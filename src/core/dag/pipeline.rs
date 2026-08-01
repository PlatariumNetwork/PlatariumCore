//! Pack selected digests into an ephemeral DAG and linearize (confirm wiring).

use crate::core::dag::linearize::{linearize, LinearizeResult};
use crate::core::dag::store::DagStore;
use crate::core::dag::types::{AuthorId, DagVertex};
use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrderDigestsResult {
    pub digests: Vec<String>,
    pub vertex_order: Vec<String>,
    pub tip: String,
}

/// Build genesis → one vertex per digest → tip; return linearized TX digests.
/// Duplicate digests are dropped (first-seen order for vertex creation; output is DAG order).
pub fn order_digests(producer: AuthorId, digests: &[String]) -> Result<OrderDigestsResult> {
    if producer.is_empty() {
        return Err(PlatariumError::State("dag: producer required".into()));
    }

    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for d in digests {
        if d.is_empty() {
            continue;
        }
        if seen.insert(d.clone()) {
            unique.push(d.clone());
        }
    }

    let mut store = DagStore::new();
    let genesis = DagVertex::genesis(producer.clone());
    let gid = store.insert(genesis)?;

    if unique.is_empty() {
        let lin = linearize(&store, &gid)?;
        return Ok(OrderDigestsResult {
            digests: lin.digests,
            vertex_order: lin.vertex_order,
            tip: gid,
        });
    }

    let mut round1_ids = Vec::with_capacity(unique.len());
    for d in &unique {
        let v = DagVertex::new(1, producer.clone(), vec![gid.clone()], vec![d.clone()]);
        round1_ids.push(store.insert(v)?);
    }

    let tip = DagVertex::new(2, producer.clone(), round1_ids, Vec::new());
    let tip_id = store.insert(tip)?;
    let LinearizeResult {
        vertex_order,
        digests: ordered,
    } = linearize(&store, &tip_id)?;

    Ok(OrderDigestsResult {
        digests: ordered,
        vertex_order,
        tip: tip_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_digests_returns_genesis_only() {
        let r = order_digests("n0".into(), &[]).unwrap();
        assert!(r.digests.is_empty());
        assert_eq!(r.vertex_order.len(), 1);
    }

    #[test]
    fn same_set_different_input_order_same_output() {
        let a = vec!["tx_zzz".into(), "tx_aaa".into(), "tx_mmm".into()];
        let b = vec!["tx_mmm".into(), "tx_zzz".into(), "tx_aaa".into()];
        let ra = order_digests("producer".into(), &a).unwrap();
        let rb = order_digests("producer".into(), &b).unwrap();
        assert_eq!(ra.digests, rb.digests);
        assert_eq!(ra.tip, rb.tip);
        assert_eq!(ra.digests.len(), 3);
    }

    #[test]
    fn dedupes_duplicates() {
        let d = vec!["a".into(), "a".into(), "b".into()];
        let r = order_digests("n0".into(), &d).unwrap();
        assert_eq!(r.digests.len(), 2);
    }

    #[test]
    fn rejects_empty_producer() {
        assert!(order_digests("".into(), &["x".into()]).is_err());
    }
}
