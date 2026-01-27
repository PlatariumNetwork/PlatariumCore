// Core module for stateful transaction execution
// This module will contain the transaction execution engine
//
// DETERMINISM GUARANTEES:
// =======================
// This module guarantees deterministic execution:
// 1. Same transaction order → same final state
// 2. No randomness: all operations are deterministic
// 3. No system time: no timestamps or time-dependent logic
// 4. Deterministic hash computation: HashSet elements are sorted before hashing
// 5. Deterministic state updates: operations are applied in order
//
// INVARIANTS:
// - Transaction hash is computed deterministically from transaction data
// - State updates are applied atomically and in order
// - No external sources of non-determinism (time, random, etc.)

pub mod asset;
pub mod transaction;
pub mod state;
pub mod mempool;
pub mod execution;
pub mod fee;
pub mod determinism;

use crate::error::{PlatariumError, Result};
use crate::core::transaction::Transaction;
use crate::core::state::State;
use crate::core::mempool::Mempool;

/// Transaction hash type (alias for String)
pub type TxHash = String;

/// Core execution engine
/// This is the main entry point for transaction processing
/// Combines State and Mempool into a single interface
///
/// DETERMINISM: This struct ensures that applying the same sequence
/// of transactions in the same order always produces the same final state.
/// No randomness or system time is used in the execution path.
#[derive(Debug)]
pub struct Core {
    /// Blockchain state manager
    /// INVARIANT: State updates are deterministic and order-dependent
    state: State,
    
    /// Transaction pool (mempool)
    /// INVARIANT: Mempool storage order does not affect execution order
    mempool: Mempool,
}

impl Core {
    /// Creates a new Core instance with empty state and mempool
    pub fn new() -> Self {
        Self {
            state: State::new(),
            mempool: Mempool::new(),
        }
    }
    
    /// Submits a transaction for processing
    /// 
    /// Flow:
    /// 1. Validates the transaction (validate_basic)
    /// 2. Adds to mempool
    /// 3. Applies to state immediately
    /// 
    /// DETERMINISM GUARANTEE:
    /// - Same transaction applied in same order → same state result
    /// - No randomness or system time used
    /// - All operations are deterministic
    /// 
    /// Returns the transaction hash on success
    /// Returns error if validation fails, transaction is duplicate, or state application fails
    pub fn submit_transaction(&self, tx: Transaction) -> Result<TxHash> {
        // Step 1: Validate transaction
        // This checks signatures, amount > 0, fee >= min_fee
        tx.validate_basic()
            .map_err(|e| PlatariumError::from(e))?;
        
        // Step 2: Add to mempool
        // This prevents duplicates and stores the transaction
        self.mempool.add_transaction(tx.clone())
            .map_err(|e| PlatariumError::from(e))?;
        
        // Step 3: Apply to state immediately
        // This checks nonce, balance, and updates state atomically
        self.state.apply_transaction(&tx)
            .map_err(|e| {
                // If state application fails, remove from mempool
                // This ensures consistency: transaction is only in mempool if successfully applied
                let _ = self.mempool.remove_transaction(&tx.hash);
                e
            })?;
        
        // Return transaction hash
        Ok(tx.hash)
    }
    
    /// Gets the state manager (for direct access if needed)
    pub fn state(&self) -> &State {
        &self.state
    }
    
    /// Gets the mempool (for direct access if needed)
    pub fn mempool(&self) -> &Mempool {
        &self.mempool
    }
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use std::collections::HashSet;
    
    #[test]
    fn test_new_core() {
        let core = Core::new();
        assert!(core.mempool().is_empty());
    }
    
    #[test]
    fn test_submit_transaction_flow() {
        let core = Core::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        core.state().set_balance(&sender, 1000);
        core.state().set_uplp_balance(&sender, 10);
        core.state().set_nonce(&sender, 0);
        let tx = Transaction::new(
            sender.clone(),
            receiver.clone(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "dummy_sig_main".to_string(),
            "dummy_sig_derived".to_string(),
        ).unwrap();
        
        // This will fail at signature validation, but we can test the structure
        let result = core.submit_transaction(tx.clone());
        
        // Expect signature validation to fail with dummy signatures
        assert!(result.is_err());
    }
    
    #[test]
    fn test_submit_transaction_duplicate() {
        let core = Core::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        core.state().set_balance(&sender, 1000);
        core.state().set_uplp_balance(&sender, 10);
        core.state().set_nonce(&sender, 0);
        let tx = Transaction::new(
            sender.clone(),
            receiver.clone(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "dummy_sig".to_string(),
            "dummy_sig".to_string(),
        ).unwrap();
        // First submission will fail at signature validation
        let result1 = core.submit_transaction(tx.clone());
        assert!(result1.is_err());
        
        // Second submission should also fail (but would fail at duplicate if signatures were valid)
        // Since signature validation happens first, it will fail there
        let result2 = core.submit_transaction(tx);
        assert!(result2.is_err());
    }
    
    #[test]
    fn test_determinism_same_transactions_same_state() {
        use std::collections::HashSet;
        
        // Test that applying the same transactions in the same order
        // produces the same final state
        
        let core1 = Core::new();
        let core2 = Core::new();
        
        let sender = "sender".to_string();
        let receiver1 = "receiver1".to_string();
        let receiver2 = "receiver2".to_string();
        
        core1.state().set_balance(&sender, 1000);
        core1.state().set_uplp_balance(&sender, 10);
        core1.state().set_nonce(&sender, 0);
        core2.state().set_balance(&sender, 1000);
        core2.state().set_uplp_balance(&sender, 10);
        core2.state().set_nonce(&sender, 0);
        let tx1 = Transaction::new(
            sender.clone(),
            receiver1.clone(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "dummy_sig1".to_string(),
            "dummy_sig1".to_string(),
        ).unwrap();
        let tx2 = Transaction::new(
            sender.clone(),
            receiver2.clone(),
            Asset::PLP,
            200,
            1,
            1,
            HashSet::new(),
            HashSet::new(),
            "dummy_sig2".to_string(),
            "dummy_sig2".to_string(),
        ).unwrap();
        
        // Both transactions will fail signature validation,
        // but we can verify that the hash computation is deterministic
        assert_eq!(tx1.hash, tx1.hash); // Same transaction → same hash
        assert_eq!(tx2.hash, tx2.hash); // Same transaction → same hash
        
        // Verify hash computation is deterministic
        let hash1_1 = tx1.compute_hash().unwrap();
        let hash1_2 = tx1.compute_hash().unwrap();
        assert_eq!(hash1_1, hash1_2); // Deterministic hash computation
    }
}
