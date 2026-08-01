//! Bullshark-lite commit: leader schedule + 2f+1 support.

use crate::core::dag::linearize::{linearize, LinearizeResult};
use crate::core::dag::store::DagStore;
use crate::core::dag::types::{AuthorId, CommitteeConfig, VertexId};
use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitOutcome {
    pub round: u64,
    pub anchor: VertexId,
    pub digests: Vec<String>,
    pub vertex_order: Vec<VertexId>,
}

/// Deterministic leader for `round`: committee[round % n].
pub fn leader_for_round(committee: &CommitteeConfig, round: u64) -> Result<AuthorId> {
    committee.validate().map_err(PlatariumError::State)?;
    let n = committee.authors.len() as u64;
    let idx = (round % n) as usize;
    Ok(committee.authors[idx].clone())
}

/// Try to commit leader of `support_round - 1` when ≥ quorum vertices in `support_round`
/// include that leader vertex as a parent.
///
/// `support_round` must be ≥ 1. Genesis (round 0) is committed implicitly when present.
pub fn try_commit(
    store: &DagStore,
    committee: &CommitteeConfig,
    support_round: u64,
) -> Result<Option<CommitOutcome>> {
    committee.validate().map_err(PlatariumError::State)?;
    if support_round == 0 {
        return Err(PlatariumError::State(
            "dag: try_commit support_round must be >= 1".into(),
        ));
    }
    let leader_round = support_round - 1;
    let leader_author = leader_for_round(committee, leader_round)?;

    // Find leader vertex in leader_round (one per author expected; take matching author).
    let leader_verts: Vec<_> = store
        .vertices_in_round(leader_round)
        .into_iter()
        .filter(|v| v.author == leader_author)
        .collect();
    if leader_verts.is_empty() {
        return Ok(None);
    }
    if leader_verts.len() > 1 {
        return Err(PlatariumError::State(format!(
            "dag: multiple leader vertices for {} at round {}",
            leader_author, leader_round
        )));
    }
    let anchor = leader_verts[0].id.clone();

    let quorum = committee.quorum();
    let supporters = store
        .vertices_in_round(support_round)
        .into_iter()
        .filter(|v| v.parents.iter().any(|p| p == &anchor))
        .count();
    if supporters < quorum {
        return Ok(None);
    }

    let LinearizeResult {
        vertex_order,
        digests,
    } = linearize(store, &anchor)?;
    Ok(Some(CommitOutcome {
        round: leader_round,
        anchor,
        digests,
        vertex_order,
    }))
}

/// Commit when ≥ quorum distinct committee authors published `batch_round` vertices
/// that parent the shared genesis. Digests = flatten vertices sorted by (author, id).
pub fn try_commit_batches(
    store: &DagStore,
    committee: &CommitteeConfig,
    batch_round: u64,
    genesis_id: &str,
) -> Result<Option<CommitOutcome>> {
    committee.validate().map_err(PlatariumError::State)?;
    if batch_round == 0 {
        return Err(PlatariumError::State(
            "dag: try_commit_batches batch_round must be >= 1".into(),
        ));
    }
    if store.get(genesis_id).is_none() {
        return Ok(None);
    }

    let committee_set: std::collections::BTreeSet<_> =
        committee.authors.iter().cloned().collect();

    let mut batches: Vec<&crate::core::dag::types::DagVertex> = store
        .vertices_in_round(batch_round)
        .into_iter()
        .filter(|v| {
            committee_set.contains(&v.author)
                && v.parents.iter().any(|p| p == genesis_id)
        })
        .collect();

    // One vertex per author (if multiple, keep lowest vertex id for determinism).
    batches.sort_by(|a, b| (&a.author, &a.id).cmp(&(&b.author, &b.id)));
    let mut seen_authors = std::collections::BTreeSet::new();
    batches.retain(|v| seen_authors.insert(v.author.clone()));

    if batches.len() < committee.quorum() {
        return Ok(None);
    }

    let mut digests = Vec::new();
    let mut vertex_order = Vec::new();
    vertex_order.push(genesis_id.to_string());
    for v in &batches {
        vertex_order.push(v.id.clone());
        digests.extend(v.tx_digests.iter().cloned());
    }

    // Anchor = first batch vertex id (deterministic after sort).
    let anchor = batches[0].id.clone();
    Ok(Some(CommitOutcome {
        round: batch_round,
        anchor,
        digests,
        vertex_order,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dag::types::DagVertex;

    fn committee4() -> CommitteeConfig {
        CommitteeConfig {
            authors: vec!["n0".into(), "n1".into(), "n2".into(), "n3".into()],
            f: 1,
        }
    }

    #[test]
    fn leader_schedule_stable() {
        let c = committee4();
        assert_eq!(leader_for_round(&c, 0).unwrap(), "n0");
        assert_eq!(leader_for_round(&c, 1).unwrap(), "n1");
        assert_eq!(leader_for_round(&c, 4).unwrap(), "n0");
    }

    #[test]
    fn commit_requires_quorum() {
        let c = committee4();
        let mut store = DagStore::new();
        // Round 0: all authors genesis-like under n0 only for simplicity — use one genesis.
        let g = DagVertex::genesis("n0".into());
        let gid = store.insert(g).unwrap();

        // Round 0 leader is n0; their vertex is genesis.
        // Round 1: need 3 supporters that parent genesis for commit of round 0.
        let mut support_ids = Vec::new();
        for (i, author) in ["n0", "n1", "n2"].iter().enumerate() {
            let v = DagVertex::new(
                1,
                (*author).into(),
                vec![gid.clone()],
                vec![format!("t{}", i)],
            );
            support_ids.push(store.insert(v).unwrap());
        }

        // Only 2 supporters → not enough (need 3).
        let mut store2 = DagStore::new();
        let gid2 = store2.insert(DagVertex::genesis("n0".into())).unwrap();
        for (i, author) in ["n0", "n1"].iter().enumerate() {
            store2
                .insert(DagVertex::new(
                    1,
                    (*author).into(),
                    vec![gid2.clone()],
                    vec![format!("t{}", i)],
                ))
                .unwrap();
        }
        assert!(try_commit(&store2, &c, 1).unwrap().is_none());

        let out = try_commit(&store, &c, 1).unwrap().expect("committed");
        assert_eq!(out.anchor, gid);
        assert!(!out.digests.is_empty() || out.vertex_order.len() == 1);
        let _ = support_ids;
    }

    #[test]
    fn try_commit_batches_quorum_and_single() {
        use crate::core::dag::genesis::shared_genesis;

        let g = shared_genesis();
        let gid = g.id.clone();

        // Single node f=0
        let c1 = CommitteeConfig::from_authors(vec!["n0".into()]).unwrap();
        let mut s1 = DagStore::new();
        s1.insert(g.clone()).unwrap();
        assert!(try_commit_batches(&s1, &c1, 1, &gid).unwrap().is_none());
        s1.insert(DagVertex::new(
            1,
            "n0".into(),
            vec![gid.clone()],
            vec!["txA".into()],
        ))
        .unwrap();
        let out = try_commit_batches(&s1, &c1, 1, &gid)
            .unwrap()
            .expect("single commit");
        assert_eq!(out.digests, vec!["txA"]);

        // 3 authors f=1 need 3
        let c3 = CommitteeConfig::from_authors(vec![
            "n0".into(),
            "n1".into(),
            "n2".into(),
        ])
        .unwrap();
        assert_eq!(c3.f, 0); // (3-1)/3 = 0, quorum=1 — wait
        // For f=1 need n>=4. Use explicit f=1 with 3 authors would fail validate.
        // Use n=4 f=1.
        let c4 = committee4();
        let mut s4 = DagStore::new();
        s4.insert(g.clone()).unwrap();
        for (i, a) in ["n0", "n1"].iter().enumerate() {
            s4.insert(DagVertex::new(
                1,
                (*a).into(),
                vec![gid.clone()],
                vec![format!("t{}", i)],
            ))
            .unwrap();
        }
        assert!(try_commit_batches(&s4, &c4, 1, &gid).unwrap().is_none());
        for (i, a) in ["n2", "n3"].iter().enumerate() {
            s4.insert(DagVertex::new(
                1,
                (*a).into(),
                vec![gid.clone()],
                vec![format!("t{}", i + 2)],
            ))
            .unwrap();
        }
        let out4 = try_commit_batches(&s4, &c4, 1, &gid)
            .unwrap()
            .expect("quorum");
        assert_eq!(out4.digests.len(), 4);
        // Deterministic author order → digests n0,n1,n2,n3
        assert_eq!(
            out4.digests,
            vec!["t0", "t1", "t2", "t3"]
        );
    }
}
