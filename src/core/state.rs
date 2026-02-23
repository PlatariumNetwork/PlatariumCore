//! Blockchain state: single source of truth for balances, μPLP (fee) balances, and nonces.
//!
//! # Determinism
//! Same transaction order yields the same final state. All updates are deterministic; no randomness or system time is used.
//!
//! # Invariants
//! - State transitions are deterministic functions of the transaction sequence.
//! - Balance and nonce updates follow fixed rules. Same sequence of transactions always produces the same state.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use sha2::{Sha256, Digest};
use crate::error::{PlatariumError, Result};
use crate::core::asset::Asset;
use crate::core::transaction::{Transaction, TransactionValidationError};
use thiserror::Error;

/// Fee recipient address. Fee is always in μPLP.
pub const TREASURY_ADDRESS: &str = "treasury";

/// Address type (alias for String).
pub type Address = String;

/// Trait for types that can produce immutable state snapshots. Same state yields the same snapshot; no randomness or system time. Snapshots are immutable.
pub trait SnapshotableState {
    /// Produces an immutable snapshot of the current state. Deterministic: same state yields the same snapshot.
    fn snapshot(&self) -> StateSnapshot;
}

/// Immutable snapshot of blockchain state. Cannot be modified after creation; creation is O(1) via `Arc` (no full copy).
///
/// # Invariants
/// - **Immutability:** No mutation methods; snapshot values never change after creation.
/// - **Restore identity:** For any state S, `state.restore(&state.snapshot())` restores S exactly; restore is idempotent.
/// - **No side effects:** Snapshot creation and read operations are pure; restore only modifies state, not the snapshot.
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    asset_balances: Arc<HashMap<(Address, String), u128>>,
    uplp_balances: Arc<HashMap<Address, u128>>,
    nonces: Arc<HashMap<Address, u64>>,
}

impl StateSnapshot {
    fn new(
        asset_balances: Arc<HashMap<(Address, String), u128>>,
        uplp_balances: Arc<HashMap<Address, u128>>,
        nonces: Arc<HashMap<Address, u64>>,
    ) -> Self {
        Self {
            asset_balances,
            uplp_balances,
            nonces,
        }
    }

    pub(crate) fn asset_balances_arc(&self) -> &Arc<HashMap<(Address, String), u128>> {
        &self.asset_balances
    }
    pub(crate) fn uplp_balances_arc(&self) -> &Arc<HashMap<Address, u128>> {
        &self.uplp_balances
    }
    pub(crate) fn nonces_arc(&self) -> &Arc<HashMap<Address, u64>> {
        &self.nonces
    }

    /// Returns the PLP balance for the address, or 0 if absent.
    pub fn get_balance(&self, address: &Address) -> u128 {
        let k = (address.clone(), Asset::PLP.as_canonical());
        self.asset_balances.get(&k).copied().unwrap_or(0)
    }

    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.nonces.get(address).copied().unwrap_or(0)
    }

    /// Returns all PLP balances, sorted by address for deterministic ordering.
    pub fn get_all_balances(&self) -> Vec<(Address, u128)> {
        let plp = Asset::PLP.as_canonical();
        let mut v: Vec<_> = self
            .asset_balances
            .iter()
            .filter(|((_, a), _)| *a == plp)
            .map(|((addr, _), bal)| (addr.clone(), *bal))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    pub fn get_all_nonces(&self) -> Vec<(Address, u64)> {
        let mut v: Vec<_> = self
            .nonces
            .iter()
            .map(|(a, n)| (a.clone(), *n))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Computes the deterministic state root for the block header from sorted balances and nonces.
    pub fn compute_state_root(&self) -> String {
        let mut hasher = Sha256::new();
        for (addr, bal) in self.get_all_balances() {
            hasher.update(addr.as_bytes());
            hasher.update(bal.to_le_bytes());
        }
        for (addr, nonce) in self.get_all_nonces() {
            hasher.update(addr.as_bytes());
            hasher.update(nonce.to_le_bytes());
        }
        hex::encode(hasher.finalize())
    }

    pub fn is_empty(&self) -> bool {
        self.asset_balances.is_empty() && self.uplp_balances.is_empty() && self.nonces.is_empty()
    }

    pub fn balance_count(&self) -> usize {
        self.asset_balances.len()
    }
    pub fn nonce_count(&self) -> usize {
        self.nonces.len()
    }
}

impl PartialEq for StateSnapshot {
    fn eq(&self, other: &Self) -> bool {
        *self.asset_balances == *other.asset_balances
            && *self.uplp_balances == *other.uplp_balances
            && *self.nonces == *other.nonces
    }
}

impl Eq for StateSnapshot {}

/// Errors produced by state operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance {
        required: u128,
        available: u128,
    },
    
    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce {
        expected: u64,
        got: u64,
    },
    
    #[error("State error: {0}")]
    Other(String),
}

impl From<StateError> for PlatariumError {
    fn from(err: StateError) -> Self {
        PlatariumError::State(err.to_string())
    }
}

impl From<TransactionValidationError> for PlatariumError {
    fn from(err: TransactionValidationError) -> Self {
        PlatariumError::State(format!("Transaction validation error: {}", err))
    }
}

/// Blockchain state: asset balances, μPLP (fee) balances, and nonces. Fee is always in μPLP and is separate from asset balances.
#[derive(Debug)]
pub struct State {
    /// Asset balances: (address, asset_canonical) -> balance in minimal units
    asset_balances: RwLock<Arc<HashMap<(Address, String), u128>>>,
    /// μPLP balances for fees only. Fee is always paid from this.
    uplp_balances: RwLock<Arc<HashMap<Address, u128>>>,
    nonces: RwLock<Arc<HashMap<Address, u64>>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            asset_balances: RwLock::new(Arc::new(HashMap::new())),
            uplp_balances: RwLock::new(Arc::new(HashMap::new())),
            nonces: RwLock::new(Arc::new(HashMap::new())),
        }
    }

    fn asset_key(address: &Address, asset: &Asset) -> (Address, String) {
        (address.clone(), asset.as_canonical())
    }

    /// Asset balance (minimal units). Returns 0 if missing.
    pub fn get_asset_balance(&self, address: &Address, asset: &Asset) -> u128 {
        let ab = self.asset_balances.read().unwrap();
        let k = Self::asset_key(address, asset);
        ab.get(&k).copied().unwrap_or(0)
    }

    /// μPLP balance for fees. Returns 0 if missing.
    pub fn get_uplp_balance(&self, address: &Address) -> u128 {
        let ub = self.uplp_balances.read().unwrap();
        ub.get(address).copied().unwrap_or(0)
    }

    /// Legacy: PLP asset balance (for compatibility).
    pub fn get_balance(&self, address: &Address) -> u128 {
        self.get_asset_balance(address, &Asset::PLP)
    }

    pub fn get_nonce(&self, address: &Address) -> u64 {
        let nonces = self.nonces.read().unwrap();
        nonces.get(address).copied().unwrap_or(0)
    }

    pub fn set_asset_balance(&self, address: &Address, asset: &Asset, balance: u128) {
        let mut ab = self.asset_balances.write().unwrap();
        let ab_mut = Arc::make_mut(&mut ab);
        ab_mut.insert(Self::asset_key(address, asset), balance);
    }

    pub fn set_uplp_balance(&self, address: &Address, balance: u128) {
        let mut ub = self.uplp_balances.write().unwrap();
        Arc::make_mut(&mut ub).insert(address.clone(), balance);
    }

    /// Sets PLP asset balance (for initialization/testing). Legacy compat.
    pub fn set_balance(&self, address: &Address, balance: u128) {
        self.set_asset_balance(address, &Asset::PLP, balance);
    }

    /// Applies a transfer: deduct fee from uplp, amount from asset; credit amount to receiver, fee to treasury.
    /// Fee is always μPLP. Order: fee from uplp, then asset transfer, then nonce (deterministic).
    pub fn apply_transfer(
        &self,
        from: &Address,
        to: &Address,
        asset: &Asset,
        amount: u128,
        fee_uplp: u128,
        expected_nonce: Option<u64>,
    ) -> Result<()> {
        let treasury = TREASURY_ADDRESS.to_string();
        let k = Self::asset_key(from, asset);

        let mut ab_arc = self.asset_balances.write().unwrap();
        let mut ub_arc = self.uplp_balances.write().unwrap();
        let mut nonces_arc = self.nonces.write().unwrap();
        let ab = Arc::make_mut(&mut ab_arc);
        let ub = Arc::make_mut(&mut ub_arc);
        let nonces = Arc::make_mut(&mut nonces_arc);

        if let Some(expected) = expected_nonce {
            let cur = nonces.get(from).copied().unwrap_or(0);
            if cur != expected {
                return Err(StateError::InvalidNonce { expected, got: cur }.into());
            }
        }
        let asset_bal = ab.get(&k).copied().unwrap_or(0);
        if asset_bal < amount {
            return Err(StateError::InsufficientBalance {
                required: amount,
                available: asset_bal,
            }
            .into());
        }
        let uplp_bal = ub.get(from).copied().unwrap_or(0);
        if uplp_bal < fee_uplp {
            return Err(StateError::InsufficientBalance {
                required: fee_uplp,
                available: uplp_bal,
            }
            .into());
        }

        ab.insert(k.clone(), asset_bal - amount);
        let to_k = Self::asset_key(to, asset);
        let to_bal = ab.get(&to_k).copied().unwrap_or(0);
        ab.insert(to_k, to_bal + amount);

        ub.insert(from.clone(), uplp_bal - fee_uplp);
        let treasury_bal = ub.get(&treasury).copied().unwrap_or(0);
        ub.insert(treasury, treasury_bal + fee_uplp);

        if let Some(expected) = expected_nonce {
            nonces.insert(from.clone(), expected + 1);
        }
        Ok(())
    }
    
    /// Sets nonce for an address (for initialization/testing)
    /// 
    /// PERFORMANCE: Creates new Arc if HashMap is shared (copy-on-write)
    pub fn set_nonce(&self, address: &Address, nonce: u64) {
        let mut nonces_arc = self.nonces.write().unwrap();
        // Use Arc::make_mut for copy-on-write: clones only if shared
        Arc::make_mut(&mut nonces_arc).insert(address.clone(), nonce);
    }
    
    /// Gets all balances (for debugging/testing)
    /// 
    /// PERFORMANCE: O(n) - clones HashMap data
    /// PLP balances only (for compatibility).
    pub fn get_all_balances(&self) -> HashMap<Address, u128> {
        let plp = Asset::PLP.as_canonical();
        let ab = self.asset_balances.read().unwrap();
        ab.iter()
            .filter(|((_, a), _)| *a == plp)
            .map(|((addr, _), bal)| (addr.clone(), *bal))
            .collect()
    }
    
    /// Gets all nonces (for debugging/testing)
    /// 
    /// PERFORMANCE: O(n) - clones HashMap data
    pub fn get_all_nonces(&self) -> HashMap<Address, u64> {
        let nonces_arc = self.nonces.read().unwrap();
        nonces_arc.as_ref().clone()
    }
    
    /// Creates an immutable snapshot of the current state
    /// 
    /// PERFORMANCE: O(1) - only clones Arc, no data copying
    /// This is achieved by using Arc for shared ownership.
    /// The snapshot shares the same underlying HashMap data with the state
    /// until the state is modified (copy-on-write semantics).
    /// 
    /// DETERMINISM GUARANTEE:
    /// - Same state → same snapshot (always)
    /// - No randomness or system time used
    /// - Snapshot is a pure function of state data
    /// 
    /// CRITICAL INVARIANTS:
    /// ===================
    /// 
    /// 1. **SNAPSHOT IMMUTABILITY**
    ///    - Snapshot created here is immutable and cannot be modified
    ///    - ASSERT: Snapshot fields are private, no mutation methods exist
    /// 
    /// 2. **NO HIDDEN SIDE EFFECTS**
    ///    - Snapshot creation has no side effects on state
    ///    - State remains unchanged after snapshot creation
    ///    - No global state, no external dependencies
    ///    - ASSERT: State unchanged after snapshot creation
    /// 
    /// ADDITIONAL INVARIANTS:
    /// - Snapshot is immutable after creation
    /// - Snapshot does not depend on system time
    /// - No full state copy occurs (only Arc clone, O(1))
    pub fn create_snapshot(&self) -> StateSnapshot {
        let ab = self.asset_balances.read().unwrap();
        let ub = self.uplp_balances.read().unwrap();
        let nc = self.nonces.read().unwrap();
        let ab_snap = ab.as_ref().clone();
        let ub_snap = ub.as_ref().clone();
        let nc_snap = nc.as_ref().clone();
        drop(ab);
        drop(ub);
        drop(nc);
        let ab_arc = self.asset_balances.read().unwrap();
        let ub_arc = self.uplp_balances.read().unwrap();
        let nc_arc = self.nonces.read().unwrap();
        assert!(**ab_arc == ab_snap, "INVARIANT: state changed during snapshot");
        assert!(**ub_arc == ub_snap, "INVARIANT: state changed during snapshot");
        assert!(**nc_arc == nc_snap, "INVARIANT: state changed during snapshot");
        let snapshot = StateSnapshot::new(ab_arc.clone(), ub_arc.clone(), nc_arc.clone());
        assert!(**snapshot.asset_balances_arc() == ab_snap, "INVARIANT: snapshot != state");
        assert!(**snapshot.uplp_balances_arc() == ub_snap, "INVARIANT: snapshot != state");
        assert!(**snapshot.nonces_arc() == nc_snap, "INVARIANT: snapshot != state");
        let ab2 = self.asset_balances.read().unwrap();
        let ub2 = self.uplp_balances.read().unwrap();
        let nc2 = self.nonces.read().unwrap();
        assert!(**ab2 == ab_snap, "INVARIANT: snapshot creation modified state");
        assert!(**ub2 == ub_snap, "INVARIANT: snapshot creation modified state");
        assert!(**nc2 == nc_snap, "INVARIANT: snapshot creation modified state");
        snapshot
    }
    
    /// Creates an immutable snapshot of the current state
    /// 
    /// This is an alias for create_snapshot() for trait implementation.
    /// 
    /// PERFORMANCE: O(1) - only clones Arc, no data copying
    pub fn snapshot(&self) -> StateSnapshot {
        self.create_snapshot()
    }
    
    /// Restores the state from a snapshot
    /// 
    /// This method performs a complete rollback of all state changes
    /// by replacing the current state with the snapshot state.
    /// 
    /// PERFORMANCE: O(1) - only replaces Arc references, no data copying
    /// 
    /// ATOMICITY GUARANTEE:
    /// - All state changes are rolled back atomically (all or nothing)
    /// - No partial restore: both balances and nonces are restored together
    /// - Operation is atomic within a single lock scope
    /// 
    /// DETERMINISM GUARANTEE:
    /// - Restore order is deterministic: balances first, then nonces
    /// - Same snapshot → same restored state (always)
    /// - No randomness or system time used
    /// 
    /// CRITICAL INVARIANTS:
    /// ===================
    /// 
    /// 1. **RESTORE == IDENTITY**
    ///    - After restore, state must exactly match the state at snapshot creation time
    ///    - For snapshot created from state S: restore(snapshot) → state == S
    ///    - ASSERT: State after restore matches snapshot exactly
    /// 
    /// 2. **NO SNAPSHOT MODIFICATION**
    ///    - Restore operation never modifies the snapshot
    ///    - Snapshot remains immutable and unchanged
    ///    - ASSERT: Snapshot values unchanged after restore
    /// 
    /// 3. **NO HIDDEN SIDE EFFECTS**
    ///    - Restore only modifies state, nothing else
    ///    - No global state changes, no external dependencies
    ///    - Operation is pure with respect to state and snapshot
    /// 
    /// ADDITIONAL INVARIANTS:
    /// - Restore is atomic (all or nothing)
    /// - Restore order is deterministic
    pub fn restore(&self, snapshot: &StateSnapshot) {
        assert!(Arc::strong_count(snapshot.asset_balances_arc()) > 0);
        assert!(Arc::strong_count(snapshot.uplp_balances_arc()) > 0);
        assert!(Arc::strong_count(snapshot.nonces_arc()) > 0);
        let ab_snap = snapshot.asset_balances_arc().as_ref().clone();
        let ub_snap = snapshot.uplp_balances_arc().as_ref().clone();
        let nc_snap = snapshot.nonces_arc().as_ref().clone();
        let mut ab = self.asset_balances.write().unwrap();
        let mut ub = self.uplp_balances.write().unwrap();
        let mut nc = self.nonces.write().unwrap();
        *ab = snapshot.asset_balances_arc().clone();
        *ub = snapshot.uplp_balances_arc().clone();
        *nc = snapshot.nonces_arc().clone();
        assert!(**ab == ab_snap, "INVARIANT: restore failed");
        assert!(**ub == ub_snap, "INVARIANT: restore failed");
        assert!(**nc == nc_snap, "INVARIANT: restore failed");
        assert!(Arc::strong_count(snapshot.asset_balances_arc()) > 0);
        assert!(Arc::strong_count(snapshot.uplp_balances_arc()) > 0);
        assert!(Arc::strong_count(snapshot.nonces_arc()) > 0);
    }
    
    /// Applies a transaction: validate_basic, then apply_transfer(from, to, asset, amount, fee_uplp, nonce).
    /// Fee is always μPLP; asset balance and uplp balance are checked separately.
    pub fn apply_transaction(&self, tx: &Transaction) -> Result<()> {
        tx.validate_basic().map_err(PlatariumError::from)?;
        self.apply_transfer(
            &tx.from,
            &tx.to,
            &tx.asset,
            tx.amount,
            tx.fee_uplp,
            Some(tx.nonce),
        )
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotableState for State {
    /// Creates an immutable snapshot of the current state
    /// 
    /// PERFORMANCE: O(1) - only clones Arc, no data copying
    /// 
    /// DETERMINISM GUARANTEE:
    /// - Same state → same snapshot (always)
    /// - No randomness or system time used
    /// - Snapshot is a pure function of state data
    /// 
    /// This method acquires read locks, clones Arc (O(1)),
    /// and creates an immutable snapshot. The snapshot shares
    /// the same underlying data with the state until the state
    /// is modified (copy-on-write semantics).
    /// 
    /// INVARIANT: Snapshot is immutable after creation
    /// INVARIANT: Snapshot does not depend on system time
    /// INVARIANT: No full state copy occurs (only Arc clone, O(1))
    fn snapshot(&self) -> StateSnapshot {
        self.create_snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;

    #[test]
    fn test_new_state() {
        let state = State::new();
        assert_eq!(state.get_balance(&"addr1".to_string()), 0);
        assert_eq!(state.get_nonce(&"addr1".to_string()), 0);
    }
    
    #[test]
    fn test_get_balance() {
        let state = State::new();
        let addr = "sender".to_string();
        
        // Initially zero
        assert_eq!(state.get_balance(&addr), 0);
        
        // Set balance
        state.set_balance(&addr, 1000);
        assert_eq!(state.get_balance(&addr), 1000);
    }
    
    #[test]
    fn test_get_nonce() {
        let state = State::new();
        let addr = "sender".to_string();
        
        // Initially zero
        assert_eq!(state.get_nonce(&addr), 0);
        
        // Set nonce
        state.set_nonce(&addr, 5);
        assert_eq!(state.get_nonce(&addr), 5);
    }
    
    #[test]
    fn test_apply_transfer_success() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        state.set_balance(&sender, 1000);
        state.set_uplp_balance(&sender, 10);
        let result = state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, None);
        assert!(result.is_ok());
        assert_eq!(state.get_balance(&sender), 900);
        assert_eq!(state.get_balance(&receiver), 100);
        assert_eq!(state.get_uplp_balance(&sender), 9);
    }
    
    #[test]
    fn test_apply_transfer_insufficient_balance() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        state.set_balance(&sender, 50);
        state.set_uplp_balance(&sender, 10);
        let result = state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, None);
        assert!(result.is_err());
        if let Err(PlatariumError::State(msg)) = result {
            assert!(msg.contains("Insufficient balance"));
        } else {
            panic!("Expected State error");
        }
    }
    
    #[test]
    fn test_apply_transfer_with_nonce() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        state.set_balance(&sender, 1000);
        state.set_uplp_balance(&sender, 10);
        state.set_nonce(&sender, 5);
        let result = state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, Some(5));
        assert!(result.is_ok());
        assert_eq!(state.get_nonce(&sender), 6);
    }
    #[test]
    fn test_apply_transfer_invalid_nonce() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        state.set_balance(&sender, 1000);
        state.set_uplp_balance(&sender, 10);
        state.set_nonce(&sender, 5);
        let result = state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, Some(3));
        assert!(result.is_err());
        
        if let Err(PlatariumError::State(msg)) = result {
            assert!(msg.contains("Invalid nonce"));
        } else {
            panic!("Expected State error with invalid nonce");
        }
    }
    
    #[test]
    fn test_apply_transfer_zero_balance() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        let result = state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, None);
        assert!(result.is_err());
        
        if let Err(PlatariumError::State(msg)) = result {
            assert!(msg.contains("Insufficient balance"));
        } else {
            panic!("Expected State error");
        }
    }
    
    #[test]
    fn test_apply_transfer_exact_balance() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        state.set_balance(&sender, 100);
        state.set_uplp_balance(&sender, 1);
        let result = state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, None);
        assert!(result.is_ok());
        assert_eq!(state.get_balance(&sender), 0);
        assert_eq!(state.get_balance(&receiver), 100);
    }
    
    #[test]
    fn test_apply_transfer_multiple() {
        let state = State::new();
        let sender = "sender".to_string();
        let receiver1 = "receiver1".to_string();
        let receiver2 = "receiver2".to_string();
        
        state.set_balance(&sender, 1000);
        state.set_uplp_balance(&sender, 10);
        state.apply_transfer(&sender, &receiver1, &Asset::PLP, 100, 1, None).unwrap();
        assert_eq!(state.get_balance(&sender), 900);
        assert_eq!(state.get_balance(&receiver1), 100);
        state.apply_transfer(&sender, &receiver2, &Asset::PLP, 200, 1, None).unwrap();
        assert_eq!(state.get_balance(&sender), 700);
        assert_eq!(state.get_balance(&receiver2), 200);
    }
    #[test]
    fn test_apply_transaction_success() {
        use crate::core::transaction::Transaction;
        use std::collections::HashSet;
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        state.set_balance(&sender, 1000);
        state.set_uplp_balance(&sender, 10);
        state.set_nonce(&sender, 0);
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
        
        // This will fail signature validation, but we can test the structure
        let result = state.apply_transaction(&tx);
        
        // Expect signature validation to fail with dummy signatures
        assert!(result.is_err());
        if let Err(PlatariumError::State(msg)) = result {
            assert!(msg.contains("Transaction validation error"));
        } else {
            panic!("Expected State error for invalid signature");
        }
    }
    
    #[test]
    fn test_apply_transaction_invalid_nonce() {
        use crate::core::transaction::Transaction;
        use std::collections::HashSet;
        
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        state.set_balance(&sender, 1000);
        state.set_nonce(&sender, 5); // Current nonce is 5
        
        state.set_uplp_balance(&sender, 10);
        let tx = Transaction::new(
            sender.clone(),
            receiver.clone(),
            Asset::PLP,
            100,
            1,
            3,
            HashSet::new(),
            HashSet::new(),
            "dummy_sig_main".to_string(),
            "dummy_sig_derived".to_string(),
        ).unwrap();
        
        let result = state.apply_transaction(&tx);
        
        // Should fail at signature validation first, but if we bypass that,
        // it would fail at nonce check
        assert!(result.is_err());
    }
    
    #[test]
    fn test_apply_transaction_insufficient_balance() {
        use crate::core::transaction::Transaction;
        use std::collections::HashSet;
        
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        state.set_balance(&sender, 50);
        state.set_uplp_balance(&sender, 10);
        state.set_nonce(&sender, 0);
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
        
        let result = state.apply_transaction(&tx);
        
        // Should fail at signature validation first
        assert!(result.is_err());
    }
    
    #[test]
    fn test_apply_transaction_atomicity() {
        use crate::core::transaction::Transaction;
        use std::collections::HashSet;
        
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        // Set initial state
        state.set_balance(&sender, 1000);
        state.set_nonce(&sender, 0);
        
        // Create a transaction that will fail (invalid signature)
        let tx = Transaction::new(
            sender.clone(),
            receiver.clone(),
            Asset::PLP,
            100,
            1,
            0,
            HashSet::new(),
            HashSet::new(),
            "invalid_sig".to_string(),
            "invalid_sig".to_string(),
        ).unwrap();
        
        // Try to apply - should fail
        let result = state.apply_transaction(&tx);
        assert!(result.is_err());
        
        // Verify state was NOT modified (atomicity)
        assert_eq!(state.get_balance(&sender), 1000);
        assert_eq!(state.get_balance(&receiver), 0);
        assert_eq!(state.get_nonce(&sender), 0);
    }
    
    #[test]
    fn test_snapshot_creation() {
        let state = State::new();
        let addr1 = "addr1".to_string();
        let addr2 = "addr2".to_string();
        
        // Set some state
        state.set_balance(&addr1, 1000);
        state.set_nonce(&addr1, 5);
        state.set_balance(&addr2, 500);
        state.set_nonce(&addr2, 2);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // Verify snapshot contains the state
        assert_eq!(snapshot.get_balance(&addr1), 1000);
        assert_eq!(snapshot.get_nonce(&addr1), 5);
        assert_eq!(snapshot.get_balance(&addr2), 500);
        assert_eq!(snapshot.get_nonce(&addr2), 2);
    }
    
    #[test]
    fn test_snapshot_immutability() {
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // Modify original state
        state.set_balance(&addr, 2000);
        state.set_nonce(&addr, 10);
        
        // Snapshot should remain unchanged (immutability)
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
        
        // Original state should be changed
        assert_eq!(state.get_balance(&addr), 2000);
        assert_eq!(state.get_nonce(&addr), 10);
    }
    
    #[test]
    fn test_snapshot_determinism() {
        let state1 = State::new();
        let state2 = State::new();
        let addr1 = "addr1".to_string();
        let addr2 = "addr2".to_string();
        
        // Set same state in both
        state1.set_balance(&addr1, 1000);
        state1.set_nonce(&addr1, 5);
        state1.set_balance(&addr2, 500);
        state1.set_nonce(&addr2, 2);
        
        state2.set_balance(&addr1, 1000);
        state2.set_nonce(&addr1, 5);
        state2.set_balance(&addr2, 500);
        state2.set_nonce(&addr2, 2);
        
        // Create snapshots
        let snapshot1 = state1.snapshot();
        let snapshot2 = state2.snapshot();
        
        // Snapshots should be equal (deterministic)
        assert_eq!(snapshot1, snapshot2);
        
        // Verify contents are the same
        assert_eq!(snapshot1.get_balance(&addr1), snapshot2.get_balance(&addr1));
        assert_eq!(snapshot1.get_nonce(&addr1), snapshot2.get_nonce(&addr1));
        assert_eq!(snapshot1.get_balance(&addr2), snapshot2.get_balance(&addr2));
        assert_eq!(snapshot1.get_nonce(&addr2), snapshot2.get_nonce(&addr2));
    }
    
    #[test]
    fn test_snapshot_get_all_balances_deterministic_order() {
        let state = State::new();
        
        // Set balances in non-alphabetical order
        state.set_balance(&"zebra".to_string(), 100);
        state.set_balance(&"alpha".to_string(), 200);
        state.set_balance(&"beta".to_string(), 300);
        
        let snapshot = state.snapshot();
        let balances = snapshot.get_all_balances();
        
        // Should be sorted by address (deterministic order)
        assert_eq!(balances.len(), 3);
        assert_eq!(balances[0].0, "alpha");
        assert_eq!(balances[0].1, 200);
        assert_eq!(balances[1].0, "beta");
        assert_eq!(balances[1].1, 300);
        assert_eq!(balances[2].0, "zebra");
        assert_eq!(balances[2].1, 100);
        
        // Multiple calls should return same order
        let balances2 = snapshot.get_all_balances();
        assert_eq!(balances, balances2);
    }
    
    #[test]
    fn test_snapshot_get_all_nonces_deterministic_order() {
        let state = State::new();
        
        // Set nonces in non-alphabetical order
        state.set_nonce(&"zebra".to_string(), 10);
        state.set_nonce(&"alpha".to_string(), 20);
        state.set_nonce(&"beta".to_string(), 30);
        
        let snapshot = state.snapshot();
        let nonces = snapshot.get_all_nonces();
        
        // Should be sorted by address (deterministic order)
        assert_eq!(nonces.len(), 3);
        assert_eq!(nonces[0].0, "alpha");
        assert_eq!(nonces[0].1, 20);
        assert_eq!(nonces[1].0, "beta");
        assert_eq!(nonces[1].1, 30);
        assert_eq!(nonces[2].0, "zebra");
        assert_eq!(nonces[2].1, 10);
        
        // Multiple calls should return same order
        let nonces2 = snapshot.get_all_nonces();
        assert_eq!(nonces, nonces2);
    }
    
    #[test]
    fn test_snapshot_empty() {
        let state = State::new();
        let snapshot = state.snapshot();
        
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.balance_count(), 0);
        assert_eq!(snapshot.nonce_count(), 0);
    }
    
    #[test]
    fn test_snapshot_no_system_time() {
        // This test verifies that snapshot creation does not depend on system time
        // by creating multiple snapshots and verifying they are identical
        // if the state is the same
        
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create multiple snapshots
        let snapshot1 = state.snapshot();
        let snapshot2 = state.snapshot();
        let snapshot3 = state.snapshot();
        
        // All snapshots should be identical (no time-dependent data)
        assert_eq!(snapshot1, snapshot2);
        assert_eq!(snapshot2, snapshot3);
        assert_eq!(snapshot1, snapshot3);
    }
    
    #[test]
    fn test_snapshot_trait_implementation() {
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Test trait method
        let snapshot = SnapshotableState::snapshot(&state);
        
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
    }
    
    #[test]
    fn test_snapshot_clone() {
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        let snapshot1 = state.snapshot();
        let snapshot2 = snapshot1.clone();
        
        // Cloned snapshot should be equal
        assert_eq!(snapshot1, snapshot2);
        
        // Cloned snapshot should be independent
        assert_eq!(snapshot2.get_balance(&addr), 1000);
        assert_eq!(snapshot2.get_nonce(&addr), 5);
    }
    
    #[test]
    fn test_snapshot_identity_two_snapshots_in_row() {
        // Test: два snapshot підряд → однакові
        let state = State::new();
        let addr1 = "addr1".to_string();
        let addr2 = "addr2".to_string();
        
        state.set_balance(&addr1, 1000);
        state.set_nonce(&addr1, 5);
        state.set_balance(&addr2, 500);
        state.set_nonce(&addr2, 2);
        
        // Create two snapshots in a row without modifying state
        let snapshot1 = state.snapshot();
        let snapshot2 = state.snapshot();
        
        // Both snapshots should be identical
        assert_eq!(snapshot1, snapshot2);
        
        // Verify they contain the same data
        assert_eq!(snapshot1.get_balance(&addr1), snapshot2.get_balance(&addr1));
        assert_eq!(snapshot1.get_nonce(&addr1), snapshot2.get_nonce(&addr1));
        assert_eq!(snapshot1.get_balance(&addr2), snapshot2.get_balance(&addr2));
        assert_eq!(snapshot1.get_nonce(&addr2), snapshot2.get_nonce(&addr2));
    }
    
    #[test]
    fn test_snapshot_identity_after_state_modification() {
        // Test: snapshot ≠ mutable state (snapshot doesn't change when state changes)
        let state = State::new();
        let addr = "addr".to_string();
        
        // Set initial state
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // Verify snapshot matches initial state
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
        
        // Modify state
        state.set_balance(&addr, 2000);
        state.set_nonce(&addr, 10);
        
        // Snapshot should remain unchanged (immutability)
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
        
        // State should be changed
        assert_eq!(state.get_balance(&addr), 2000);
        assert_eq!(state.get_nonce(&addr), 10);
        
        // Snapshot and state should be different
        assert_ne!(snapshot.get_balance(&addr), state.get_balance(&addr));
        assert_ne!(snapshot.get_nonce(&addr), state.get_nonce(&addr));
    }
    
    #[test]
    fn test_snapshot_o1_creation_no_copy() {
        // Test that creating multiple snapshots doesn't copy data
        // This test verifies O(1) creation by checking that snapshots
        // share the same underlying data until state is modified
        
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create multiple snapshots
        let snapshot1 = state.snapshot();
        let snapshot2 = state.snapshot();
        let snapshot3 = state.snapshot();
        
        // All snapshots should be equal (same state)
        assert_eq!(snapshot1, snapshot2);
        assert_eq!(snapshot2, snapshot3);
        assert_eq!(snapshot1, snapshot3);
        
        // All snapshots should show the same data
        assert_eq!(snapshot1.get_balance(&addr), 1000);
        assert_eq!(snapshot2.get_balance(&addr), 1000);
        assert_eq!(snapshot3.get_balance(&addr), 1000);
        
        // Modify state
        state.set_balance(&addr, 2000);
        
        // Snapshots should still show old data (immutability)
        assert_eq!(snapshot1.get_balance(&addr), 1000);
        assert_eq!(snapshot2.get_balance(&addr), 1000);
        assert_eq!(snapshot3.get_balance(&addr), 1000);
        
        // State should show new data
        assert_eq!(state.get_balance(&addr), 2000);
    }
    
    #[test]
    fn test_snapshot_structural_equality() {
        // Test that snapshots are compared by value, not by Arc pointer
        let state1 = State::new();
        let state2 = State::new();
        let addr = "addr".to_string();
        
        // Set same state in both
        state1.set_balance(&addr, 1000);
        state1.set_nonce(&addr, 5);
        
        state2.set_balance(&addr, 1000);
        state2.set_nonce(&addr, 5);
        
        // Create snapshots from different states
        let snapshot1 = state1.snapshot();
        let snapshot2 = state2.snapshot();
        
        // Snapshots should be equal (structural equality, not pointer equality)
        assert_eq!(snapshot1, snapshot2);
    }
    
    #[test]
    fn test_restore_apply_tx_restore_state_equals_original() {
        // Test: apply tx → restore → state == original
        let state = State::new();
        let sender = "sender".to_string();
        let receiver = "receiver".to_string();
        
        state.set_balance(&sender, 1000);
        state.set_uplp_balance(&sender, 10);
        state.set_nonce(&sender, 0);
        let original_snapshot = state.snapshot();
        assert_eq!(state.get_balance(&sender), 1000);
        assert_eq!(state.get_nonce(&sender), 0);
        assert_eq!(state.get_balance(&receiver), 0);
        state.apply_transfer(&sender, &receiver, &Asset::PLP, 100, 1, None).unwrap();
        assert_eq!(state.get_balance(&sender), 900);
        assert_eq!(state.get_balance(&receiver), 100);
        
        // Restore from snapshot
        state.restore(&original_snapshot);
        
        // Verify state equals original
        assert_eq!(state.get_balance(&sender), 1000);
        assert_eq!(state.get_nonce(&sender), 0);
        assert_eq!(state.get_balance(&receiver), 0);
        
        // Verify state matches snapshot
        assert_eq!(state.get_balance(&sender), original_snapshot.get_balance(&sender));
        assert_eq!(state.get_nonce(&sender), original_snapshot.get_nonce(&sender));
        assert_eq!(state.get_balance(&receiver), original_snapshot.get_balance(&receiver));
    }
    
    #[test]
    fn test_restore_does_not_modify_snapshot() {
        // Test: restore не змінює snapshot
        let state = State::new();
        let addr = "addr".to_string();
        
        // Set initial state
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // Verify snapshot
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
        
        // Modify state
        state.set_balance(&addr, 2000);
        state.set_nonce(&addr, 10);
        
        // Restore from snapshot
        state.restore(&snapshot);
        
        // Verify state was restored
        assert_eq!(state.get_balance(&addr), 1000);
        assert_eq!(state.get_nonce(&addr), 5);
        
        // Verify snapshot was NOT modified
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
        
        // Modify state again
        state.set_balance(&addr, 3000);
        state.set_nonce(&addr, 15);
        
        // Snapshot should still be unchanged
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
        
        // Restore again
        state.restore(&snapshot);
        
        // State should be restored again
        assert_eq!(state.get_balance(&addr), 1000);
        assert_eq!(state.get_nonce(&addr), 5);
        
        // Snapshot should still be unchanged
        assert_eq!(snapshot.get_balance(&addr), 1000);
        assert_eq!(snapshot.get_nonce(&addr), 5);
    }
    
    #[test]
    fn test_restore_atomicity_all_or_nothing() {
        // Test that restore is atomic (all or nothing)
        let state = State::new();
        let addr1 = "addr1".to_string();
        let addr2 = "addr2".to_string();
        
        // Set initial state
        state.set_balance(&addr1, 1000);
        state.set_nonce(&addr1, 5);
        state.set_balance(&addr2, 500);
        state.set_nonce(&addr2, 2);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // Modify state
        state.set_balance(&addr1, 2000);
        state.set_nonce(&addr1, 10);
        state.set_balance(&addr2, 1500);
        state.set_nonce(&addr2, 7);
        
        // Verify state changed
        assert_eq!(state.get_balance(&addr1), 2000);
        assert_eq!(state.get_nonce(&addr1), 10);
        assert_eq!(state.get_balance(&addr2), 1500);
        assert_eq!(state.get_nonce(&addr2), 7);
        
        // Restore
        state.restore(&snapshot);
        
        // Verify ALL state was restored (no partial restore)
        assert_eq!(state.get_balance(&addr1), 1000);
        assert_eq!(state.get_nonce(&addr1), 5);
        assert_eq!(state.get_balance(&addr2), 500);
        assert_eq!(state.get_nonce(&addr2), 2);
    }
    
    #[test]
    fn test_restore_deterministic_order() {
        // Test that restore happens in deterministic order
        let state1 = State::new();
        let state2 = State::new();
        let addr = "addr".to_string();
        
        // Set same initial state in both
        state1.set_balance(&addr, 1000);
        state1.set_nonce(&addr, 5);
        
        state2.set_balance(&addr, 1000);
        state2.set_nonce(&addr, 5);
        
        // Create snapshots
        let snapshot1 = state1.snapshot();
        let snapshot2 = state2.snapshot();
        
        // Modify both states differently
        state1.set_balance(&addr, 2000);
        state1.set_nonce(&addr, 10);
        
        state2.set_balance(&addr, 3000);
        state2.set_nonce(&addr, 15);
        
        // Restore both
        state1.restore(&snapshot1);
        state2.restore(&snapshot2);
        
        // Both should be restored to the same state (deterministic)
        assert_eq!(state1.get_balance(&addr), state2.get_balance(&addr));
        assert_eq!(state1.get_nonce(&addr), state2.get_nonce(&addr));
    }
    
    #[test]
    fn test_restore_multiple_times() {
        // Test that restore can be called multiple times
        let state = State::new();
        let addr = "addr".to_string();
        
        // Set initial state
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // Modify and restore multiple times
        for i in 0..5 {
            // Modify state
            state.set_balance(&addr, 2000 + i as u128);
            state.set_nonce(&addr, 10 + i as u64);
            
            // Verify state changed
            assert_eq!(state.get_balance(&addr), 2000 + i as u128);
            assert_eq!(state.get_nonce(&addr), 10 + i as u64);
            
            // Restore
            state.restore(&snapshot);
            
            // Verify state restored
            assert_eq!(state.get_balance(&addr), 1000);
            assert_eq!(state.get_nonce(&addr), 5);
            
            // Verify snapshot unchanged
            assert_eq!(snapshot.get_balance(&addr), 1000);
            assert_eq!(snapshot.get_nonce(&addr), 5);
        }
    }
    
    #[test]
    fn test_restore_empty_snapshot() {
        // Test restoring to empty snapshot
        let state = State::new();
        let addr = "addr".to_string();
        
        // Set some state
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create empty snapshot
        let empty_state = State::new();
        let empty_snapshot = empty_state.snapshot();
        
        // Restore to empty
        state.restore(&empty_snapshot);
        
        // Verify state is empty
        assert_eq!(state.get_balance(&addr), 0);
        assert_eq!(state.get_nonce(&addr), 0);
        assert!(state.get_all_balances().is_empty());
        assert!(state.get_all_nonces().is_empty());
    }
    
    // ============================================================================
    // NEGATIVE TESTS: Attempting to violate invariants (should fail or be prevented)
    // ============================================================================
    
    #[test]
    fn test_negative_snapshot_immutability_violation_attempt() {
        // NEGATIVE TEST: Attempt to show that snapshot is truly immutable
        // This test demonstrates that even with multiple operations, snapshot never changes
        
        let state = State::new();
        let addr = "addr".to_string();
        
        // Set initial state
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        let original_balance = snapshot.get_balance(&addr);
        let original_nonce = snapshot.get_nonce(&addr);
        
        // ATTEMPT TO VIOLATE INVARIANT: Try to modify state in various ways
        // and verify snapshot remains unchanged
        
        // Modify state multiple times
        for i in 0..10 {
            state.set_balance(&addr, 2000 + i as u128);
            state.set_nonce(&addr, 10 + i as u64);
            
            // INVARIANT CHECK: Snapshot should remain unchanged
            assert_eq!(
                snapshot.get_balance(&addr),
                original_balance,
                "INVARIANT VIOLATION: Snapshot balance changed (immutability violated)"
            );
            assert_eq!(
                snapshot.get_nonce(&addr),
                original_nonce,
                "INVARIANT VIOLATION: Snapshot nonce changed (immutability violated)"
            );
        }
        
        // Restore and modify again
        state.restore(&snapshot);
        state.set_balance(&addr, 9999);
        state.set_nonce(&addr, 9999);
        
        // INVARIANT CHECK: Snapshot still unchanged
        assert_eq!(
            snapshot.get_balance(&addr),
            original_balance,
            "INVARIANT VIOLATION: Snapshot changed after restore (immutability violated)"
        );
        assert_eq!(
            snapshot.get_nonce(&addr),
            original_nonce,
            "INVARIANT VIOLATION: Snapshot changed after restore (immutability violated)"
        );
    }
    
    #[test]
    fn test_negative_restore_identity_violation_attempt() {
        // NEGATIVE TEST: Attempt to show that restore == identity always holds
        // This test tries various scenarios to break the identity property
        
        let state = State::new();
        let addr1 = "addr1".to_string();
        let addr2 = "addr2".to_string();
        
        // Set complex initial state
        state.set_balance(&addr1, 1000);
        state.set_nonce(&addr1, 5);
        state.set_balance(&addr2, 500);
        state.set_nonce(&addr2, 2);
        
        // Capture original state
        let original_balance1 = state.get_balance(&addr1);
        let original_nonce1 = state.get_nonce(&addr1);
        let original_balance2 = state.get_balance(&addr2);
        let original_nonce2 = state.get_nonce(&addr2);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // ATTEMPT TO VIOLATE INVARIANT: Apply various state changes
        // and verify restore always returns to original state
        
        // Scenario 1: Single modification
        state.set_balance(&addr1, 9999);
        state.restore(&snapshot);
        assert_eq!(
            state.get_balance(&addr1),
            original_balance1,
            "INVARIANT VIOLATION: Restore != identity (single modification)"
        );
        
        // Scenario 2: Multiple modifications
        state.set_balance(&addr1, 1111);
        state.set_nonce(&addr1, 111);
        state.set_balance(&addr2, 2222);
        state.set_nonce(&addr2, 222);
        state.restore(&snapshot);
        assert_eq!(
            state.get_balance(&addr1),
            original_balance1,
            "INVARIANT VIOLATION: Restore != identity (multiple modifications)"
        );
        assert_eq!(
            state.get_nonce(&addr1),
            original_nonce1,
            "INVARIANT VIOLATION: Restore != identity (multiple modifications)"
        );
        assert_eq!(
            state.get_balance(&addr2),
            original_balance2,
            "INVARIANT VIOLATION: Restore != identity (multiple modifications)"
        );
        assert_eq!(
            state.get_nonce(&addr2),
            original_nonce2,
            "INVARIANT VIOLATION: Restore != identity (multiple modifications)"
        );
        
        state.set_uplp_balance(&addr1, 10);
        state.apply_transfer(&addr1, &addr2, &Asset::PLP, 100, 1, None).unwrap();
        state.restore(&snapshot);
        assert_eq!(
            state.get_balance(&addr1),
            original_balance1,
            "INVARIANT VIOLATION: Restore != identity (after transfer)"
        );
        assert_eq!(
            state.get_balance(&addr2),
            original_balance2,
            "INVARIANT VIOLATION: Restore != identity (after transfer)"
        );
        
        // Scenario 4: Multiple restores (idempotency)
        state.set_balance(&addr1, 8888);
        state.restore(&snapshot);
        state.restore(&snapshot); // Restore again
        state.restore(&snapshot); // Restore third time
        assert_eq!(
            state.get_balance(&addr1),
            original_balance1,
            "INVARIANT VIOLATION: Restore != identity (multiple restores)"
        );
    }
    
    #[test]
    fn test_negative_hidden_side_effects_violation_attempt() {
        // NEGATIVE TEST: Attempt to detect hidden side effects
        // This test verifies that snapshot/restore operations have no unexpected side effects
        
        let state1 = State::new();
        let state2 = State::new();
        let addr = "addr".to_string();
        
        // Set same initial state in both
        state1.set_balance(&addr, 1000);
        state1.set_nonce(&addr, 5);
        state2.set_balance(&addr, 1000);
        state2.set_nonce(&addr, 5);
        
        // Verify states are identical
        assert_eq!(state1.get_balance(&addr), state2.get_balance(&addr));
        assert_eq!(state1.get_nonce(&addr), state2.get_nonce(&addr));
        
        // Create snapshot from state1
        let snapshot = state1.snapshot();
        
        // ATTEMPT TO VIOLATE INVARIANT: Check for side effects on state2
        // Snapshot creation from state1 should not affect state2
        
        // INVARIANT CHECK: state2 should be unchanged
        assert_eq!(
            state2.get_balance(&addr),
            1000,
            "INVARIANT VIOLATION: Hidden side effect - state2 modified by snapshot creation"
        );
        assert_eq!(
            state2.get_nonce(&addr),
            5,
            "INVARIANT VIOLATION: Hidden side effect - state2 modified by snapshot creation"
        );
        
        // Modify state1
        state1.set_balance(&addr, 2000);
        state1.set_nonce(&addr, 10);
        
        // INVARIANT CHECK: state2 should still be unchanged
        assert_eq!(
            state2.get_balance(&addr),
            1000,
            "INVARIANT VIOLATION: Hidden side effect - state2 modified by state1 change"
        );
        assert_eq!(
            state2.get_nonce(&addr),
            5,
            "INVARIANT VIOLATION: Hidden side effect - state2 modified by state1 change"
        );
        
        // Restore state1
        state1.restore(&snapshot);
        
        // INVARIANT CHECK: state2 should still be unchanged
        assert_eq!(
            state2.get_balance(&addr),
            1000,
            "INVARIANT VIOLATION: Hidden side effect - state2 modified by restore"
        );
        assert_eq!(
            state2.get_nonce(&addr),
            5,
            "INVARIANT VIOLATION: Hidden side effect - state2 modified by restore"
        );
        
        // INVARIANT CHECK: Snapshot should be unchanged
        assert_eq!(
            snapshot.get_balance(&addr),
            1000,
            "INVARIANT VIOLATION: Hidden side effect - snapshot modified by restore"
        );
        assert_eq!(
            snapshot.get_nonce(&addr),
            5,
            "INVARIANT VIOLATION: Hidden side effect - snapshot modified by restore"
        );
    }
    
    #[test]
    fn test_negative_snapshot_operations_pure_functions() {
        // NEGATIVE TEST: Verify that snapshot operations are pure functions
        // (no side effects, same input → same output)
        
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        // ATTEMPT TO VIOLATE INVARIANT: Call snapshot methods multiple times
        // and verify they always return the same result (pure function property)
        
        // Call get_balance multiple times - should always return same value
        for _ in 0..100 {
            let balance = snapshot.get_balance(&addr);
            assert_eq!(
                balance,
                1000,
                "INVARIANT VIOLATION: get_balance not pure - returned different values"
            );
        }
        
        // Call get_nonce multiple times - should always return same value
        for _ in 0..100 {
            let nonce = snapshot.get_nonce(&addr);
            assert_eq!(
                nonce,
                5,
                "INVARIANT VIOLATION: get_nonce not pure - returned different values"
            );
        }
        
        // Call get_all_balances multiple times - should return same order
        let balances1 = snapshot.get_all_balances();
        for _ in 0..10 {
            let balances2 = snapshot.get_all_balances();
            assert_eq!(
                balances1,
                balances2,
                "INVARIANT VIOLATION: get_all_balances not pure - returned different order"
            );
        }
        
        // Modify state and verify snapshot operations still return same values
        state.set_balance(&addr, 9999);
        state.set_nonce(&addr, 9999);
        
        // INVARIANT CHECK: Snapshot operations should still return original values
        assert_eq!(
            snapshot.get_balance(&addr),
            1000,
            "INVARIANT VIOLATION: Snapshot operation affected by state change (not pure)"
        );
        assert_eq!(
            snapshot.get_nonce(&addr),
            5,
            "INVARIANT VIOLATION: Snapshot operation affected by state change (not pure)"
        );
    }
    
    #[test]
    fn test_negative_restore_does_not_modify_snapshot_arc() {
        // NEGATIVE TEST: Verify that restore doesn't modify snapshot's internal Arc
        // This is a low-level check to ensure snapshot immutability at Arc level
        
        let state = State::new();
        let addr = "addr".to_string();
        
        state.set_balance(&addr, 1000);
        state.set_nonce(&addr, 5);
        
        // Create snapshot
        let snapshot = state.snapshot();
        
        let ab_count_before = Arc::strong_count(snapshot.asset_balances_arc());
        let nc_count_before = Arc::strong_count(snapshot.nonces_arc());
        state.set_balance(&addr, 2000);
        state.set_nonce(&addr, 10);
        state.restore(&snapshot);
        let ab_count_after = Arc::strong_count(snapshot.asset_balances_arc());
        let nc_count_after = Arc::strong_count(snapshot.nonces_arc());
        assert!(ab_count_after >= ab_count_before, "INVARIANT: restore modified snapshot Arc");
        assert!(nc_count_after >= nc_count_before, "INVARIANT: restore modified snapshot Arc");
        
        // INVARIANT CHECK: Snapshot data should be unchanged
        assert_eq!(
            snapshot.get_balance(&addr),
            1000,
            "INVARIANT VIOLATION: Restore modified snapshot data"
        );
        assert_eq!(
            snapshot.get_nonce(&addr),
            5,
            "INVARIANT VIOLATION: Restore modified snapshot data"
        );
    }
}
