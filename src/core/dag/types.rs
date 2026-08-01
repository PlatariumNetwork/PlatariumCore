//! DAG vertex types and committee config.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub type AuthorId = String;
pub type VertexId = String;

/// One Narwhal-style DAG vertex (unsigned in v0).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DagVertex {
    pub id: VertexId,
    pub round: u64,
    pub author: AuthorId,
    pub parents: Vec<VertexId>,
    pub tx_digests: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VertexBody<'a> {
    round: u64,
    author: &'a str,
    parents: Vec<&'a str>,
    tx_digests: Vec<&'a str>,
}

impl DagVertex {
    /// Content-hash id for a vertex body (parents sorted).
    pub fn compute_id(
        round: u64,
        author: &str,
        parents: &[VertexId],
        tx_digests: &[String],
    ) -> VertexId {
        let mut parents_sorted: Vec<&str> = parents.iter().map(|s| s.as_str()).collect();
        parents_sorted.sort();
        parents_sorted.dedup();
        let body_digests: Vec<&str> = tx_digests.iter().map(|s| s.as_str()).collect();
        let body = VertexBody {
            round,
            author,
            parents: parents_sorted,
            tx_digests: body_digests,
        };
        let json = serde_json::to_vec(&body).expect("vertex body json");
        hex::encode(Sha256::digest(&json))
    }

    /// Build vertex and assign content-hash id (parents sorted for hashing).
    pub fn new(
        round: u64,
        author: AuthorId,
        mut parents: Vec<VertexId>,
        tx_digests: Vec<String>,
    ) -> Self {
        parents.sort();
        parents.dedup();
        let id = Self::compute_id(round, &author, &parents, &tx_digests);
        Self {
            id,
            round,
            author,
            parents,
            tx_digests,
        }
    }

    /// Accept a peer wire vertex only if `id` matches content hash.
    pub fn from_wire(
        id: VertexId,
        round: u64,
        author: AuthorId,
        mut parents: Vec<VertexId>,
        tx_digests: Vec<String>,
    ) -> Result<Self, String> {
        parents.sort();
        parents.dedup();
        let expected = Self::compute_id(round, &author, &parents, &tx_digests);
        if id != expected {
            return Err(format!(
                "dag: wire id mismatch: got {} expected {}",
                id, expected
            ));
        }
        Ok(Self {
            id,
            round,
            author,
            parents,
            tx_digests,
        })
    }

    /// Genesis vertex: round 0, empty parents/payloads.
    pub fn genesis(author: AuthorId) -> Self {
        Self::new(0, author, Vec::new(), Vec::new())
    }
}

/// Static committee for Bullshark-lite commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitteeConfig {
    pub authors: Vec<AuthorId>,
    pub f: usize,
}

impl CommitteeConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.authors.is_empty() {
            return Err("committee empty".into());
        }
        let unique: BTreeSet<_> = self.authors.iter().collect();
        if unique.len() != self.authors.len() {
            return Err("committee authors must be unique".into());
        }
        let n = self.authors.len();
        let min = 3 * self.f + 1;
        if n < min {
            return Err(format!("need n >= 3f+1 (n={}, f={}, min={})", n, self.f, min));
        }
        Ok(())
    }

    pub fn quorum(&self) -> usize {
        2 * self.f + 1
    }

    /// Build committee from author list; `f = (n-1)/3` (0 when n=1).
    pub fn from_authors(mut authors: Vec<AuthorId>) -> Result<Self, String> {
        authors.sort();
        authors.dedup();
        let n = authors.len();
        let f = if n == 0 { 0 } else { (n - 1) / 3 };
        let c = Self { authors, f };
        c.validate()?;
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_stable() {
        let a = DagVertex::new(1, "n0".into(), vec!["p".into()], vec!["t1".into()]);
        let b = DagVertex::new(1, "n0".into(), vec!["p".into()], vec!["t1".into()]);
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn committee_rejects_small_n() {
        let c = CommitteeConfig {
            authors: vec!["a".into(), "b".into()],
            f: 1,
        };
        assert!(c.validate().is_err());
    }
}
