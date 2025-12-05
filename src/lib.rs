pub mod mnemonic;
pub mod key_generator;
pub mod signer;
pub mod signature;
pub mod utils;
pub mod error;

pub use mnemonic::{generate_mnemonic, validate_mnemonic, CHARACTER_SET};
pub use key_generator::{KeyGenerator, KeyPair, DerivationPaths, generate_alphanumeric_part};
pub use utils::{derive_signature_seed_from_master_seed, bn_to_hex32};
pub use signer::{sign_with_both_keys, DualSignature, SignatureWithType};
pub use signature::{verify_signature, hash_message, sign_message, SignatureComponents};
pub use utils::verify_correlation;
pub use error::{PlatariumError, Result};

