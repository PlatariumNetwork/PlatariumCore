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
