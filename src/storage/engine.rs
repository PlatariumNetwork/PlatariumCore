//! StorageEngine trait — durable apply of StateDiff account post-images.
//!
//! RocksDB types stay behind adapters in this module; kernel must not import rocks.rs.

use crate::core::asset::Asset;
use crate::core::kernel::state_diff::AccountPostImage;
use crate::core::state::State;
use crate::core::state_file::{load_state_file, save_state_file};
use crate::error::{PlatariumError, Result};
use crate::storage::cache::open_cached;
use crate::storage::commit::AccountRecord;
use crate::storage::query::get_account;
use crate::storage::schema::key_account;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Abstract durable account store for CommitEngine.
pub trait StorageEngine {
    fn begin(&mut self) -> Result<()>;
    fn apply_accounts(&mut self, accounts: &[AccountPostImage]) -> Result<()>;
    fn commit_atomic(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
    fn get_account(&self, address: &str) -> Option<AccountPostImage>;
}

/// In-memory engine for tests.
#[derive(Debug, Default)]
pub struct InMemoryStorageEngine {
    committed: HashMap<String, AccountPostImage>,
    staging: Option<HashMap<String, AccountPostImage>>,
}

impl InMemoryStorageEngine {
    pub fn from_state(state: &State) -> Self {
        let snap = state.snapshot();
        let mut committed = HashMap::new();
        let mut addrs = std::collections::BTreeSet::new();
        for ((a, _), _) in snap.asset_balances_arc().iter() {
            addrs.insert(a.clone());
        }
        for (a, _) in snap.uplp_balances_arc().iter() {
            addrs.insert(a.clone());
        }
        for (a, _) in snap.nonces_arc().iter() {
            addrs.insert(a.clone());
        }
        for addr in addrs {
            committed.insert(addr.clone(), account_from_state(state, &addr));
        }
        Self {
            committed,
            staging: None,
        }
    }
}

impl StorageEngine for InMemoryStorageEngine {
    fn begin(&mut self) -> Result<()> {
        self.staging = Some(self.committed.clone());
        Ok(())
    }

    fn apply_accounts(&mut self, accounts: &[AccountPostImage]) -> Result<()> {
        let staging = self
            .staging
            .as_mut()
            .ok_or_else(|| PlatariumError::State("StorageEngine.begin not called".into()))?;
        for a in accounts {
            staging.insert(a.address.clone(), a.clone());
        }
        Ok(())
    }

    fn commit_atomic(&mut self) -> Result<()> {
        let staging = self
            .staging
            .take()
            .ok_or_else(|| PlatariumError::State("StorageEngine.begin not called".into()))?;
        self.committed = staging;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        self.staging = None;
        Ok(())
    }

    fn get_account(&self, address: &str) -> Option<AccountPostImage> {
        self.committed.get(address).cloned()
    }
}

/// State-file backed engine (JSON ledger).
pub struct StateFileStorageEngine {
    path: PathBuf,
    state: State,
    begun: bool,
    snapshot_before: Option<crate::core::state::StateSnapshot>,
}

impl StateFileStorageEngine {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            load_state_file(&path)?
        } else {
            let s = State::new();
            save_state_file(&path, &s)?;
            s
        };
        Ok(Self {
            path,
            state,
            begun: false,
            snapshot_before: None,
        })
    }

    pub fn state(&self) -> &State {
        &self.state
    }
}

impl StorageEngine for StateFileStorageEngine {
    fn begin(&mut self) -> Result<()> {
        self.snapshot_before = Some(self.state.snapshot());
        self.begun = true;
        Ok(())
    }

    fn apply_accounts(&mut self, accounts: &[AccountPostImage]) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        apply_post_images_to_state(&self.state, accounts)?;
        Ok(())
    }

    fn commit_atomic(&mut self) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        save_state_file(&self.path, &self.state)?;
        self.begun = false;
        self.snapshot_before = None;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        if let Some(snap) = self.snapshot_before.take() {
            self.state.restore(&snap);
        }
        self.begun = false;
        Ok(())
    }

    fn get_account(&self, address: &str) -> Option<AccountPostImage> {
        Some(account_from_state(&self.state, address))
    }
}

/// Applies PLP/μPLP/nonce post-images into RocksDB account keys (WriteBatch).
pub struct RocksAccountStorageEngine {
    db_path: PathBuf,
    staging: Vec<AccountPostImage>,
    begun: bool,
}

impl RocksAccountStorageEngine {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db_path: db_path.as_ref().to_path_buf(),
            staging: Vec::new(),
            begun: false,
        })
    }
}

impl StorageEngine for RocksAccountStorageEngine {
    fn begin(&mut self) -> Result<()> {
        self.staging.clear();
        self.begun = true;
        Ok(())
    }

    fn apply_accounts(&mut self, accounts: &[AccountPostImage]) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        self.staging.extend(accounts.iter().cloned());
        Ok(())
    }

    fn commit_atomic(&mut self) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        let store = open_cached(&self.db_path)?;
        let mut batch = rocksdb::WriteBatch::default();
        for a in &self.staging {
            let rec = AccountRecord {
                address: a.address.clone(),
                balance: a.plp_balance.clone(),
                uplp_balance: a.uplp_balance.clone(),
                nonce: a.nonce,
            };
            let bytes = serde_json::to_vec(&rec)
                .map_err(|e| PlatariumError::State(format!("encode account: {}", e)))?;
            batch.put(key_account(&a.address), bytes);
        }
        store.write_batch(batch)?;
        self.staging.clear();
        self.begun = false;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        self.staging.clear();
        self.begun = false;
        Ok(())
    }

    fn get_account(&self, address: &str) -> Option<AccountPostImage> {
        let store = open_cached(&self.db_path).ok()?;
        let rec = get_account(store.as_ref(), address).ok()??;
        Some(AccountPostImage {
            address: rec.address,
            plp_balance: rec.balance,
            uplp_balance: rec.uplp_balance,
            nonce: rec.nonce,
            token_balances: Default::default(),
        })
    }
}

fn account_from_state(state: &State, addr: &str) -> AccountPostImage {
    let snap = state.snapshot();
    let mut token_balances = std::collections::BTreeMap::new();
    let plp = Asset::PLP.as_canonical();
    for ((a, asset), bal) in snap.asset_balances_arc().iter() {
        if a == addr && asset != &plp {
            token_balances.insert(asset.clone(), bal.to_string());
        }
    }
    AccountPostImage {
        address: addr.to_string(),
        plp_balance: state.get_balance(&addr.to_string()).to_string(),
        uplp_balance: state.get_uplp_balance(&addr.to_string()).to_string(),
        nonce: state.get_nonce(&addr.to_string()),
        token_balances,
    }
}

fn apply_post_images_to_state(state: &State, accounts: &[AccountPostImage]) -> Result<()> {
    for a in accounts {
        let plp: u128 = a
            .plp_balance
            .parse()
            .map_err(|e| PlatariumError::State(format!("bad plp_balance: {}", e)))?;
        let uplp: u128 = a
            .uplp_balance
            .parse()
            .map_err(|e| PlatariumError::State(format!("bad uplp_balance: {}", e)))?;
        state.set_balance(&a.address, plp);
        state.set_uplp_balance(&a.address, uplp);
        state.set_nonce(&a.address, a.nonce);
        for (sym, bal_s) in &a.token_balances {
            let bal: u128 = bal_s
                .parse()
                .map_err(|e| PlatariumError::State(format!("bad token bal: {}", e)))?;
            state.set_asset_balance(&a.address, &Asset::Token(sym.clone()), bal);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kernel::state_diff::AccountPostImage;
    use tempfile::TempDir;

    #[test]
    fn state_file_engine_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");
        let mut eng = StateFileStorageEngine::open(&path).unwrap();
        eng.begin().unwrap();
        eng.apply_accounts(&[AccountPostImage {
            address: "PxA".into(),
            plp_balance: "100".into(),
            uplp_balance: "5".into(),
            nonce: 2,
            token_balances: Default::default(),
        }])
        .unwrap();
        eng.commit_atomic().unwrap();
        let got = eng.get_account("PxA").unwrap();
        assert_eq!(got.plp_balance, "100");
        assert_eq!(got.nonce, 2);
    }

    #[test]
    fn crash_before_commit_rolls_back_state_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");
        {
            let mut eng = StateFileStorageEngine::open(&path).unwrap();
            eng.begin().unwrap();
            eng.apply_accounts(&[AccountPostImage {
                address: "PxA".into(),
                plp_balance: "1".into(),
                uplp_balance: "0".into(),
                nonce: 0,
                token_balances: Default::default(),
            }])
            .unwrap();
            eng.commit_atomic().unwrap();
        }
        let mut eng = StateFileStorageEngine::open(&path).unwrap();
        eng.begin().unwrap();
        eng.apply_accounts(&[AccountPostImage {
            address: "PxA".into(),
            plp_balance: "999".into(),
            uplp_balance: "0".into(),
            nonce: 9,
            token_balances: Default::default(),
        }])
        .unwrap();
        eng.rollback().unwrap();
        let got = eng.get_account("PxA").unwrap();
        assert_eq!(got.plp_balance, "1");
        assert_eq!(got.nonce, 0);
    }
}
