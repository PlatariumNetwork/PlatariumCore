//! # Core finalize contract (ADR)
//!
//! Documented phases for applying an ordered batch and making state durable.
//! Owned by Core (`kernel` + `StorageEngine`); Gateway may orchestrate RPC but
//! does not redefine these phases.
//!
//! ## Phases
//!
//! | Phase | Name | What happens |
//! |-------|------|----------------|
//! | 1 | **PREPARE** | Build [`OrderedBatch`](crate::core::kernel::OrderedBatch), open staging via [`StorageEngine::begin`](crate::storage::engine::StorageEngine::begin). |
//! | 2 | **execute** | [`execute_ordered_batch`](crate::core::kernel::execute_ordered_batch) produces a [`StateDiff`](crate::core::kernel::StateDiff) (receipts + account/escrow post-images). |
//! | 3 | **validate** | Diff schema check; escrow JSON parse; optional verified-execution gate before Rocks block commit (`assert_commit_allowed_after_execution`). |
//! | 4 | **persist** | [`commit_state_diff`](crate::core::kernel::commit_state_diff) → `apply_accounts` / `apply_escrows` staged, then `commit_atomic` (Rocks `WriteBatch` or state-file replace). |
//! | 5 | **COMMITTED** | Staging cleared; durable store reflects post-images; callers may treat `CommitResult.ok == true` as success. |
//!
//! ## Canonical store decision (not dual SoT, not 2PC)
//!
//! **RocksDB is the canonical tip/account store.** The JSON `state_file` is
//! staging, recovery, and/or a local cache for CLI/dev flows — it is **not** a
//! second source of truth alongside Rocks. Finalize is a single ordered path
//! (PREPARE → execute → validate → persist → COMMITTED). Core does **not**
//! implement a real two-phase commit across JSON and Rocks; failures before
//! persist leave the prior committed tip unchanged.
//!
//! ## Failure → result table
//!
//! | Failure mode | Observable result | Durability |
//! |--------------|-------------------|------------|
//! | **Execution** error (invalid tx / apply panic path) | Batch aborts before persist; `ExecuteOutcome` / RPC error | Prior commit unchanged |
//! | **JSON** decode/encode error (StateDiff, escrow, BlockCommit) | `PlatariumError::State` or `CommitResult { ok: false, error: Some(...) }` | Staging rolled back; prior commit unchanged |
//! | **Rocks** write / open error | Error from `write_batch` / open | No partial head advance for atomic account batch |
//! | **Crash before** `commit_atomic` / `write_batch` | Process dies with staging only | On restart: last **COMMITTED** height/state remains |
//! | **Crash after** successful `write_batch` | Process dies post-persist | Restart loads new post-images / head (idempotent re-read) |
//! | **Duplicate** batch / replay of same post-images | Re-apply same absolute post-images | Same durable values (absolute replace, not delta) |
//! | **Conflict** (touch-set / concurrent staging) | Scheduler / begin contract rejects or caller serializes | No cross-batch interleaving inside one engine begin/commit |
//!
//! Related entrypoints: [`finalize_prepare_execute_validate`], `kernel_apply_batch` (RPC),
//! `block_cycle` (optional Rocks commit after `apply_txs`).

use crate::core::failpoints::{self, FP_FINALIZE_BEFORE_PERSIST};
use crate::core::kernel::{
    commit_state_diff, execute_ordered_batch, CommitResult, ExecuteOptions, ExecuteOutcome,
    OrderedBatch, StateDiff, STATE_DIFF_SCHEMA_VERSION,
};
use crate::core::state::State;
use crate::error::{PlatariumError, Result};
use crate::storage::engine::StorageEngine;
use serde::{Deserialize, Serialize};

/// Marker so the finalize ADR module is linked and discoverable in rustdoc.
pub const FINALIZE_CONTRACT_DOC: &str = "PREPARE → execute → validate → persist → COMMITTED";

/// Stable label for the Rocks-canonical / JSON-staging decision (issue #40).
pub const ROCKS_CANONICAL_JSON_STAGING_DECISION: &str =
    "rocks_canonical_tip; json_state_file_staging_recovery_cache; not_dual_sot; not_real_2pc";

/// Last phase completed by [`finalize_prepare_execute_validate`] / [`finalize_to_storage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinalizePhase {
    Prepare,
    Execute,
    Validate,
    Persist,
    Committed,
}

/// Result of the prepare→execute→validate surface (persist optional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeValidateResult {
    pub ok: bool,
    /// Furthest phase reached (validate on success of this entry; never persist here).
    pub phase: FinalizePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<StateDiff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waves: Option<Vec<crate::core::kernel::ExecutionWave>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Always false for [`finalize_prepare_execute_validate`] (stops before persist).
    pub persisted: bool,
}

/// Validate a StateDiff is safe to persist (schema + escrow JSON parse).
pub fn validate_state_diff_for_persist(diff: &StateDiff) -> Result<()> {
    if diff.schema_version != STATE_DIFF_SCHEMA_VERSION {
        return Err(PlatariumError::State(format!(
            "finalize validate: unsupported StateDiff schema {}",
            diff.schema_version
        )));
    }
    if let Some(ref escrows) = diff.escrows_json {
        for js in escrows {
            let _: crate::modules::escrow::Escrow = serde_json::from_str(js).map_err(|e| {
                PlatariumError::State(format!("finalize validate: invalid escrow json: {}", e))
            })?;
        }
    }
    Ok(())
}

/// Single finalize entry: **PREPARE → execute → validate**, stopping before persist.
///
/// - PREPARE: `batch.validate()`
/// - execute: [`execute_ordered_batch`]
/// - validate: [`validate_state_diff_for_persist`]; any non-ok receipt aborts (`ok=false`,
///   `persisted=false`) so callers never persist an invalid execute.
pub fn finalize_prepare_execute_validate(
    pre_state: &State,
    batch: &OrderedBatch,
    opts: ExecuteOptions,
) -> Result<FinalizeValidateResult> {
    // PREPARE
    if let Err(e) = batch.validate() {
        return Ok(FinalizeValidateResult {
            ok: false,
            phase: FinalizePhase::Prepare,
            diff: None,
            waves: None,
            error: Some(e.to_string()),
            persisted: false,
        });
    }

    // execute
    let outcome: ExecuteOutcome = match execute_ordered_batch(pre_state, batch, opts) {
        Ok(o) => o,
        Err(e) => {
            return Ok(FinalizeValidateResult {
                ok: false,
                phase: FinalizePhase::Execute,
                diff: None,
                waves: None,
                error: Some(e.to_string()),
                persisted: false,
            });
        }
    };

    // Invalid execute (failed receipt) stops before persist.
    if let Some(bad) = outcome
        .diff
        .receipts
        .iter()
        .find(|r| r.status != "ok")
        .cloned()
    {
        return Ok(FinalizeValidateResult {
            ok: false,
            phase: FinalizePhase::Execute,
            diff: Some(outcome.diff),
            waves: Some(outcome.waves),
            error: Some(
                bad.error
                    .unwrap_or_else(|| format!("tx {} status={}", bad.tx_hash, bad.status)),
            ),
            persisted: false,
        });
    }

    // validate
    if let Err(e) = validate_state_diff_for_persist(&outcome.diff) {
        return Ok(FinalizeValidateResult {
            ok: false,
            phase: FinalizePhase::Validate,
            diff: Some(outcome.diff),
            waves: Some(outcome.waves),
            error: Some(e.to_string()),
            persisted: false,
        });
    }

    Ok(FinalizeValidateResult {
        ok: true,
        phase: FinalizePhase::Validate,
        diff: Some(outcome.diff),
        waves: Some(outcome.waves),
        error: None,
        persisted: false,
    })
}

/// Last successfully COMMITTED finalize tip (height / batch / post-root).
///
/// Used by [`finalize_to_storage_with_tip`] for duplicate idempotency and conflict reject (issue #46).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizeTip {
    pub height: u64,
    pub batch_id: String,
    pub post_state_root: String,
}

/// Decision from [`decide_finalize_tip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipDecision {
    /// Persist the new validated diff.
    Proceed,
    /// Same height+batch+root already committed — treat as success without overwrite work.
    Idempotent,
}

/// Duplicate finalize of the same tip is idempotent; a conflicting block at the tip is rejected.
pub fn decide_finalize_tip(
    tip: &FinalizeTip,
    batch: &OrderedBatch,
    post_state_root: &str,
) -> Result<TipDecision> {
    if tip.height == 0 {
        return Ok(TipDecision::Proceed);
    }
    if batch.height == tip.height {
        if batch.batch_id == tip.batch_id && post_state_root == tip.post_state_root {
            return Ok(TipDecision::Idempotent);
        }
        return Err(PlatariumError::State(format!(
            "finalize conflict: height {} already committed with different block (canonical unchanged)",
            tip.height
        )));
    }
    Ok(TipDecision::Proceed)
}

/// Full finalize: prepare→execute→validate, then persist only when validate succeeded.
pub fn finalize_to_storage(
    pre_state: &State,
    batch: &OrderedBatch,
    opts: ExecuteOptions,
    storage: &mut dyn StorageEngine,
) -> Result<(FinalizeValidateResult, Option<CommitResult>)> {
    let validated = finalize_prepare_execute_validate(pre_state, batch, opts)?;
    if !validated.ok {
        return Ok((validated, None));
    }
    // Issue #54: injectable failure before durable persist.
    failpoints::hit(FP_FINALIZE_BEFORE_PERSIST)?;
    let diff = validated
        .diff
        .as_ref()
        .ok_or_else(|| PlatariumError::State("finalize: missing diff after validate".into()))?;
    let commit = commit_state_diff(storage, diff)?;
    let mut out = validated;
    if commit.ok {
        out.phase = FinalizePhase::Committed;
        out.persisted = true;
    } else {
        out.ok = false;
        out.phase = FinalizePhase::Persist;
        out.persisted = false;
        out.error = commit.error.clone();
    }
    Ok((out, Some(commit)))
}

/// Like [`finalize_to_storage`], with tip tracking for duplicate/conflict (issue #46).
pub fn finalize_to_storage_with_tip(
    pre_state: &State,
    batch: &OrderedBatch,
    opts: ExecuteOptions,
    storage: &mut dyn StorageEngine,
    tip: &mut FinalizeTip,
) -> Result<(FinalizeValidateResult, Option<CommitResult>)> {
    let validated = finalize_prepare_execute_validate(pre_state, batch, opts)?;
    if !validated.ok {
        return Ok((validated, None));
    }
    let post_root = validated
        .diff
        .as_ref()
        .map(|d| d.post_state_root.clone())
        .ok_or_else(|| PlatariumError::State("finalize: missing diff after validate".into()))?;
    match decide_finalize_tip(tip, batch, &post_root)? {
        TipDecision::Idempotent => {
            let mut out = validated;
            out.phase = FinalizePhase::Committed;
            out.persisted = true;
            let commit = CommitResult {
                ok: true,
                post_state_root: post_root,
                height: batch.height,
                error: None,
            };
            return Ok((out, Some(commit)));
        }
        TipDecision::Proceed => {}
    }
    failpoints::hit(FP_FINALIZE_BEFORE_PERSIST)?;
    let diff = validated
        .diff
        .clone()
        .ok_or_else(|| PlatariumError::State("finalize: missing diff after validate".into()))?;
    let commit = commit_state_diff(storage, &diff)?;
    let mut out = validated;
    if commit.ok {
        out.phase = FinalizePhase::Committed;
        out.persisted = true;
        tip.height = batch.height;
        tip.batch_id = batch.batch_id.clone();
        tip.post_state_root = post_root;
    } else {
        out.ok = false;
        out.phase = FinalizePhase::Persist;
        out.persisted = false;
        out.error = commit.error.clone();
    }
    Ok((out, Some(commit)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::kernel::{execute_ordered_batch, OrderedBatch};
    use crate::core::state::State;
    use crate::core::transaction::Transaction;
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
    use crate::storage::engine::InMemoryStorageEngine;
    use serde::Serialize;
    use std::collections::HashSet;

    #[derive(Serialize)]
    struct TxHashData {
        from: String,
        to: String,
        asset: String,
        amount: u128,
        fee_uplp: u128,
        nonce: u64,
        reads: Vec<String>,
        writes: Vec<String>,
    }

    fn wallet() -> (String, String, String) {
        let (mnemonic, alpha) = generate_mnemonic().unwrap();
        let from = crate::signer::signing_address_from_mnemonic(&mnemonic, &alpha).unwrap();
        (mnemonic, alpha, from)
    }

    fn signed_tx(
        mnemonic: &str,
        alpha: &str,
        from: &str,
        to: &str,
        amount: u128,
        fee: u128,
        nonce: u64,
    ) -> Transaction {
        let message = TxHashData {
            from: from.into(),
            to: to.into(),
            asset: Asset::PLP.as_canonical(),
            amount,
            fee_uplp: fee,
            nonce,
            reads: vec![],
            writes: vec![],
        };
        let sig = sign_with_both_keys(&message, mnemonic, alpha).unwrap();
        Transaction {
            hash: sig.hash,
            from: from.into(),
            to: to.into(),
            asset: Asset::PLP,
            amount,
            fee_uplp: fee,
            nonce,
            reads: HashSet::new(),
            writes: HashSet::new(),
            sig_main: normalize_signature_hex(&sig.signatures[0].signature_compact),
            sig_derived: normalize_signature_hex(&sig.signatures[1].signature_compact),
            pub_main: Some(sig.signatures[0].pub_key.clone()),
            pub_derived: Some(sig.signatures[1].pub_key.clone()),
            tx_kind: None,
            request_id_hash: None,
            settle_outcome: None,
            settle_outcome_key: None,
            escrow_id: None,
            purpose: None,
            expires_at: None,
            settle_payee: None,
            settle_node: None,
        }
    }

    #[test]
    fn phases_marker_lists_contract() {
        assert!(super::FINALIZE_CONTRACT_DOC.contains("PREPARE"));
        assert!(super::FINALIZE_CONTRACT_DOC.contains("COMMITTED"));
        assert!(super::ROCKS_CANONICAL_JSON_STAGING_DECISION.contains("rocks_canonical"));
        assert!(super::ROCKS_CANONICAL_JSON_STAGING_DECISION.contains("not_real_2pc"));
        assert!(!super::ROCKS_CANONICAL_JSON_STAGING_DECISION.contains("dual_sot_enabled"));
    }

    #[test]
    fn prepare_execute_validate_does_not_persist() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobFinalize000000000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("f1".into(), 1, vec![tx]).unwrap();
        let res =
            finalize_prepare_execute_validate(&state, &batch, ExecuteOptions::default()).unwrap();
        assert!(res.ok, "{:?}", res.error);
        assert_eq!(res.phase, FinalizePhase::Validate);
        assert!(!res.persisted);
        assert!(res.diff.is_some());
    }

    #[test]
    fn invalid_execute_stops_before_persist() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobFinalizeFail0000000000000000000000000000000000000000000001";
        let state = State::new();
        // Insufficient balance → execute receipt failed; must not commit.
        state.set_balance(&alice, 1);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("f-bad".into(), 1, vec![tx]).unwrap();

        let mut mem = InMemoryStorageEngine::from_state(&state);
        let (res, commit) =
            finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut mem).unwrap();
        assert!(!res.ok);
        assert!(!res.persisted);
        assert!(commit.is_none());
        assert_eq!(res.phase, FinalizePhase::Execute);
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, "1");
    }

    /// Issue #42: COMMITTED only after durable persist success.
    #[test]
    fn persist_marks_committed_only_after_durable_success() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobFinalizeOk0000000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("f-ok".into(), 1, vec![tx]).unwrap();

        let mut mem = InMemoryStorageEngine::from_state(&state);
        let (res, commit) =
            finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut mem).unwrap();
        assert!(res.ok, "{:?}", res.error);
        assert!(res.persisted);
        assert_eq!(res.phase, FinalizePhase::Committed);
        let commit = commit.expect("commit result present");
        assert!(commit.ok);
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, "4950");
        assert_eq!(mem.get_account(&alice).unwrap().nonce, 1);
    }

    /// Issue #42: partial persist (commit_atomic failure) must not mark COMMITTED.
    #[test]
    fn partial_persist_does_not_mark_committed() {
        struct FailCommitEngine {
            inner: InMemoryStorageEngine,
        }
        impl StorageEngine for FailCommitEngine {
            fn begin(&mut self) -> Result<()> {
                self.inner.begin()
            }
            fn apply_accounts(
                &mut self,
                accounts: &[crate::core::kernel::AccountPostImage],
            ) -> Result<()> {
                self.inner.apply_accounts(accounts)
            }
            fn apply_escrows(&mut self, escrows_json: &[String]) -> Result<()> {
                self.inner.apply_escrows(escrows_json)
            }
            fn commit_atomic(&mut self) -> Result<()> {
                let _ = self.inner.rollback();
                Err(PlatariumError::State("simulated durable persist failure".into()))
            }
            fn rollback(&mut self) -> Result<()> {
                self.inner.rollback()
            }
            fn get_account(
                &self,
                address: &str,
            ) -> Option<crate::core::kernel::AccountPostImage> {
                self.inner.get_account(address)
            }
        }

        let (mn, alpha, alice) = wallet();
        let bob = "PxBobFinalizePartial00000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("f-partial".into(), 1, vec![tx]).unwrap();

        let mut eng = FailCommitEngine {
            inner: InMemoryStorageEngine::from_state(&state),
        };
        let err = finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut eng)
            .expect_err("persist failure must surface");
        assert!(
            err.to_string().contains("durable persist") || err.to_string().contains("simulated"),
            "{err}"
        );
        // Prior committed state unchanged (rollback on failed commit_atomic).
        assert_eq!(eng.get_account(&alice).unwrap().plp_balance, "5000");
        assert_eq!(eng.get_account(&alice).unwrap().nonce, 0);
    }

    /// Issue #43: forced execution failure leaves account + height (batch) unchanged.
    #[test]
    fn execution_failure_leaves_no_account_or_height_mutation() {
        use crate::storage::cache::{evict_cached, open_cached};
        use crate::storage::engine::RocksAccountStorageEngine;
        use crate::storage::query::get_head;
        use tempfile::TempDir;

        let (mn, alpha, alice) = wallet();
        let bob = "PxBobExecFailHeight0000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 100);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 2);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50_000, 1, 2); // will fail balance
        let batch = OrderedBatch::new("exec-fail".into(), 7, vec![tx]).unwrap();
        assert_eq!(batch.height, 7);

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let mut eng = RocksAccountStorageEngine::open(&db_path).unwrap();
        // Seed prior account so we can prove no mutation.
        eng.begin().unwrap();
        eng.apply_accounts(&[crate::core::kernel::AccountPostImage {
            address: alice.clone(),
            plp_balance: "100".into(),
            uplp_balance: "10".into(),
            nonce: 2,
            token_balances: Default::default(),
        }])
        .unwrap();
        eng.commit_atomic().unwrap();
        {
            let store = open_cached(&db_path).unwrap();
            assert_eq!(get_head(store.as_ref()).unwrap(), 0);
        }

        let (res, commit) =
            finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut eng).unwrap();
        assert!(!res.ok);
        assert!(!res.persisted);
        assert!(commit.is_none());
        assert_eq!(res.phase, FinalizePhase::Execute);
        let got = eng.get_account(&alice).unwrap();
        assert_eq!(got.plp_balance, "100");
        assert_eq!(got.nonce, 2);
        let store = open_cached(&db_path).unwrap();
        assert_eq!(
            get_head(store.as_ref()).unwrap(),
            0,
            "height must not advance on execute fail"
        );
        evict_cached(&db_path);
    }

    /// Issue #44: JSON staging persist failure must not mark COMMITTED; prior tip unchanged.
    #[test]
    fn json_staging_persist_failure_blocks_committed() {
        use crate::core::failpoints::{self, FP_JSON_STAGING_COMMIT};
        use crate::core::state_file::load_state_file;
        use crate::storage::engine::StateFileStorageEngine;
        use tempfile::TempDir;

        failpoints::clear_all();
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobJsonFail00000000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("json-fail".into(), 1, vec![tx]).unwrap();

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");
        {
            let mut seed = StateFileStorageEngine::open(&path).unwrap();
            seed.begin().unwrap();
            seed.apply_accounts(&[crate::core::kernel::AccountPostImage {
                address: alice.clone(),
                plp_balance: "5000".into(),
                uplp_balance: "10".into(),
                nonce: 0,
                token_balances: Default::default(),
            }])
            .unwrap();
            seed.commit_atomic().unwrap();
        }
        let tip_before = load_state_file(&path).unwrap();
        assert_eq!(tip_before.get_balance(&alice), 5000);
        assert_eq!(tip_before.get_nonce(&alice), 0);

        failpoints::arm(FP_JSON_STAGING_COMMIT);
        let mut eng = StateFileStorageEngine::open(&path).unwrap();
        let err = finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut eng)
            .expect_err("JSON persist failpoint must surface");
        assert!(
            err.to_string().contains("failpoint:json_staging_commit")
                || err.to_string().contains("json_staging"),
            "{err}"
        );
        failpoints::clear_all();

        // In-engine view rolled back; on-disk tip unchanged.
        assert_eq!(eng.get_account(&alice).unwrap().plp_balance, "5000");
        assert_eq!(eng.get_account(&alice).unwrap().nonce, 0);
        let tip_after = load_state_file(&path).unwrap();
        assert_eq!(tip_after.get_balance(&alice), 5000);
        assert_eq!(tip_after.get_nonce(&alice), 0);
    }

    /// Issue #45: Rocks write failure rolls back to prior commit; no half-applied accounts.
    #[test]
    fn rocks_persist_failure_rolls_back_to_prior_commit() {
        use crate::core::failpoints::{self, FP_ROCKS_WRITE_BATCH};
        use crate::storage::cache::{evict_cached, open_cached};
        use crate::storage::engine::RocksAccountStorageEngine;
        use crate::storage::query::get_account;
        use crate::storage::schema::key_account;
        use tempfile::TempDir;

        failpoints::clear_all();
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobRocksFail000000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("rocks-fail".into(), 1, vec![tx]).unwrap();

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let mut eng = RocksAccountStorageEngine::open(&db_path).unwrap();
        eng.begin().unwrap();
        eng.apply_accounts(&[crate::core::kernel::AccountPostImage {
            address: alice.clone(),
            plp_balance: "5000".into(),
            uplp_balance: "10".into(),
            nonce: 0,
            token_balances: Default::default(),
        }])
        .unwrap();
        eng.commit_atomic().unwrap();
        let prior = eng.get_account(&alice).unwrap();
        assert_eq!(prior.plp_balance, "5000");

        failpoints::arm(FP_ROCKS_WRITE_BATCH);
        let err = finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut eng)
            .expect_err("Rocks write failpoint must surface");
        assert!(
            err.to_string().contains("failpoint:rocks_write_batch"),
            "{err}"
        );
        failpoints::clear_all();

        let got = eng.get_account(&alice).unwrap();
        assert_eq!(got.plp_balance, "5000");
        assert_eq!(got.nonce, 0);
        assert!(
            eng.get_account(bob).is_none(),
            "no half-applied canonical bob account"
        );
        let store = open_cached(&db_path).unwrap();
        let raw = store.get(&key_account(&alice)).unwrap().unwrap();
        let rec: crate::storage::commit::AccountRecord = serde_json::from_slice(&raw).unwrap();
        assert_eq!(rec.balance, "5000");
        assert_eq!(rec.nonce, 0);
        assert!(get_account(store.as_ref(), bob).unwrap().is_none());
        evict_cached(&db_path);
    }

    /// Issue #46: duplicate finalize is idempotent; conflicting block is rejected.
    #[test]
    fn duplicate_finalize_idempotent_and_conflict_reject() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobDupFinal0000000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("dup-1".into(), 1, vec![tx.clone()]).unwrap();

        let mut mem = InMemoryStorageEngine::from_state(&state);
        let mut tip = FinalizeTip::default();
        let (r1, c1) =
            finalize_to_storage_with_tip(&state, &batch, ExecuteOptions::default(), &mut mem, &mut tip)
                .unwrap();
        assert!(r1.ok && r1.persisted);
        assert_eq!(r1.phase, FinalizePhase::Committed);
        assert!(c1.unwrap().ok);
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, "4950");
        let tip_after_first = tip.clone();

        // Duplicate same block → idempotent success; balances unchanged.
        let (r2, c2) =
            finalize_to_storage_with_tip(&state, &batch, ExecuteOptions::default(), &mut mem, &mut tip)
                .unwrap();
        assert!(r2.ok && r2.persisted);
        assert_eq!(r2.phase, FinalizePhase::Committed);
        assert!(c2.unwrap().ok);
        assert_eq!(tip, tip_after_first);
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, "4950");
        assert_eq!(mem.get_account(&alice).unwrap().nonce, 1);

        // Conflicting block at same height → reject; canonical unchanged.
        let tx_conflict = signed_tx(&mn, &alpha, &alice, bob, 10, 1, 0);
        let conflict = OrderedBatch::new("dup-conflict".into(), 1, vec![tx_conflict]).unwrap();
        let err = finalize_to_storage_with_tip(
            &state,
            &conflict,
            ExecuteOptions::default(),
            &mut mem,
            &mut tip,
        )
        .expect_err("conflict must reject");
        assert!(
            err.to_string().contains("conflict") && err.to_string().contains("canonical unchanged"),
            "{err}"
        );
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, "4950");
        assert_eq!(mem.get_account(&alice).unwrap().nonce, 1);
        assert_eq!(tip, tip_after_first);
    }

    /// Issue #66: same initial state + block + txs executed twice → StateDiff A == B.
    #[test]
    fn double_execution_identical_state_diff() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobDoubleExec00000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 10_000);
        state.set_uplp_balance(&alice, 50);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 123, 2, 0);
        let batch = OrderedBatch::new("double-exec".into(), 3, vec![tx]).unwrap();

        let a = execute_ordered_batch(&state, &batch, ExecuteOptions::default()).unwrap();
        let b = execute_ordered_batch(&state, &batch, ExecuteOptions::default()).unwrap();
        assert_eq!(a.diff, b.diff, "StateDiff field drift between identical executions");
        assert_eq!(a.diff.content_fingerprint(), b.diff.content_fingerprint());
        assert_eq!(a.diff.post_state_root, b.diff.post_state_root);
        assert_eq!(a.diff.receipts, b.diff.receipts);
        assert_eq!(a.diff.accounts, b.diff.accounts);
    }
}
