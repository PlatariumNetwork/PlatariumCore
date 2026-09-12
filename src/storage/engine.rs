//! StorageEngine trait — durable apply of StateDiff account post-images.
//!
//! RocksDB types stay behind adapters in this module; kernel must not import rocks.rs.

use crate::core::asset::Asset;
use crate::core::kernel::state_diff::AccountPostImage;
use crate::core::state::State;
use crate::core::state_file::{load_state_file, save_state_file};
use crate::error::{PlatariumError, Result};
use crate::storage::cache::open_cached;
use crate::storage::commit::{account_rmw_preserve_tokens_xp, AccountRecord};
use crate::storage::query::get_account;
use crate::storage::schema::{key_account, key_escrow, KEY_META_ESCROWS, PREFIX_ESCROW};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Map a StateDiff [`AccountPostImage`] into a Rocks [`AccountRecord`].
///
/// Copies `token_balances` into `tokens` and derives `xp` from the canonical
/// `Token:XP` entry (default `"0"` when absent).
pub fn account_record_from_post_image(a: &AccountPostImage) -> AccountRecord {
    let xp = a
        .token_balances
        .get(&Asset::xp().as_canonical())
        .cloned()
        .unwrap_or_else(|| "0".into());
    AccountRecord {
        address: a.address.clone(),
        balance: a.plp_balance.clone(),
        uplp_balance: a.uplp_balance.clone(),
        nonce: a.nonce,
        tokens: a.token_balances.clone(),
        xp,
    }
}

/// Abstract durable account store for CommitEngine.
pub trait StorageEngine {
    fn begin(&mut self) -> Result<()>;
    fn apply_accounts(&mut self, accounts: &[AccountPostImage]) -> Result<()>;
    /// Replace escrow records when StateDiff includes `escrows_json`.
    fn apply_escrows(&mut self, escrows_json: &[String]) -> Result<()>;
    fn commit_atomic(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
    fn get_account(&self, address: &str) -> Option<AccountPostImage>;
}

/// In-memory engine for tests.
#[derive(Debug, Default)]
pub struct InMemoryStorageEngine {
    committed: HashMap<String, AccountPostImage>,
    committed_escrows: Vec<String>,
    staging: Option<HashMap<String, AccountPostImage>>,
    staging_escrows: Option<Vec<String>>,
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
        let mut escrows: Vec<_> = snap.contact_escrows_arc().values().cloned().collect();
        escrows.sort_by(|a, b| a.escrow_id.cmp(&b.escrow_id));
        let committed_escrows: Vec<String> = escrows
            .iter()
            .map(|e| {
                serde_json::to_string(e).expect("Escrow must serialize for InMemoryStorageEngine")
            })
            .collect();
        Self {
            committed,
            committed_escrows,
            staging: None,
            staging_escrows: None,
        }
    }

    /// Escrow JSON post-images last committed (sorted by escrow_id at write time).
    pub fn escrows_json(&self) -> &[String] {
        &self.committed_escrows
    }
}

impl StorageEngine for InMemoryStorageEngine {
    fn begin(&mut self) -> Result<()> {
        self.staging = Some(self.committed.clone());
        self.staging_escrows = Some(self.committed_escrows.clone());
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

    fn apply_escrows(&mut self, escrows_json: &[String]) -> Result<()> {
        let staging = self
            .staging_escrows
            .as_mut()
            .ok_or_else(|| PlatariumError::State("StorageEngine.begin not called".into()))?;
        // Validate JSON before replacing (same contract as StateFile / Rocks).
        for js in escrows_json {
            let _: crate::modules::escrow::Escrow = serde_json::from_str(js).map_err(|e| {
                PlatariumError::State(format!("invalid escrow json in StateDiff: {}", e))
            })?;
        }
        *staging = escrows_json.to_vec();
        Ok(())
    }

    fn commit_atomic(&mut self) -> Result<()> {
        let staging = self
            .staging
            .take()
            .ok_or_else(|| PlatariumError::State("StorageEngine.begin not called".into()))?;
        let staging_escrows = self
            .staging_escrows
            .take()
            .ok_or_else(|| PlatariumError::State("StorageEngine.begin not called".into()))?;
        self.committed = staging;
        self.committed_escrows = staging_escrows;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        self.staging = None;
        self.staging_escrows = None;
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

    fn apply_escrows(&mut self, escrows_json: &[String]) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        apply_escrows_to_state(&self.state, escrows_json)
    }

    fn commit_atomic(&mut self) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        crate::core::failpoints::hit(crate::core::failpoints::FP_JSON_STAGING_COMMIT)?;
        match save_state_file(&self.path, &self.state) {
            Ok(()) => {
                self.begun = false;
                self.snapshot_before = None;
                Ok(())
            }
            Err(e) => {
                let _ = self.rollback();
                Err(e)
            }
        }
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
    /// When set, replace-all escrow keys on commit (StateDiff `escrows_json`).
    staging_escrows: Option<Vec<String>>,
    begun: bool,
}

impl RocksAccountStorageEngine {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db_path: db_path.as_ref().to_path_buf(),
            staging: Vec::new(),
            staging_escrows: None,
            begun: false,
        })
    }

    /// Load escrow JSON post-images last written by `apply_escrows` / commit.
    pub fn load_escrows_json(&self) -> Result<Vec<String>> {
        let store = open_cached(&self.db_path)?;
        match store.get(KEY_META_ESCROWS)? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                PlatariumError::State(format!("decode meta/escrows: {}", e))
            }),
            None => Ok(Vec::new()),
        }
    }
}

impl StorageEngine for RocksAccountStorageEngine {
    fn begin(&mut self) -> Result<()> {
        self.staging.clear();
        self.staging_escrows = None;
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

    fn apply_escrows(&mut self, escrows_json: &[String]) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        for js in escrows_json {
            let _: crate::modules::escrow::Escrow = serde_json::from_str(js).map_err(|e| {
                PlatariumError::State(format!("invalid escrow json in StateDiff: {}", e))
            })?;
        }
        self.staging_escrows = Some(escrows_json.to_vec());
        Ok(())
    }

    fn commit_atomic(&mut self) -> Result<()> {
        if !self.begun {
            return Err(PlatariumError::State("StorageEngine.begin not called".into()));
        }
        if let Err(e) = crate::core::failpoints::hit(crate::core::failpoints::FP_ROCKS_COMMIT_ATOMIC)
        {
            let _ = self.rollback();
            return Err(e);
        }
        let store = open_cached(&self.db_path)?;
        let mut batch = rocksdb::WriteBatch::default();
        for a in &self.staging {
            let mut rec = account_record_from_post_image(a);
            // Issue #38: balance-only post-images (empty token_balances) must not
            // silently zero durable tokens/xp already on disk.
            if a.token_balances.is_empty() {
                if let Ok(Some(existing)) = get_account(store.as_ref(), &a.address) {
                    rec = account_rmw_preserve_tokens_xp(
                        &existing,
                        rec.balance,
                        rec.uplp_balance,
                        rec.nonce,
                    );
                }
            }
            let bytes = serde_json::to_vec(&rec)
                .map_err(|e| PlatariumError::State(format!("encode account: {}", e)))?;
            batch.put(key_account(&a.address), bytes);
        }
        if let Some(ref escrows) = self.staging_escrows {
            // Replace-all: drop previous per-id keys, then write post-images + meta index.
            let iter = store.db().prefix_iterator(PREFIX_ESCROW);
            for item in iter {
                let (key, _) = item.map_err(|e| {
                    PlatariumError::State(format!("rocksdb escrow prefix iter: {}", e))
                })?;
                if !key.starts_with(PREFIX_ESCROW) {
                    break;
                }
                batch.delete(key);
            }
            for js in escrows {
                let e: crate::modules::escrow::Escrow = serde_json::from_str(js).map_err(|err| {
                    PlatariumError::State(format!("invalid escrow json in StateDiff: {}", err))
                })?;
                batch.put(key_escrow(&e.escrow_id), js.as_bytes());
            }
            let meta = serde_json::to_vec(escrows)
                .map_err(|e| PlatariumError::State(format!("encode meta/escrows: {}", e)))?;
            batch.put(KEY_META_ESCROWS, meta);
        }
        if let Err(e) = store.write_batch(batch) {
            let _ = self.rollback();
            return Err(e);
        }
        self.staging.clear();
        self.staging_escrows = None;
        self.begun = false;
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        self.staging.clear();
        self.staging_escrows = None;
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
            token_balances: rec.tokens,
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

fn apply_escrows_to_state(state: &State, escrows_json: &[String]) -> Result<()> {
    let mut escrows = Vec::with_capacity(escrows_json.len());
    for js in escrows_json {
        let e: crate::modules::escrow::Escrow = serde_json::from_str(js).map_err(|e| {
            PlatariumError::State(format!("invalid escrow json in StateDiff: {}", e))
        })?;
        escrows.push(e);
    }
    state.replace_all_escrows(escrows);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kernel::state_diff::AccountPostImage;
    use crate::storage::schema::key_account;
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

    /// Issue #34: StateDiff token_balances/xp map into Rocks AccountRecord WriteBatch.
    #[test]
    fn state_diff_tokens_xp_reach_rocks_write_batch() {
        use crate::core::kernel::commit_engine::commit_state_diff;
        use crate::core::kernel::state_diff::{StateDiff, STATE_DIFF_SCHEMA_VERSION};
        use crate::storage::cache::evict_cached;
        use crate::storage::commit::AccountRecord;
        use std::collections::BTreeMap;

        let mut tokens = BTreeMap::new();
        tokens.insert(Asset::xp().as_canonical(), "150".into());
        tokens.insert("Token:USDT".into(), "42".into());
        let image = AccountPostImage {
            address: "PxTok".into(),
            plp_balance: "1000".into(),
            uplp_balance: "3".into(),
            nonce: 5,
            token_balances: tokens.clone(),
        };
        let mapped = account_record_from_post_image(&image);
        assert_eq!(mapped.balance, "1000");
        assert_eq!(mapped.nonce, 5);
        assert_eq!(mapped.xp, "150");
        assert_eq!(mapped.tokens.get("Token:USDT").map(String::as_str), Some("42"));
        assert_eq!(
            mapped.tokens.get(&Asset::xp().as_canonical()).map(String::as_str),
            Some("150")
        );

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let mut eng = RocksAccountStorageEngine::open(&db_path).unwrap();
        let diff = StateDiff {
            schema_version: STATE_DIFF_SCHEMA_VERSION,
            batch_id: "tok-xp".into(),
            receipts: vec![],
            accounts: vec![image],
            pre_state_root: None,
            post_state_root: "root-tok".into(),
            escrows_json: None,
        };
        let res = commit_state_diff(&mut eng, &diff).unwrap();
        assert!(res.ok, "{:?}", res.error);

        // Prove encoded AccountRecord bytes (WriteBatch payload) carry tokens/xp.
        let store = crate::storage::cache::open_cached(&db_path).unwrap();
        let raw = store
            .get(&key_account("PxTok"))
            .unwrap()
            .expect("account key written by WriteBatch");
        let rec: AccountRecord = serde_json::from_slice(&raw).unwrap();
        assert_eq!(rec.xp, "150");
        assert_eq!(rec.tokens.get("Token:USDT").map(String::as_str), Some("42"));
        assert_eq!(rec.balance, "1000");
        assert_eq!(rec.nonce, 5);

        let got = eng.get_account("PxTok").unwrap();
        assert_eq!(
            got.token_balances.get(&Asset::xp().as_canonical()).map(String::as_str),
            Some("150")
        );
        evict_cached(&db_path);
    }

    /// Issue #37: golden restart — tokens=100 xp=250 survive StateDiff → Rocks → reopen.
    #[test]
    fn golden_restart_tokens_100_xp_250() {
        use crate::core::kernel::commit_engine::commit_state_diff;
        use crate::core::kernel::state_diff::{StateDiff, STATE_DIFF_SCHEMA_VERSION};
        use crate::core::state_file::{init_state_file, load_state_file, save_state_file};
        use crate::storage::cache::evict_cached;
        use crate::storage::commit::AccountRecord;
        use std::collections::BTreeMap;

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        init_state_file(&state_path).unwrap();
        let state = load_state_file(&state_path).unwrap();
        state.set_balance(&"PxGolden".to_string(), 5000);
        state.set_uplp_balance(&"PxGolden".to_string(), 1);
        state.set_nonce(&"PxGolden".to_string(), 0);
        state.set_asset_balance(&"PxGolden".to_string(), &Asset::Token("USDT".into()), 100);
        state.set_asset_balance(&"PxGolden".to_string(), &Asset::xp(), 250);
        save_state_file(&state_path, &state).unwrap();

        let mut tokens = BTreeMap::new();
        tokens.insert("Token:USDT".into(), "100".into());
        tokens.insert(Asset::xp().as_canonical(), "250".into());
        let image = AccountPostImage {
            address: "PxGolden".into(),
            plp_balance: "5000".into(),
            uplp_balance: "1".into(),
            nonce: 0,
            token_balances: tokens,
        };
        assert_eq!(account_record_from_post_image(&image).xp, "250");

        let db_path = dir.path().join("rocks");
        {
            let mut eng = RocksAccountStorageEngine::open(&db_path).unwrap();
            let diff = StateDiff {
                schema_version: STATE_DIFF_SCHEMA_VERSION,
                batch_id: "golden-100-250".into(),
                receipts: vec![],
                accounts: vec![image],
                pre_state_root: None,
                post_state_root: "root-golden".into(),
                escrows_json: None,
            };
            let res = commit_state_diff(&mut eng, &diff).unwrap();
            assert!(res.ok, "{:?}", res.error);
        }
        evict_cached(&db_path);

        // Restart: reopen Rocks; values must equal 100 / 250 (not silently dropped).
        let eng = RocksAccountStorageEngine::open(&db_path).unwrap();
        let got = eng.get_account("PxGolden").unwrap();
        assert_eq!(
            got.token_balances.get("Token:USDT").map(String::as_str),
            Some("100"),
            "tokens silently dropped by WriteBatch/restart"
        );
        assert_eq!(
            got.token_balances
                .get(&Asset::xp().as_canonical())
                .map(String::as_str),
            Some("250"),
            "xp silently dropped by WriteBatch/restart"
        );
        let store = crate::storage::cache::open_cached(&db_path).unwrap();
        let raw = store.get(&key_account("PxGolden")).unwrap().unwrap();
        let rec: AccountRecord = serde_json::from_slice(&raw).unwrap();
        assert_eq!(rec.xp, "250");
        assert_eq!(rec.tokens.get("Token:USDT").map(String::as_str), Some("100"));
        evict_cached(&db_path);
    }

    /// Issue #38: balance-only StateDiff update must retain prior tokens/xp on Rocks.
    #[test]
    fn balance_only_update_preserves_tokens_xp_on_rocks() {
        use crate::core::kernel::commit_engine::commit_state_diff;
        use crate::core::kernel::state_diff::{StateDiff, STATE_DIFF_SCHEMA_VERSION};
        use crate::storage::cache::evict_cached;
        use std::collections::BTreeMap;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let mut eng = RocksAccountStorageEngine::open(&db_path).unwrap();

        let mut tokens = BTreeMap::new();
        tokens.insert(Asset::xp().as_canonical(), "250".into());
        tokens.insert("Token:USDT".into(), "99".into());
        let seed = StateDiff {
            schema_version: STATE_DIFF_SCHEMA_VERSION,
            batch_id: "seed".into(),
            receipts: vec![],
            accounts: vec![AccountPostImage {
                address: "PxBal".into(),
                plp_balance: "1000".into(),
                uplp_balance: "10".into(),
                nonce: 3,
                token_balances: tokens.clone(),
            }],
            pre_state_root: None,
            post_state_root: "r1".into(),
            escrows_json: None,
        };
        assert!(commit_state_diff(&mut eng, &seed).unwrap().ok);

        // Balance-only post-image (empty token_balances) — must not zero tokens/xp.
        let bal_only = StateDiff {
            schema_version: STATE_DIFF_SCHEMA_VERSION,
            batch_id: "bal-only".into(),
            receipts: vec![],
            accounts: vec![AccountPostImage {
                address: "PxBal".into(),
                plp_balance: "900".into(),
                uplp_balance: "9".into(),
                nonce: 4,
                token_balances: BTreeMap::new(),
            }],
            pre_state_root: None,
            post_state_root: "r2".into(),
            escrows_json: None,
        };
        assert!(commit_state_diff(&mut eng, &bal_only).unwrap().ok);
        let got = eng.get_account("PxBal").unwrap();
        assert_eq!(got.plp_balance, "900");
        assert_eq!(got.uplp_balance, "9");
        assert_eq!(got.nonce, 4);
        assert_eq!(
            got.token_balances.get(&Asset::xp().as_canonical()).map(String::as_str),
            Some("250"),
            "xp zeroed by balance-only update"
        );
        assert_eq!(
            got.token_balances.get("Token:USDT").map(String::as_str),
            Some("99"),
            "tokens zeroed by balance-only update"
        );
        evict_cached(&db_path);
    }

}
