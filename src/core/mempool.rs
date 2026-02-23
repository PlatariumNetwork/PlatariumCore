//! Mempool (transaction pool): pending transaction storage and ordering before execution.
//!
//! # Forced inclusion (anti-censorship)
//! Transaction hashes can be enqueued for forced inclusion so they are prioritized when
//! building the next block. Use `add_forced_inclusion` and `get_transaction_hashes_for_block(max_count)` in block assembly.
//!
//! # Fairness and determinism
//!
//! **Hash-only ordering and starvation:** Ordering solely by `tx.hash` can indefinitely delay
//! transactions whose hashes sort later (e.g. lexicographically) when the mempool is continuously refilled with earlier-sorting hashes.
//!
//! **No system time:** System time differs across nodes and runs. Using it for ordering would
//! break consensus determinism (same transaction set could yield different execution order on different nodes).
//!
//! **`arrival_index`:** A monotonic, node-local counter incremented on each successful `add_transaction()`.
//! It is not derived from system time and is not part of tx hash, signatures, state, or consensus.
//! It is used only inside the mempool for arrival-order fairness. Same mempool contents yield the same
//! `(arrival_index, tx.hash)` ordering and thus the same execution order.
//!
//! **Separation of concerns:** The mempool handles fairness and liveness (arrival-order scheduling);
//! the execution layer receives only the sorted batch of transactions and never `arrival_index`.
//!
//! # Invariants
//! - Storage is keyed by transaction hash (deterministic lookup).
//! - Execution order is derived from the sorted batch produced by this module.
//! - `arrival_index` is never exposed outside this module.

use std::collections::HashMap;
use std::sync::RwLock;
use crate::error::{PlatariumError, Result};
use crate::core::transaction::Transaction;
use thiserror::Error;

/// Internal mempool entry: transaction and its logical arrival order.
///
/// `arrival_index` is monotonic and node-local; it is not derived from system time and is not used in hashes, signatures, state, or consensus. It is used only for fair ordering within the mempool.
#[derive(Debug, Clone)]
pub struct MempoolEntry {
    pub tx: Transaction,
    pub arrival_index: u64,
}

/// Errors produced by the mempool.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum MempoolError {
    #[error("Duplicate transaction: transaction with hash {0} already exists")]
    DuplicateTransaction(String),
    
    #[error("Mempool error: {0}")]
    Other(String),
}

impl From<MempoolError> for PlatariumError {
    fn from(err: MempoolError) -> Self {
        PlatariumError::State(format!("Mempool error: {}", err))
    }
}

/// Maximum number of transaction hashes in the forced-inclusion queue.
const FORCED_INCLUSION_CAP: usize = 256;

/// Thread-safe transaction pool (mempool) for pending transactions before execution.
#[derive(Debug)]
pub struct Mempool {
    /// Pending transactions keyed by hash; each entry includes the transaction and its arrival index.
    transactions: RwLock<HashMap<String, MempoolEntry>>,
    /// Monotonic counter for logical arrival order; incremented on each successful add. Not exposed outside this module.
    next_arrival_index: RwLock<u64>,
    /// Forced-inclusion queue (anti-censorship): these hashes are prioritized when building the next block.
    forced_inclusion: RwLock<Vec<String>>,
}

impl Mempool {
    /// Creates a new empty mempool.
    pub fn new() -> Self {
        Self {
            transactions: RwLock::new(HashMap::new()),
            next_arrival_index: RwLock::new(0),
            forced_inclusion: RwLock::new(Vec::new()),
        }
    }

    /// Adds a transaction to the mempool. Errors if a transaction with the same hash already exists.
    ///
    /// Assigns a monotonic `arrival_index` (node-local, not from system time), stores the transaction, and does not validate or execute it.
    pub fn add_transaction(&self, tx: Transaction) -> Result<()> {
        let mut transactions = self.transactions.write().unwrap();
        let mut next = self.next_arrival_index.write().unwrap();

        if transactions.contains_key(&tx.hash) {
            return Err(MempoolError::DuplicateTransaction(tx.hash.clone()).into());
        }

        let idx = *next;
        *next = next.saturating_add(1);
        transactions.insert(
            tx.hash.clone(),
            MempoolEntry { tx, arrival_index: idx },
        );

        Ok(())
    }

    /// Returns the transaction for the given hash, if present. The execution layer receives only the transaction; `arrival_index` is not exposed.
    pub fn get_transaction(&self, hash: &str) -> Option<Transaction> {
        let transactions = self.transactions.read().unwrap();
        transactions.get(hash).map(|e| e.tx.clone())
    }

    /// Removes a transaction from the mempool by hash. Typically called after the transaction has been executed.
    pub fn remove_transaction(&self, hash: &str) -> bool {
        let mut transactions = self.transactions.write().unwrap();
        transactions.remove(hash).is_some()
    }

    /// Removes the given transactions from the mempool. Typically called after they have been executed in a block.
    pub fn remove_transactions(&self, hashes: &[String]) {
        let mut transactions = self.transactions.write().unwrap();
        for hash in hashes {
            transactions.remove(hash);
        }
    }

    /// Returns all pending transactions in a fair, deterministic order: sorted by (arrival_index, tx.hash). Same mempool contents yield the same order; the execution layer receives only the transaction list.
    pub fn get_all_transactions(&self) -> Vec<Transaction> {
        let transactions = self.transactions.read().unwrap();
        let mut entries: Vec<MempoolEntry> = transactions.values().cloned().collect();
        entries.sort_by(|a, b| {
            (a.arrival_index, a.tx.hash.as_str()).cmp(&(b.arrival_index, b.tx.hash.as_str()))
        });
        entries.into_iter().map(|e| e.tx).collect()
    }
    
    /// Returns the number of pending transactions.
    pub fn len(&self) -> usize {
        let transactions = self.transactions.read().unwrap();
        transactions.len()
    }
    
    /// Returns whether the mempool is empty.
    pub fn is_empty(&self) -> bool {
        let transactions = self.transactions.read().unwrap();
        transactions.is_empty()
    }
    
    /// Returns whether a transaction with the given hash is in the mempool.
    pub fn contains(&self, hash: &str) -> bool {
        let transactions = self.transactions.read().unwrap();
        transactions.contains_key(hash)
    }
    
    /// Removes all transactions from the mempool.
    pub fn clear(&self) {
        let mut transactions = self.transactions.write().unwrap();
        transactions.clear();
    }

    /// Adds a transaction hash to the forced-inclusion queue (anti-censorship). No effect if the queue is at capacity or the hash is already enqueued.
    pub fn add_forced_inclusion(&self, tx_hash: String) {
        let mut q = self.forced_inclusion.write().unwrap();
        if q.len() < FORCED_INCLUSION_CAP && !q.contains(&tx_hash) {
            q.push(tx_hash);
        }
    }

    /// Returns a copy of the forced-inclusion queue (order preserved).
    pub fn get_forced_inclusion(&self) -> Vec<String> {
        self.forced_inclusion.read().unwrap().clone()
    }

    /// Removes the given hashes from the forced-inclusion queue (e.g. after they were included in a block).
    pub fn remove_forced_inclusion(&self, hashes: &[String]) {
        let mut q = self.forced_inclusion.write().unwrap();
        let set: std::collections::HashSet<_> = hashes.iter().collect();
        q.retain(|h| !set.contains(h));
    }

    /// Returns transaction hashes for block assembly: forced-inclusion entries that are still in the mempool first, then others up to `max_count`. Deterministic for the same mempool and forced-inclusion state.
    pub fn get_transaction_hashes_for_block(&self, max_count: usize) -> Vec<String> {
        use std::collections::HashSet;
        let forced = self.get_forced_inclusion();
        let all_txs = self.get_all_transactions();
        let in_mempool: HashSet<_> = all_txs.iter().map(|t| t.hash.as_str()).collect();
        let mut result: Vec<String> = forced.into_iter().filter(|h| in_mempool.contains(h.as_str())).collect();
        let mut used: HashSet<String> = result.iter().cloned().collect();
        for tx in all_txs {
            if result.len() >= max_count {
                break;
            }
            let hash = tx.hash.clone();
            if used.insert(hash.clone()) {
                result.push(hash);
            }
        }
        result
    }
}

impl Default for Mempool {
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
    fn test_new_mempool() {
        let mempool = Mempool::new();
        assert!(mempool.is_empty());
        assert_eq!(mempool.len(), 0);
    }
    
    #[test]
    fn test_add_transaction() {
        let mempool = Mempool::new();
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
        ).unwrap();
        let result = mempool.add_transaction(tx.clone());
        assert!(result.is_ok());
        
        assert_eq!(mempool.len(), 1);
        assert!(!mempool.is_empty());
        assert!(mempool.contains(&tx.hash));
    }
    
    #[test]
    fn test_add_transaction_duplicate() {
        let mempool = Mempool::new();
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
        ).unwrap();
        // Add first time
        let result1 = mempool.add_transaction(tx.clone());
        assert!(result1.is_ok());
        
        // Try to add duplicate
        let result2 = mempool.add_transaction(tx);
        assert!(result2.is_err());
        
        if let Err(PlatariumError::State(msg)) = result2 {
            assert!(msg.contains("Duplicate transaction"));
        } else {
            panic!("Expected MempoolError::DuplicateTransaction");
        }
        
        // Should still have only one transaction
        assert_eq!(mempool.len(), 1);
    }
    
    #[test]
    fn test_get_transaction() {
        let mempool = Mempool::new();
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
        ).unwrap();
        mempool.add_transaction(tx.clone()).unwrap();
        
        // Get existing transaction
        let retrieved = mempool.get_transaction(&tx.hash);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().hash, tx.hash);
        
        // Get non-existent transaction
        let not_found = mempool.get_transaction("nonexistent_hash");
        assert!(not_found.is_none());
    }
    
    #[test]
    fn test_remove_transaction() {
        let mempool = Mempool::new();
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
        ).unwrap();
        mempool.add_transaction(tx.clone()).unwrap();
        assert_eq!(mempool.len(), 1);
        // Remove transaction
        let removed = mempool.remove_transaction(&tx.hash);
        assert!(removed);
        assert_eq!(mempool.len(), 0);
        assert!(!mempool.contains(&tx.hash));
        
        // Try to remove again (should return false)
        let removed_again = mempool.remove_transaction(&tx.hash);
        assert!(!removed_again);
    }
    
    #[test]
    fn test_remove_transactions() {
        let mempool = Mempool::new();
        
        let tx1 = Transaction::new(
            "sender1".to_string(),
            "receiver1".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main1".to_string(),
            "sig_derived1".to_string(),
        ).unwrap();
        let tx2 = Transaction::new(
            "sender2".to_string(),
            "receiver2".to_string(),
            Asset::PLP,
            200,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main2".to_string(),
            "sig_derived2".to_string(),
        ).unwrap();
        mempool.add_transaction(tx1.clone()).unwrap();
        mempool.add_transaction(tx2.clone()).unwrap();
        assert_eq!(mempool.len(), 2);
        mempool.remove_transactions(&[tx1.hash.clone(), tx2.hash.clone()]);
        assert_eq!(mempool.len(), 0);
    }
    
    #[test]
    fn test_get_all_transactions() {
        let mempool = Mempool::new();
        
        let tx1 = Transaction::new(
            "sender1".to_string(),
            "receiver1".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main1".to_string(),
            "sig_derived1".to_string(),
        ).unwrap();
        let tx2 = Transaction::new(
            "sender2".to_string(),
            "receiver2".to_string(),
            Asset::PLP,
            200,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_main2".to_string(),
            "sig_derived2".to_string(),
        ).unwrap();
        mempool.add_transaction(tx1.clone()).unwrap();
        mempool.add_transaction(tx2.clone()).unwrap();
        let all_txs = mempool.get_all_transactions();
        assert_eq!(all_txs.len(), 2);
        
        // Verify both transactions are present
        let hashes: Vec<String> = all_txs.iter().map(|tx| tx.hash.clone()).collect();
        assert!(hashes.contains(&tx1.hash));
        assert!(hashes.contains(&tx2.hash));
    }
    
    #[test]
    fn test_clear() {
        let mempool = Mempool::new();
        
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
        ).unwrap();
        mempool.add_transaction(tx.clone()).unwrap();
        assert_eq!(mempool.len(), 1);
        mempool.clear();
        assert_eq!(mempool.len(), 0);
        assert!(mempool.is_empty());
        assert!(!mempool.contains(&tx.hash));
    }
    
    #[test]
    fn test_multiple_transactions_different_hashes() {
        let mempool = Mempool::new();
        
        // Create multiple transactions with different data (different hashes)
        for i in 0..5 {
            let tx = Transaction::new(
                format!("sender{}", i),
                format!("receiver{}", i),
                Asset::PLP,
                100 + i as u128,
                1,
                i as u64,
                HashSet::new(),
                HashSet::new(),
                format!("sig_main{}", i),
                format!("sig_derived{}", i),
            ).unwrap();
            mempool.add_transaction(tx).unwrap();
        }
        
        assert_eq!(mempool.len(), 5);
        assert!(!mempool.is_empty());
    }

    // --- Anti-starvation / fairness tests (no system time, no randomness) ---

    #[test]
    fn test_arrival_order_influences_selection() {
        let mempool = Mempool::new();
        let tx_first = Transaction::new(
            "zzz_sender".to_string(),
            "zzz_receiver".to_string(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_z".to_string(),
            "sig_z".to_string(),
        )
        .unwrap();
        let tx_second = Transaction::new(
            "aaa_sender".to_string(),
            "aaa_receiver".to_string(),
            Asset::PLP,
            200,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig_a".to_string(),
            "sig_a".to_string(),
        )
        .unwrap();
        assert_ne!(tx_first.hash, tx_second.hash);
        let (higher_hash, lower_hash) = if tx_first.hash > tx_second.hash {
            (tx_first.clone(), tx_second.clone())
        } else {
            (tx_second.clone(), tx_first.clone())
        };
        mempool.add_transaction(higher_hash.clone()).unwrap();
        mempool.add_transaction(lower_hash.clone()).unwrap();
        let batch = mempool.get_all_transactions();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].hash, higher_hash.hash);
        assert_eq!(batch[1].hash, lower_hash.hash);
    }

    #[test]
    fn test_late_hash_not_starved() {
        let mempool = Mempool::new();
        let tx_late = Transaction::new(
            "zzz_late".to_string(),
            "zzz_recv".to_string(),
            Asset::PLP,
            1,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "sig".to_string(),
            "sig".to_string(),
        )
        .unwrap();
        mempool.add_transaction(tx_late.clone()).unwrap();
        for i in 0..5 {
            let tx = Transaction::new(
                format!("aaa_early_{}", i),
                format!("aaa_r_{}", i),
                Asset::PLP,
                1,
                1,
                i as u64,
                HashSet::new(),
                HashSet::new(),
                "sig".to_string(),
                "sig".to_string(),
            )
            .unwrap();
            mempool.add_transaction(tx).unwrap();
        }
        let batch = mempool.get_all_transactions();
        assert_eq!(batch.len(), 6);
        assert_eq!(batch[0].hash, tx_late.hash);
    }

    #[test]
    fn test_same_tx_set_same_execution_order() {
        let mempool = Mempool::new();
        let mut txs = Vec::new();
        for i in 0..4 {
            let tx = Transaction::new(
                format!("s{}", i),
                format!("r{}", i),
                Asset::PLP,
                1,
                1,
                i as u64,
                HashSet::new(),
                HashSet::new(),
                "sig".to_string(),
                "sig".to_string(),
            )
            .unwrap();
            mempool.add_transaction(tx.clone()).unwrap();
            txs.push(tx);
        }
        let a = mempool.get_all_transactions();
        let b = mempool.get_all_transactions();
        assert_eq!(a.len(), b.len());
        for (i, (xa, xb)) in a.iter().zip(b.iter()).enumerate() {
            assert_eq!(xa.hash, xb.hash, "index {} differs", i);
        }
    }
}
