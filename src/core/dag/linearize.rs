//! Deterministic causal linearization of a DAG anchor.

use crate::core::dag::store::DagStore;
use crate::core::dag::types::VertexId;
use crate::error::{PlatariumError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinearizeResult {
    pub vertex_order: Vec<VertexId>,
    pub digests: Vec<String>,
}

/// Linearize causal history of `anchor` (ancestors + self).
/// Tie-break among ready vertices: (round, author, vertex_id).
pub fn linearize(store: &DagStore, anchor: &str) -> Result<LinearizeResult> {
    let anchor_v = store.get(anchor).ok_or_else(|| {
        PlatariumError::State(format!("dag: missing anchor {}", anchor))
    })?;

    // Collect ancestor set (including anchor).
    let mut needed: HashSet<VertexId> = HashSet::new();
    let mut stack = vec![anchor.to_string()];
    while let Some(id) = stack.pop() {
        if !needed.insert(id.clone()) {
            continue;
        }
        if let Some(v) = store.get(&id) {
            for p in &v.parents {
                stack.push(p.clone());
            }
        }
    }

    // Kahn-style over subgraph: count parents inside `needed`.
    let mut indeg: std::collections::HashMap<VertexId, usize> = std::collections::HashMap::new();
    let mut children: std::collections::HashMap<VertexId, Vec<VertexId>> =
        std::collections::HashMap::new();
    for id in &needed {
        let v = store.get(id).unwrap();
        let parents_in: Vec<_> = v
            .parents
            .iter()
            .filter(|p| needed.contains(*p))
            .cloned()
            .collect();
        indeg.insert(id.clone(), parents_in.len());
        for p in parents_in {
            children.entry(p).or_default().push(id.clone());
        }
    }

    let mut ready: BTreeSet<(u64, String, String)> = BTreeSet::new();
    for id in &needed {
        if indeg[id] == 0 {
            let v = store.get(id).unwrap();
            ready.insert((v.round, v.author.clone(), id.clone()));
        }
    }

    let mut vertex_order = Vec::with_capacity(needed.len());
    while let Some(key) = ready.iter().next().cloned() {
        ready.remove(&key);
        let id = key.2;
        vertex_order.push(id.clone());
        if let Some(chs) = children.get(&id) {
            for c in chs {
                let e = indeg.get_mut(c).unwrap();
                *e -= 1;
                if *e == 0 {
                    let v = store.get(c).unwrap();
                    ready.insert((v.round, v.author.clone(), c.clone()));
                }
            }
        }
    }

    if vertex_order.len() != needed.len() {
        return Err(PlatariumError::State(
            "dag: cycle detected in causal history".into(),
        ));
    }

    if !vertex_order.iter().any(|id| id == anchor) {
        return Err(PlatariumError::State("dag: anchor missing from order".into()));
    }
    let _ = anchor_v;

    let mut digests = Vec::new();
    for id in &vertex_order {
        let v = store.get(id).unwrap();
        digests.extend(v.tx_digests.iter().cloned());
    }

    Ok(LinearizeResult {
        vertex_order,
        digests,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dag::types::DagVertex;

    fn diamond() -> (DagStore, String) {
        let mut store = DagStore::new();
        let g = DagVertex::genesis("n0".into());
        let gid = store.insert(g).unwrap();
        let a = DagVertex::new(1, "n1".into(), vec![gid.clone()], vec!["txA".into()]);
        let b = DagVertex::new(1, "n2".into(), vec![gid.clone()], vec!["txB".into()]);
        let aid = store.insert(a).unwrap();
        let bid = store.insert(b).unwrap();
        // Tie-break: author n1 before n2 → txA before txB when same round under genesis.
        let tip = DagVertex::new(
            2,
            "n0".into(),
            vec![aid, bid],
            vec!["txTip".into()],
        );
        let tip_id = store.insert(tip).unwrap();
        (store, tip_id)
    }

    #[test]
    fn deterministic_across_stores() {
        let (s1, a1) = diamond();
        let (s2, a2) = diamond();
        assert_eq!(a1, a2);
        let r1 = linearize(&s1, &a1).unwrap();
        let r2 = linearize(&s2, &a2).unwrap();
        assert_eq!(r1.digests, r2.digests);
        assert_eq!(r1.vertex_order, r2.vertex_order);
        assert_eq!(r1.digests, vec!["txA", "txB", "txTip"]);
    }

    #[test]
    fn missing_anchor_errors() {
        let store = DagStore::new();
        assert!(linearize(&store, "nope").is_err());
    }

    #[test]
    fn insert_order_independent_for_siblings() {
        let mut s_ab = DagStore::new();
        let mut s_ba = DagStore::new();
        let g = DagVertex::genesis("n0".into());
        let g2 = g.clone();
        let gid1 = s_ab.insert(g).unwrap();
        let gid2 = s_ba.insert(g2).unwrap();
        assert_eq!(gid1, gid2);

        let a = DagVertex::new(1, "n1".into(), vec![gid1.clone()], vec!["txA".into()]);
        let b = DagVertex::new(1, "n2".into(), vec![gid1.clone()], vec!["txB".into()]);
        let a2 = a.clone();
        let b2 = b.clone();

        let aid = s_ab.insert(a).unwrap();
        let bid = s_ab.insert(b).unwrap();
        let tip1 = s_ab
            .insert(DagVertex::new(
                2,
                "n0".into(),
                vec![aid, bid],
                vec!["txTip".into()],
            ))
            .unwrap();

        let bid2 = s_ba.insert(b2).unwrap();
        let aid2 = s_ba.insert(a2).unwrap();
        let tip2 = s_ba
            .insert(DagVertex::new(
                2,
                "n0".into(),
                vec![aid2, bid2],
                vec!["txTip".into()],
            ))
            .unwrap();

        // Same tip content → same id; linearize identical.
        assert_eq!(tip1, tip2);
        assert_eq!(
            linearize(&s_ab, &tip1).unwrap().digests,
            linearize(&s_ba, &tip2).unwrap().digests
        );
    }
}
