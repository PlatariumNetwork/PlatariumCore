//! Crash-injectable finalize persist path (issues #55–#59).
//!
//! Ordered phases with named failpoints:
//! 1. `before_state_write` → JSON staging write → `after_state_write`
//! 2. `before_rocks_write` → `before_commit` → atomic Rocks `commit_block` → `after_rocks_write`
//!
//! Rocks head is the sole COMMITTED tip (`commit_block` WriteBatch includes `meta/head`).
//! JSON state_file is staging/recovery cache and must not advance the canonical tip alone.
//!
//! Contract for `after_rocks_write`: commit marker is **not** separate from the Rocks
//! payload (single atomic batch). Tripping after a successful `commit_block` means the
//! process dies post-durability → restart expects the **NEW** tip (never mixed height/hash).

use crate::core::failpoints::{
    self, FP_AFTER_ROCKS_WRITE, FP_AFTER_STATE_WRITE, FP_BEFORE_COMMIT, FP_BEFORE_ROCKS_WRITE,
    FP_BEFORE_STATE_WRITE,
};
use crate::core::state::State;
use crate::core::state_file::save_state_file;
use crate::error::Result;
use crate::storage::commit::{commit_block, BlockCommit};
use crate::storage::query::{get_block, get_head, get_state_root};
use crate::storage::rocks::RocksStore;
use std::path::Path;

/// Tip snapshot used by restart invariant checks (height + hash pairing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipPair {
    pub height: u64,
    pub block_hash: String,
    pub state_root: String,
}

impl TipPair {
    /// Genesis / empty store tip.
    pub fn genesis() -> Self {
        Self {
            height: 0,
            block_hash: String::new(),
            state_root: String::new(),
        }
    }
}

/// Read canonical tip from Rocks: head height paired with block hash + state_root.
pub fn read_canonical_tip(store: &RocksStore) -> Result<TipPair> {
    let height = get_head(store)?;
    if height == 0 {
        return Ok(TipPair::genesis());
    }
    let block = get_block(store, height)?.ok_or_else(|| {
        crate::error::PlatariumError::State(format!(
            "canonical tip missing block at height {height}"
        ))
    })?;
    let state_root = get_state_root(store, height)?.ok_or_else(|| {
        crate::error::PlatariumError::State(format!(
            "canonical tip missing state_root at height {height}"
        ))
    })?;
    Ok(TipPair {
        height,
        block_hash: block.block_hash,
        state_root,
    })
}

/// Assert `canonical_height == state_height` and hash pairing at the Rocks tip.
///
/// - `canonical_height` / `canonical_hash`: `meta/head` and `block.block_hash`
/// - `state_height` / `state_hash`: `block.height` and durable `state_root` meta
pub fn assert_height_hash_invariant(store: &RocksStore) -> Result<TipPair> {
    let tip = read_canonical_tip(store)?;
    if tip.height == 0 {
        return Ok(tip);
    }
    let block = get_block(store, tip.height)?.ok_or_else(|| {
        crate::error::PlatariumError::State(format!(
            "height/hash invariant: missing block at canonical_height={}",
            tip.height
        ))
    })?;
    let state_height = block.height;
    let canonical_height = tip.height;
    assert_eq!(
        canonical_height, state_height,
        "height/hash invariant: canonical_height={canonical_height} != state_height={state_height}"
    );
    let meta_root = get_state_root(store, tip.height)?.ok_or_else(|| {
        crate::error::PlatariumError::State(format!(
            "height/hash invariant: missing state_root at height {}",
            tip.height
        ))
    })?;
    assert_eq!(
        block.state_root, meta_root,
        "height/hash invariant: block.state_root={} != meta state_root={} at height {}",
        block.state_root, meta_root, tip.height
    );
    assert_eq!(
        tip.block_hash, block.block_hash,
        "height/hash invariant: tip.block_hash != block.block_hash at height {}",
        tip.height
    );
    assert_eq!(
        tip.state_root, meta_root,
        "height/hash invariant: tip.state_root != meta state_root at height {}",
        tip.height
    );
    Ok(tip)
}

/// Persist JSON staging then atomic Rocks commit, with named failpoints.
///
/// On success (and after `after_rocks_write` if armed only post-write), Rocks head
/// advances with paired height/hash. Failures before `commit_block` leave the prior
/// COMMITTED tip unchanged. JSON-only success is staging, not COMMITTED.
pub fn persist_staging_then_rocks(
    state_path: &Path,
    staging_state: &State,
    store: &RocksStore,
    commit: &BlockCommit,
) -> Result<()> {
    failpoints::hit(FP_BEFORE_STATE_WRITE)?;
    save_state_file(state_path, staging_state)?;
    failpoints::hit(FP_AFTER_STATE_WRITE)?;

    failpoints::hit(FP_BEFORE_ROCKS_WRITE)?;
    // before_commit: last gate before the atomic Rocks WriteBatch (includes head marker).
    failpoints::hit(FP_BEFORE_COMMIT)?;
    commit_block(store, commit)?;
    // after_rocks_write: payload + head already durable (marker not separate) → NEW tip.
    failpoints::hit(FP_AFTER_ROCKS_WRITE)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::failpoints::{
        self, FP_AFTER_ROCKS_WRITE, FP_AFTER_STATE_WRITE, FP_BEFORE_COMMIT, FP_BEFORE_ROCKS_WRITE,
        FP_BEFORE_STATE_WRITE,
    };
    use crate::core::state::State;
    use crate::core::state_file::{load_state_file, save_state_file};
    use crate::storage::cache::evict_cached;
    use crate::storage::commit::{
        commit_block, AccountRecord, BlockCommit, BlockRecordStored, ReceiptRecord,
    };
    use crate::storage::query::get_account;
    use crate::storage::rocks::RocksStore;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    /// Failpoint arming is process-global; serialize these tests.
    fn fp_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::core::failpoints::test_lock()
    }

    fn sample_commit(
        height: u64,
        state_root: &str,
        block_hash: &str,
        alice_bal: &str,
    ) -> BlockCommit {
        BlockCommit {
            block: BlockRecordStored {
                height,
                previous_hash: if height <= 1 {
                    "0".into()
                } else {
                    format!("bh{}", height - 1)
                },
                timestamp: height as i64,
                tx_hashes: vec![format!("tx{height}")],
                merkle_root: format!("m{height}"),
                state_root: state_root.into(),
                block_hash: block_hash.into(),
                producer_id: "n1".into(),
            },
            tx_jsons: vec![format!(
                r#"{{"hash":"tx{height}","from":"PxA","to":"PxB","asset":"PLP","amount":10,"fee_uplp":1,"nonce":{n},"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}}"#,
                height = height,
                n = height - 1
            )],
            accounts: vec![
                AccountRecord {
                    address: "PxA".into(),
                    balance: alice_bal.into(),
                    uplp_balance: "0".into(),
                    nonce: height,
                    tokens: BTreeMap::new(),
                    xp: "0".into(),
                },
                AccountRecord {
                    address: "PxB".into(),
                    balance: "10".into(),
                    uplp_balance: "0".into(),
                    nonce: 0,
                    tokens: BTreeMap::new(),
                    xp: "0".into(),
                },
            ],
            receipts: vec![ReceiptRecord {
                tx_hash: format!("tx{height}"),
                status: "ok".into(),
                fee_uplp: 1,
                block_height: height,
            }],
            state_root: state_root.into(),
        }
    }

    fn staging_state_for(alice_bal: u128, nonce: u64) -> State {
        let s = State::new();
        s.set_balance(&"PxA".to_string(), alice_bal);
        s.set_nonce(&"PxA".to_string(), nonce);
        s.set_balance(&"PxB".to_string(), 10);
        s
    }

    fn seed_height1(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf, TipPair) {
        let state_path = dir.path().join("state.json");
        let db_path = dir.path().join("rocks");
        let store = RocksStore::open(&db_path).unwrap();
        let c1 = sample_commit(1, "root1", "bh1", "90");
        commit_block(&store, &c1).unwrap();
        save_state_file(&state_path, &staging_state_for(90, 1)).unwrap();
        let tip = assert_height_hash_invariant(&store).unwrap();
        assert_eq!(tip.height, 1);
        assert_eq!(tip.block_hash, "bh1");
        assert_eq!(tip.state_root, "root1");
        drop(store);
        evict_cached(&db_path);
        (state_path, db_path, tip)
    }

    fn reopen_assert_tip(db_path: &Path, expected: &TipPair) -> TipPair {
        let store = RocksStore::open(db_path).unwrap();
        let tip = assert_height_hash_invariant(&store).unwrap();
        assert_eq!(
            tip.height, expected.height,
            "after restart: tip height drifted (advanced or rolled back unexpectedly)"
        );
        assert_eq!(
            tip.block_hash, expected.block_hash,
            "after restart: tip block_hash mismatch"
        );
        assert_eq!(
            tip.state_root, expected.state_root,
            "after restart: tip state_root mismatch"
        );
        tip
    }

    /// Issue #55: trip `before_state_write`; restart keeps prior commit tip.
    #[test]
    fn failpoint_before_state_write_restart_keeps_prior_tip() {
        let _guard = fp_lock();
        failpoints::clear_all();
        let dir = TempDir::new().unwrap();
        let (state_path, db_path, prior) = seed_height1(&dir);

        let store = RocksStore::open(&db_path).unwrap();
        let c2 = sample_commit(2, "root2", "bh2", "80");
        let staging = staging_state_for(80, 2);
        failpoints::arm(FP_BEFORE_STATE_WRITE);
        let err = persist_staging_then_rocks(&state_path, &staging, &store, &c2).unwrap_err();
        assert!(
            err.to_string().contains("failpoint:before_state_write"),
            "{err}"
        );
        failpoints::clear_all();
        drop(store);
        evict_cached(&db_path);

        let tip = reopen_assert_tip(&db_path, &prior);
        assert_eq!(tip.height, 1, "no advanced tip");
        let st = load_state_file(&state_path).unwrap();
        assert_eq!(st.get_balance(&"PxA".to_string()), 90);
        evict_cached(&db_path);
    }

    /// Issue #56: trip `after_state_write`; staging may advance, COMMITTED tip does not.
    #[test]
    fn failpoint_after_state_write_restart_staging_not_committed() {
        let _guard = fp_lock();
        failpoints::clear_all();
        let dir = TempDir::new().unwrap();
        let (state_path, db_path, prior) = seed_height1(&dir);

        let store = RocksStore::open(&db_path).unwrap();
        let c2 = sample_commit(2, "root2", "bh2", "80");
        let staging = staging_state_for(80, 2);
        failpoints::arm(FP_AFTER_STATE_WRITE);
        let err = persist_staging_then_rocks(&state_path, &staging, &store, &c2).unwrap_err();
        assert!(
            err.to_string().contains("failpoint:after_state_write"),
            "{err}"
        );
        failpoints::clear_all();
        drop(store);
        evict_cached(&db_path);

        let st = load_state_file(&state_path).unwrap();
        assert_eq!(st.get_balance(&"PxA".to_string()), 80);
        let tip = reopen_assert_tip(&db_path, &prior);
        assert_eq!(tip.height, 1, "partial staging must not advance canonical tip");
        let store = RocksStore::open(&db_path).unwrap();
        let a = get_account(&store, "PxA").unwrap().unwrap();
        assert_eq!(a.balance, "90", "Rocks accounts remain at prior commit");
        evict_cached(&db_path);
    }

    /// Issue #57: trip `before_rocks_write`; Rocks head unchanged from prior commit.
    #[test]
    fn failpoint_before_rocks_write_restart_prior_rocks_only() {
        let _guard = fp_lock();
        failpoints::clear_all();
        let dir = TempDir::new().unwrap();
        let (state_path, db_path, prior) = seed_height1(&dir);

        let store = RocksStore::open(&db_path).unwrap();
        let c2 = sample_commit(2, "root2", "bh2", "80");
        let staging = staging_state_for(80, 2);
        failpoints::arm(FP_BEFORE_ROCKS_WRITE);
        let err = persist_staging_then_rocks(&state_path, &staging, &store, &c2).unwrap_err();
        assert!(
            err.to_string().contains("failpoint:before_rocks_write"),
            "{err}"
        );
        failpoints::clear_all();
        drop(store);
        evict_cached(&db_path);

        let tip = reopen_assert_tip(&db_path, &prior);
        assert_eq!(tip.height, 1);
        let store = RocksStore::open(&db_path).unwrap();
        assert!(
            get_block(&store, 2).unwrap().is_none(),
            "no height-2 block before rocks write"
        );
        let a = get_account(&store, "PxA").unwrap().unwrap();
        assert_eq!(a.balance, "90");
        evict_cached(&db_path);
    }

    /// Issue #58: trip `after_rocks_write` after atomic `commit_block`.
    ///
    /// **Expected side: NEW tip.** Commit marker is not separate from the Rocks
    /// payload (single WriteBatch includes `meta/head`). Crash after durable write
    /// leaves height/hash paired at the new head — never mixed.
    #[test]
    fn failpoint_after_rocks_write_restart_expects_new_tip() {
        let _guard = fp_lock();
        failpoints::clear_all();
        let dir = TempDir::new().unwrap();
        let (state_path, db_path, _prior) = seed_height1(&dir);

        let store = RocksStore::open(&db_path).unwrap();
        let c2 = sample_commit(2, "root2", "bh2", "80");
        let staging = staging_state_for(80, 2);
        failpoints::arm(FP_AFTER_ROCKS_WRITE);
        let err = persist_staging_then_rocks(&state_path, &staging, &store, &c2).unwrap_err();
        assert!(
            err.to_string().contains("failpoint:after_rocks_write"),
            "{err}"
        );
        failpoints::clear_all();
        // Durability already happened before the failpoint.
        assert_eq!(get_head(&store).unwrap(), 2);
        let tip_live = assert_height_hash_invariant(&store).unwrap();
        assert_eq!(tip_live.height, 2);
        assert_eq!(tip_live.block_hash, "bh2");
        assert_eq!(tip_live.state_root, "root2");
        drop(store);
        evict_cached(&db_path);

        let expected = TipPair {
            height: 2,
            block_hash: "bh2".into(),
            state_root: "root2".into(),
        };
        reopen_assert_tip(&db_path, &expected);
        evict_cached(&db_path);
    }

    /// Issue #59: trip `before_commit`; no partial COMMITTED tip exposed.
    #[test]
    fn failpoint_before_commit_restart_no_partial_committed() {
        let _guard = fp_lock();
        failpoints::clear_all();
        let dir = TempDir::new().unwrap();
        let (state_path, db_path, prior) = seed_height1(&dir);

        let store = RocksStore::open(&db_path).unwrap();
        let c2 = sample_commit(2, "root2", "bh2", "80");
        let staging = staging_state_for(80, 2);
        failpoints::arm(FP_BEFORE_COMMIT);
        let err = persist_staging_then_rocks(&state_path, &staging, &store, &c2).unwrap_err();
        assert!(
            err.to_string().contains("failpoint:before_commit"),
            "{err}"
        );
        failpoints::clear_all();
        drop(store);
        evict_cached(&db_path);

        let tip = reopen_assert_tip(&db_path, &prior);
        assert_eq!(tip.height, 1, "canonical tip not half-applied");
        let store = RocksStore::open(&db_path).unwrap();
        let _ = assert_height_hash_invariant(&store).unwrap();
        assert_eq!(get_head(&store).unwrap(), 1);
        assert!(get_block(&store, 2).unwrap().is_none());
        let a = get_account(&store, "PxA").unwrap().unwrap();
        assert_eq!(a.balance, "90");
        evict_cached(&db_path);
    }

    /// Happy path: full persist advances tip with paired height/hash.
    #[test]
    fn persist_staging_then_rocks_commits_new_tip() {
        let _guard = fp_lock();
        failpoints::clear_all();
        let dir = TempDir::new().unwrap();
        let (state_path, db_path, _prior) = seed_height1(&dir);
        let store = RocksStore::open(&db_path).unwrap();
        let c2 = sample_commit(2, "root2", "bh2", "80");
        persist_staging_then_rocks(&state_path, &staging_state_for(80, 2), &store, &c2).unwrap();
        let tip = assert_height_hash_invariant(&store).unwrap();
        assert_eq!(tip.height, 2);
        assert_eq!(tip.block_hash, "bh2");
        assert_eq!(tip.state_root, "root2");
        drop(store);
        evict_cached(&db_path);
        reopen_assert_tip(
            &db_path,
            &TipPair {
                height: 2,
                block_hash: "bh2".into(),
                state_root: "root2".into(),
            },
        );
        evict_cached(&db_path);
    }
}
