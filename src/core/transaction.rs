//! Canonical transaction format for the Platarium Network.
//!
//! **Multi-asset:** Amount is in minimal units of the chosen `asset` (PLP or token); the asset does not affect fee. Fee is always in μPLP (1 PLP = 1_000_000 μPLP). Transactions with non-μPLP fee or zero fee are rejected.
//!
//! **Determinism:** Hash is computed deterministically (e.g. set elements sorted before hashing); no randomness or system time. Same transaction data yields the same hash.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use crate::error::Result;
use crate::core::asset::Asset;
use crate::signature::{hash_message, verify_signature};
use thiserror::Error;

/// Minimum transaction fee in μPLP. Fee currency is fixed to μPLP and is not configurable.
pub const MIN_FEE_UPLP: u128 = 1;

/// Errors produced by transaction validation.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum TransactionValidationError {
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Invalid amount: amount must be greater than 0")]
    InvalidAmount,

    #[error("Invalid fee: fee must be in μPLP and at least {0}, got {1}")]
    InvalidFee(u128, u128),

    #[error("Hash mismatch: expected {0}, got {1}")]
    HashMismatch(String, String),
}

/// Result type for transaction validation.
pub type ValidationResult = std::result::Result<(), TransactionValidationError>;

/// Canonical transaction structure (single source of truth for the network format).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    /// Transaction hash (computed from transaction data)
    pub hash: String,

    /// Sender address
    pub from: String,

    /// Receiver address
    pub to: String,

    /// Asset being transferred (PLP or Token). Does not affect fee.
    pub asset: Asset,

    /// Amount to transfer, in minimal units of `asset`.
    pub amount: u128,

    /// Fee, always in μPLP. Fee currency is fixed to μPLP and is not configurable.
    pub fee_uplp: u128,

    /// Nonce to prevent replay attacks
    pub nonce: u64,

    /// Set of addresses that this transaction reads from
    pub reads: HashSet<String>,

    /// Set of addresses that this transaction writes to
    pub writes: HashSet<String>,

    /// Main signature (from main private key)
    pub sig_main: String,

    /// Derived signature (from HKDF-derived key)
    pub sig_derived: String,
}

impl Transaction {
    /// Constructs a new transaction and computes its hash.
    pub fn new(
        from: String,
        to: String,
        asset: Asset,
        amount: u128,
        fee_uplp: u128,
        nonce: u64,
        reads: HashSet<String>,
        writes: HashSet<String>,
        sig_main: String,
        sig_derived: String,
    ) -> Result<Self> {
        let mut tx = Self {
            hash: String::new(),
            from,
            to,
            asset,
            amount,
            fee_uplp,
            nonce,
            reads,
            writes,
            sig_main,
            sig_derived,
        };
        tx.hash = tx.compute_hash()?;
        Ok(tx)
    }

    fn hash_data(&self) -> (Vec<String>, Vec<String>) {
        let mut reads_vec: Vec<String> = self.reads.iter().cloned().collect();
        reads_vec.sort();
        let mut writes_vec: Vec<String> = self.writes.iter().cloned().collect();
        writes_vec.sort();
        (reads_vec, writes_vec)
    }
    
    /// Computes the transaction hash. Same transaction data -> same hash. No randomness or system time.
    pub fn compute_hash(&self) -> Result<String> {
        #[derive(Serialize)]
        struct TransactionHashData {
            from: String,
            to: String,
            asset: String,
            amount: u128,
            fee_uplp: u128,
            nonce: u64,
            reads: Vec<String>,
            writes: Vec<String>,
        }
        let (reads_vec, writes_vec) = self.hash_data();
        let hash_data = TransactionHashData {
            from: self.from.clone(),
            to: self.to.clone(),
            asset: self.asset.as_canonical(),
            amount: self.amount,
            fee_uplp: self.fee_uplp,
            nonce: self.nonce,
            reads: reads_vec,
            writes: writes_vec,
        };
        let hash_bytes = hash_message(&hash_data)?;
        Ok(hex::encode(hash_bytes))
    }
    
    /// Verifies both signatures (main and derived)
    pub fn verify_signatures(&self) -> Result<bool> {
        #[derive(Serialize)]
        struct TransactionHashData {
            from: String,
            to: String,
            asset: String,
            amount: u128,
            fee_uplp: u128,
            nonce: u64,
            reads: Vec<String>,
            writes: Vec<String>,
        }
        let (reads_vec, writes_vec) = self.hash_data();
        let message = TransactionHashData {
            from: self.from.clone(),
            to: self.to.clone(),
            asset: self.asset.as_canonical(),
            amount: self.amount,
            fee_uplp: self.fee_uplp,
            nonce: self.nonce,
            reads: reads_vec,
            writes: writes_vec,
        };
        let main_verified = verify_signature(&message, &self.sig_main, &self.from)?;
        if !main_verified {
            return Ok(false);
        }
        let derived_verified = verify_signature(&message, &self.sig_derived, &self.from)?;
        Ok(main_verified && derived_verified)
    }
    
    /// Validates the transaction hash matches computed hash
    pub fn validate_hash(&self) -> Result<bool> {
        Ok(self.hash == self.compute_hash()?)
    }

    /// Validates basic transaction properties (no state access).
    /// Amount > 0; fee in μPLP, fee >= MIN_FEE_UPLP (fee = 0 forbidden); signatures.
    /// Fee currency is fixed to μPLP and is not configurable.
    pub fn validate_basic(&self) -> ValidationResult {
        if self.amount == 0 {
            return Err(TransactionValidationError::InvalidAmount);
        }
        if self.fee_uplp < MIN_FEE_UPLP {
            return Err(TransactionValidationError::InvalidFee(
                MIN_FEE_UPLP,
                self.fee_uplp,
            ));
        }
        match self.verify_signatures() {
            Ok(true) => {}
            Ok(false) => {
                return Err(TransactionValidationError::InvalidSignature(
                    "One or both signatures are invalid".to_string(),
                ));
            }
            Err(e) => {
                return Err(TransactionValidationError::InvalidSignature(
                    format!("Signature verification error: {}", e),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;

    #[test]
    fn test_transaction_creation() {
        let reads = HashSet::from(["addr1".to_string(), "addr2".to_string()]);
        let writes = HashSet::from(["addr3".to_string()]);
        let tx = Transaction::new(
            "sender_addr".to_string(),
            "receiver_addr".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            reads,
            writes,
            "sig_main".to_string(),
            "sig_derived".to_string(),
        );
        assert!(tx.is_ok());
        let tx = tx.unwrap();
        assert!(!tx.hash.is_empty());
    }

    #[test]
    fn test_hash_computation() {
        let tx = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig1".to_string(),
            "sig2".to_string(),
        )
        .unwrap();
        let hash1 = tx.compute_hash().unwrap();
        let hash2 = tx.compute_hash().unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(tx.hash, hash1);
    }

    #[test]
    fn test_hash_determinism() {
        let reads1 = HashSet::from(["a".to_string(), "b".to_string()]);
        let reads2 = HashSet::from(["b".to_string(), "a".to_string()]);
        let tx1 = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            reads1,
            HashSet::new(),
            "sig1".to_string(),
            "sig2".to_string(),
        )
        .unwrap();
        let tx2 = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            reads2,
            HashSet::new(),
            "sig1".to_string(),
            "sig2".to_string(),
        )
        .unwrap();
        assert_eq!(tx1.hash, tx2.hash);
    }

    #[test]
    fn test_validate_basic_valid() {
        let tx = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main".to_string(),
            "sig_derived".to_string(),
        )
        .unwrap();
        let result = tx.validate_basic();
        assert!(result.is_err());
        if let Err(TransactionValidationError::InvalidSignature(_)) = result {
        } else {
            panic!("Expected InvalidSignature error");
        }
    }

    #[test]
    fn test_validate_basic_invalid_amount_zero() {
        let tx = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            0,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main".to_string(),
            "sig_derived".to_string(),
        )
        .unwrap();
        let result = tx.validate_basic();
        assert!(result.is_err());
        if let Err(TransactionValidationError::InvalidAmount) = result {
        } else {
            panic!("Expected InvalidAmount error, got: {:?}", result);
        }
    }

    #[test]
    fn test_validate_basic_invalid_fee_too_low() {
        let tx = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            100,
            0,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main".to_string(),
            "sig_derived".to_string(),
        )
        .unwrap();
        let result = tx.validate_basic();
        assert!(result.is_err());
        if let Err(TransactionValidationError::InvalidFee(_, _)) = result {
        } else {
            panic!("Expected InvalidFee error, got: {:?}", result);
        }
    }

    #[test]
    fn test_validate_basic_valid_fee_at_minimum() {
        let tx = Transaction::new(
            "sender".to_string(),
            "receiver".to_string(),
            Asset::PLP,
            100,
            MIN_FEE_UPLP,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main".to_string(),
            "sig_derived".to_string(),
        )
        .unwrap();
        let result = tx.validate_basic();
        assert!(result.is_err());
        if let Err(TransactionValidationError::InvalidSignature(_)) = result {
        } else {
            panic!("Expected InvalidSignature (fee validation should have passed)");
        }
    }

    #[test]
    fn test_transaction_with_token_asset() {
        let tx = Transaction::new(
            "from".to_string(),
            "to".to_string(),
            Asset::Token("USDT".to_string()),
            1_000_000,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "s1".to_string(),
            "s2".to_string(),
        )
        .unwrap();
        assert_eq!(tx.asset.as_canonical(), "Token:USDT");
        assert_eq!(tx.amount, 1_000_000);
        assert_eq!(tx.fee_uplp, 1);
    }
}
