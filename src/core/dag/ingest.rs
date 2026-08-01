//! Peer ingest: wire-id verify + missing-parent pending buffer.

use crate::core::dag::store::DagStore;
use crate::core::dag::types::{DagVertex, VertexId};
use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

pub const PENDING_MAX: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IngestStatus {
    Inserted,
    Pending,
    Duplicate,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestResult {
    pub status: IngestStatus,
    pub id: VertexId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_parents: Vec<VertexId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flushed: Vec<VertexId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct PendingQueue {
    /// Insertion order for overflow eviction.
    order: VecDeque<VertexId>,
    by_id: HashMap<VertexId, DagVertex>,
}

impl PendingQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    fn push(&mut self, v: DagVertex) {
        let id = v.id.clone();
        if self.by_id.contains_key(&id) {
            return;
        }
        while self.by_id.len() >= PENDING_MAX {
            if let Some(old) = self.order.pop_front() {
                self.by_id.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(id.clone());
        self.by_id.insert(id, v);
    }

    fn take(&mut self, id: &str) -> Option<DagVertex> {
        let v = self.by_id.remove(id)?;
        self.order.retain(|x| x != id);
        Some(v)
    }

    fn ids(&self) -> Vec<VertexId> {
        self.order.iter().cloned().collect()
    }
}

fn missing_parents(store: &DagStore, v: &DagVertex) -> Vec<VertexId> {
    v.parents
        .iter()
        .filter(|p| store.get(p).is_none())
        .cloned()
        .collect()
}

/// Insert vertex or buffer if parents missing. After any insert, flush pending.
pub fn ingest(store: &mut DagStore, pending: &mut PendingQueue, vertex: DagVertex) -> Result<IngestResult> {
    let id = vertex.id.clone();

    if store.get(&id).is_some() {
        return Ok(IngestResult {
            status: IngestStatus::Duplicate,
            id,
            missing_parents: Vec::new(),
            flushed: Vec::new(),
            error: None,
        });
    }
    if pending.by_id.contains_key(&id) {
        return Ok(IngestResult {
            status: IngestStatus::Duplicate,
            id,
            missing_parents: Vec::new(),
            flushed: Vec::new(),
            error: None,
        });
    }

    let missing = missing_parents(store, &vertex);
    if !missing.is_empty() {
        // Genesis must not wait on parents — insert path validates separately.
        if vertex.round == 0 {
            return Ok(IngestResult {
                status: IngestStatus::Rejected,
                id,
                missing_parents: missing,
                flushed: Vec::new(),
                error: Some("dag: genesis cannot be pending".into()),
            });
        }
        pending.push(vertex);
        return Ok(IngestResult {
            status: IngestStatus::Pending,
            id,
            missing_parents: missing,
            flushed: Vec::new(),
            error: None,
        });
    }

    match store.insert(vertex) {
        Ok(inserted_id) => {
            let flushed = flush_pending(store, pending);
            Ok(IngestResult {
                status: IngestStatus::Inserted,
                id: inserted_id,
                missing_parents: Vec::new(),
                flushed,
                error: None,
            })
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("duplicate") {
                Ok(IngestResult {
                    status: IngestStatus::Duplicate,
                    id,
                    missing_parents: Vec::new(),
                    flushed: Vec::new(),
                    error: None,
                })
            } else {
                Ok(IngestResult {
                    status: IngestStatus::Rejected,
                    id,
                    missing_parents: Vec::new(),
                    flushed: Vec::new(),
                    error: Some(msg),
                })
            }
        }
    }
}

fn flush_pending(store: &mut DagStore, pending: &mut PendingQueue) -> Vec<VertexId> {
    let mut flushed = Vec::new();
    loop {
        let mut progress = false;
        let ids = pending.ids();
        for pid in ids {
            let Some(v) = pending.by_id.get(&pid).cloned() else {
                continue;
            };
            if !missing_parents(store, &v).is_empty() {
                continue;
            }
            let Some(v) = pending.take(&pid) else {
                continue;
            };
            match store.insert(v) {
                Ok(id) => {
                    flushed.push(id);
                    progress = true;
                }
                Err(_) => {
                    // Drop un-insertable pending (e.g. bad genesis rules).
                }
            }
        }
        if !progress {
            break;
        }
    }
    flushed
}

/// Parse wire JSON fields into a verified DagVertex.
pub fn vertex_from_params(
    id: Option<String>,
    round: u64,
    author: String,
    parents: Vec<String>,
    tx_digests: Vec<String>,
) -> Result<DagVertex> {
    if let Some(id) = id {
        DagVertex::from_wire(id, round, author, parents, tx_digests)
            .map_err(PlatariumError::State)
    } else {
        Ok(DagVertex::new(round, author, parents, tx_digests))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_id_mismatch_rejected() {
        let err = DagVertex::from_wire(
            "deadbeef".into(),
            1,
            "n0".into(),
            vec!["p".into()],
            vec!["t".into()],
        );
        assert!(err.unwrap_err().contains("wire id mismatch"));
    }

    #[test]
    fn honest_wire_ok() {
        let v = DagVertex::new(1, "n0".into(), vec!["p".into()], vec!["t".into()]);
        let w = DagVertex::from_wire(
            v.id.clone(),
            v.round,
            v.author.clone(),
            v.parents.clone(),
            v.tx_digests.clone(),
        )
        .unwrap();
        assert_eq!(v, w);
    }

    #[test]
    fn child_pending_then_flush_on_parent() {
        let mut store = DagStore::new();
        let mut pending = PendingQueue::new();
        let g = DagVertex::genesis("n0".into());
        let gid = g.id.clone();

        let child = DagVertex::new(1, "n1".into(), vec![gid.clone()], vec!["tx1".into()]);
        let r1 = ingest(&mut store, &mut pending, child.clone()).unwrap();
        assert_eq!(r1.status, IngestStatus::Pending);
        assert_eq!(pending.len(), 1);
        assert!(store.get(&child.id).is_none());

        let r2 = ingest(&mut store, &mut pending, g).unwrap();
        assert_eq!(r2.status, IngestStatus::Inserted);
        assert!(r2.flushed.contains(&child.id));
        assert!(store.get(&child.id).is_some());
        assert!(pending.is_empty());
    }

    #[test]
    fn duplicate_after_insert() {
        let mut store = DagStore::new();
        let mut pending = PendingQueue::new();
        let g = DagVertex::genesis("n0".into());
        assert_eq!(
            ingest(&mut store, &mut pending, g.clone())
                .unwrap()
                .status,
            IngestStatus::Inserted
        );
        assert_eq!(
            ingest(&mut store, &mut pending, g).unwrap().status,
            IngestStatus::Duplicate
        );
    }
}
