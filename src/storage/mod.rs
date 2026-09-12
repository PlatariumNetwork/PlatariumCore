//! Canonical RocksDB storage for accounts, transactions, blocks, receipts, and indexes.
//!
//! Mempool is never persisted here — RAM only.

pub mod schema;
pub mod migrations;
pub mod rocks;
pub mod cache;
pub mod engine;
pub mod commit;
pub mod snapshot;
pub mod query;
pub mod rpc;

pub use cache::{evict_cached, open_cached};
pub use engine::{
    account_record_from_post_image, InMemoryStorageEngine, RocksAccountStorageEngine,
    StateFileStorageEngine, StorageEngine,
};

pub use commit::{
    AccountRecord, BlockCommit, BlockRecordStored, ReceiptRecord,
    account_record_to_query_json, account_rmw_preserve_tokens_xp,
    assert_commit_allowed_after_execution, build_commit_batch, commit_block,
};
pub use query::{
    get_account, get_block, get_head, get_receipt, get_state_root, get_tx, list_accounts,
    list_tx_hashes_for_address,
};
pub use rocks::{RocksStore, open_store};
pub use schema::SCHEMA_VERSION;
pub use migrations::{
    ensure_schema, migrate, read_schema_version, write_schema_version,
};
pub use snapshot::{
    SNAPSHOT_INTERVAL, SnapshotMeta, bootstrap_from_snapshot, create_snapshot_if_due, list_snapshots,
};
pub use rpc::{
    migrate_json_to_rocks, rocks_bootstrap_snapshot_json, rocks_commit_block_json,
    rocks_get_account_json, rocks_get_block_json, rocks_get_head_json, rocks_get_receipt_json,
    rocks_get_snapshot_json, rocks_get_state_root_json, rocks_get_tx_json,
    rocks_list_address_txs_json, rocks_list_snapshots_json,
};
