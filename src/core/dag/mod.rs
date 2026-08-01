//! DAG availability + ordering (Narwhal/Bullshark-lite v0).
//!
//! Layering: this module MUST NOT import `crate::storage::rocks` or
//! `crate::core::execution`. It may bridge to `kernel::OrderedBatch` only.

pub mod types;
pub mod store;
pub mod linearize;
pub mod commit;
pub mod bridge;
pub mod pipeline;
pub mod ingest;
pub mod genesis;
pub mod last_commit;

pub use types::{AuthorId, CommitteeConfig, DagVertex, VertexId};
pub use store::{global_dag_store, global_pending_queue, reset_global_dag_store, DagStore};
pub use linearize::{linearize, LinearizeResult};
pub use commit::{leader_for_round, try_commit, try_commit_batches, CommitOutcome};
pub use bridge::dag_to_ordered_batch;
pub use pipeline::{order_digests, OrderDigestsResult};
pub use ingest::{ingest, vertex_from_params, IngestResult, IngestStatus, PendingQueue, PENDING_MAX};
pub use genesis::{shared_genesis, SHARED_GENESIS_AUTHOR};
pub use last_commit::{clear_last_commit, get_last_commit, set_last_commit};
