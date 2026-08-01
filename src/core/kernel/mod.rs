//! Execution-first kernel: OrderedBatch → Scheduler → StateDiff → Commit.
//!
//! Layering: this module MUST NOT import `crate::storage::rocks` or open RocksDB.
//! Durable writes go through [`commit_engine`] + [`crate::storage::engine::StorageEngine`].

pub mod ordered_batch;
pub mod state_diff;
pub mod touch;
pub mod scheduler;
pub mod execute;
pub mod commit_engine;
pub mod ordering;

pub use ordered_batch::OrderedBatch;
pub use state_diff::{AccountPostImage, StateDiff, TxReceipt, STATE_DIFF_SCHEMA_VERSION};
pub use touch::{conflict_touch_set, touch_set};
pub use scheduler::{compute_waves, ExecutionWave};
pub use execute::{clone_state, execute_ordered_batch, ExecuteOptions, ExecuteOutcome};
pub use commit_engine::{commit_state_diff, CommitResult};
pub use ordering::build_ordered_batch;
