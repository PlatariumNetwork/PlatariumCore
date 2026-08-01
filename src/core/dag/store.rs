//! In-memory DAG store with parent availability checks.

use crate::core::dag::types::{DagVertex, VertexId};
use crate::error::{PlatariumError, Result};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Default, Clone)]
pub struct DagStore {
    by_id: HashMap<VertexId, DagVertex>,
    by_round: BTreeMap<u64, Vec<VertexId>>,
}

/// Process-global DAG store + pending queue for RPC / P2P ingest.
pub fn global_dag_store() -> &'static Mutex<DagStore> {
    static STORE: OnceLock<Mutex<DagStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(DagStore::new()))
}

pub fn global_pending_queue() -> &'static Mutex<crate::core::dag::ingest::PendingQueue> {
    static PENDING: OnceLock<Mutex<crate::core::dag::ingest::PendingQueue>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(crate::core::dag::ingest::PendingQueue::new()))
}

pub fn reset_global_dag_store() {
    if let Ok(mut g) = global_dag_store().lock() {
        *g = DagStore::new();
    }
    if let Ok(mut p) = global_pending_queue().lock() {
        *p = crate::core::dag::ingest::PendingQueue::new();
    }
    crate::core::dag::last_commit::clear_last_commit();
}

impl DagStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &str) -> Option<&DagVertex> {
        self.by_id.get(id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn vertices_in_round(&self, round: u64) -> Vec<&DagVertex> {
        self.by_round
            .get(&round)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.by_id.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Insert vertex. Parents must already exist (except genesis round 0 with empty parents).
    pub fn insert(&mut self, vertex: DagVertex) -> Result<VertexId> {
        if self.by_id.contains_key(&vertex.id) {
            return Err(PlatariumError::State(format!(
                "dag: duplicate vertex {}",
                vertex.id
            )));
        }
        if vertex.round == 0 {
            if !vertex.parents.is_empty() {
                return Err(PlatariumError::State(
                    "dag: genesis must have empty parents".into(),
                ));
            }
        } else {
            if vertex.parents.is_empty() {
                return Err(PlatariumError::State(
                    "dag: non-genesis requires parents".into(),
                ));
            }
            for p in &vertex.parents {
                if !self.by_id.contains_key(p) {
                    return Err(PlatariumError::State(format!(
                        "availability: missing parent {}",
                        p
                    )));
                }
            }
        }
        let id = vertex.id.clone();
        let round = vertex.round;
        self.by_round.entry(round).or_default().push(id.clone());
        self.by_id.insert(id.clone(), vertex);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dag::types::DagVertex;

    #[test]
    fn genesis_ok_missing_parent_fails_duplicate_fails() {
        let mut store = DagStore::new();
        let g = DagVertex::genesis("n0".into());
        let gid = store.insert(g.clone()).unwrap();
        assert_eq!(store.len(), 1);

        let bad = DagVertex::new(1, "n1".into(), vec!["missing".into()], vec!["t".into()]);
        let err = store.insert(bad).unwrap_err().to_string();
        assert!(err.contains("availability: missing parent"));

        let child = DagVertex::new(1, "n1".into(), vec![gid.clone()], vec!["t1".into()]);
        store.insert(child.clone()).unwrap();
        let dup = store.insert(child).unwrap_err().to_string();
        assert!(dup.contains("duplicate"));
    }
}
