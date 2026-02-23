pub mod mnemonic;
pub mod key_generator;
pub mod signer;
pub mod signature;
pub mod utils;
pub mod error;
pub mod core;

pub use mnemonic::{generate_mnemonic, validate_mnemonic, CHARACTER_SET};
pub use key_generator::{KeyGenerator, KeyPair, DerivationPaths, generate_alphanumeric_part};
pub use utils::{derive_signature_seed_from_master_seed, bn_to_hex32};
pub use signer::{sign_with_both_keys, DualSignature, SignatureWithType};
pub use signature::{verify_signature, hash_message, sign_message, SignatureComponents};
pub use utils::verify_correlation;
pub use error::{PlatariumError, Result};

// Core API exports
pub use core::{Core, TxHash};
pub use core::asset::Asset;
pub use core::transaction::Transaction;
pub use core::state::{State, Address, StateSnapshot, SnapshotableState, TREASURY_ADDRESS};
pub use core::mempool::Mempool;
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
    select_validators_l2,
    select_count,
    selection_percent_from_load,
    selection_percent_from_load_l2,
    compute_seed,
    compute_seed_l2,
    SelectionError,
    TIER_LOW_PCT,
    TIER_MID_PCT,
    TIER_HIGH_PCT,
    SELECT_PCT_10,
    SELECT_PCT_15,
    SELECT_PCT_20,
    SELECT_PCT_25,
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
