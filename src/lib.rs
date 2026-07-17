pub mod mnemonic;
pub mod key_generator;
pub mod signer;
pub mod signature;
pub mod utils;
pub mod error;
pub mod core;
pub mod storage;

pub use mnemonic::{generate_mnemonic, validate_mnemonic, CHARACTER_SET};
pub use key_generator::{KeyGenerator, KeyPair, DerivationPaths, generate_alphanumeric_part};
pub use utils::{derive_signature_seed_from_master_seed, bn_to_hex32};
pub use signer::{sign_with_both_keys, DualSignature, SignatureWithType};
pub use signature::{verify_signature, hash_message, sign_message, normalize_signature_hex, SignatureComponents};
pub use utils::verify_correlation;
pub use error::{PlatariumError, Result};

// Core API exports
pub use core::{Core, TxHash};
pub use core::asset::Asset;
pub use core::transaction::Transaction;
pub use core::state::{State, Address, StateSnapshot, SnapshotableState, TREASURY_ADDRESS};
pub use core::mempool::{Mempool, MAX_FORCED_INCLUSION_QUEUE};
pub use core::execution::{ExecutionContext, ExecutionLogic, ExecutionError, ExecutionResult};
pub use core::fee::{
    MicroPLP,
    BASE_TX_FEE_MICRO_PLP,
    MICRO_PLP_PER_PLP,
    MAX_BATCH_SIZE,
    MULTIPLIER_1X,
    MULTIPLIER_2X,
    MULTIPLIER_3X,
    MULTIPLIER_5X,
    calculate_load_multiplier,
    calculate_fee,
    calculate_fee_micro_plp,
    calculate_fee_from_load,
    calculate_fee_from_load_micro_plp,
    fee_to_plp_string,
};
pub use core::node_registry::{
    Node,
    NodeId,
    NodeRegistry,
    NodeStatus,
    NodeRegistryError,
    SCORE_SCALE,
    WEIGHT_UPTIME,
    WEIGHT_LATENCY,
    WEIGHT_VOTE_ACCURACY,
    WEIGHT_STAKE,
};
pub use core::validator_selection::{
    select_validators,
    select_validators_with_percent,
    select_validators_l2,
    select_l1_l2_validators,
    select_count,
    select_n_by_weight,
    committee_count,
    selection_percent_from_load,
    selection_percent_from_load_pct,
    selection_percent_from_load_l2,
    compute_seed,
    committee_selection_seed,
    compute_seed_l2,
    SelectionError,
    TIER_VERY_LOW_PCT,
    TIER_LOW_PCT,
    TIER_MID_PCT,
    TIER_HIGH_PCT,
    SELECT_PCT_10,
    SELECT_PCT_15,
    SELECT_PCT_20,
    SELECT_PCT_25,
    SELECT_PCT_30,
    L2_SELECT_PCT_10,
    L2_SELECT_PCT_12,
    L2_SELECT_PCT_15,
    L2_SELECT_PCT_20,
};
pub use core::confirmation_layer::{
    Vote,
    ConfirmationResult,
    L1_CONFIRM_THRESHOLD_PCT,
    verify_tx_for_l1,
    process_l1_confirmation,
    confirm_transaction_l1,
    apply_l1_penalties,
    ConfirmationError,
};
pub use core::block_assembly::{
    Block,
    block_finalized,
    block_leader_for_height,
    block_leader_index_for_height,
    BLOCK_TIME_MIN_SEC,
    BLOCK_TIME_MAX_SEC,
    L2_CONFIRM_THRESHOLD_PCT,
    compute_merkle_root,
    max_transactions_per_block,
    max_block_size_bytes,
    max_block_time_sec,
    assemble_block,
    process_l2_block_votes,
    apply_l2_block_penalties,
    BlockConfirmationResult,
    BlockAssemblyError,
};
pub use core::slashing::{
    SlashingReason,
    SUSPENSION_THRESHOLD,
    apply_slash,
    apply_slash_with_threshold,
    apply_slash_batch,
    penalty_amounts,
    SlashingError,
};
pub use core::tx_assignment::{
    required_stake_for_tx,
    required_stake_for_amount,
    min_validator_stake_for_tx,
    min_validator_stake_for_amount,
    form_verifier_groups,
    assign_transactions_to_groups,
    TxGroupAssignment,
    TxAssignmentError,
    DEFAULT_MIN_REQUIRED_STAKE,
    DEFAULT_MIN_VALIDATOR_STAKE,
};
pub use core::state_file::{
    STATE_FILE_VERSION,
    StateFileData,
    init_state_file,
    load_state_file,
    save_state_file,
    state_apply_tx_json,
    state_credit_json,
    state_query_json,
    state_root_json,
    state_validate_tx_json,
};
pub use core::consensus_cli::{
    assemble_block_json,
    l1_process_votes_json,
    l1_verify_txs_json,
    l2_process_votes_json,
};
pub use core::block_proposal_cli::{
    block_proposal_status_json,
    mempool_admit_json,
    min_fee_from_load_cli,
    select_block_txs_json,
};
pub use core::block_proposal::{
    block_proposal_status, mempool_admit, parse_mempool_snapshot, select_block_txs,
    MempoolSnapshotEntry,
};
pub use core::consensus_params::{
    BLOCK_GAS_CAP_UPLP, BLOCK_MAX_TX_COUNT, BLOCK_MAX_WAIT_SEC, BLOCK_MIN_GAS_UPLP,
    BLOCK_MIN_TX_COUNT, FAUCET_ADDRESS,
};
pub use storage::{
    AccountRecord, BlockCommit, BlockRecordStored, ReceiptRecord, RocksStore, SNAPSHOT_INTERVAL,
    SnapshotMeta, SCHEMA_VERSION, bootstrap_from_snapshot, build_commit_batch, commit_block,
    create_snapshot_if_due, get_account, get_block, get_head, get_receipt, get_state_root, get_tx,
    list_snapshots, list_tx_hashes_for_address, migrate_json_to_rocks, open_store,
    rocks_bootstrap_snapshot_json, rocks_commit_block_json, rocks_get_account_json,
    rocks_get_block_json, rocks_get_head_json, rocks_get_receipt_json, rocks_get_snapshot_json,
    rocks_get_state_root_json, rocks_get_tx_json, rocks_list_address_txs_json,
    rocks_list_snapshots_json,
};