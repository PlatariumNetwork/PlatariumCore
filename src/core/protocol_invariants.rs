//! Core protocol invariants I1–I10 (issue #78 / #89).
//!
//! Short freeze of consensus/execution safety rules. Executable coverage for
//! **I1–I10** lives in this module (`#[cfg(test)]`). Each `Ii` links to a real
//! test name and path (issue #89 / #109).
//!
//! | Id | Statement | Test module / path |
//! |----|-----------|--------------------|
//! | **I1** | Same block and state yield the same [`StateDiff`](crate::core::kernel::StateDiff). | `core::protocol_invariants::tests::i1_same_block_state_diff` → `src/core/protocol_invariants.rs` |
//! | **I2** | An invalid signature is never executable. | `core::protocol_invariants::tests::i2_invalid_signature_never_executable` → `src/core/protocol_invariants.rs` |
//! | **I3** | Nonce cannot decrease. | `core::protocol_invariants::tests::i3_nonce_cannot_decrease` → `src/core/protocol_invariants.rs` |
//! | **I4** | Balance cannot become negative. | `core::protocol_invariants::tests::i4_balance_cannot_become_negative` → `src/core/protocol_invariants.rs` |
//! | **I5** | Tokens/XP cannot disappear on persistence. | `core::protocol_invariants::tests::i5_tokens_xp_persist` → `src/core/protocol_invariants.rs` |
//! | **I6** | A finalized block cannot be applied twice. | `core::protocol_invariants::tests::i6_finalized_block_not_applied_twice` → `src/core/protocol_invariants.rs` |
//! | **I7** | A conflicting block cannot overwrite the canonical tip. | `core::protocol_invariants::tests::i7_conflict_cannot_overwrite_canonical` → `src/core/protocol_invariants.rs` |
//! | **I8** | A failed commit cannot expose partial state. | `core::protocol_invariants::tests::i8_failed_commit_no_partial_state` → `src/core/protocol_invariants.rs` |
//! | **I9** | Restart preserves canonical state. | `core::protocol_invariants::tests::i9_restart_preserves_canonical` → `src/core/protocol_invariants.rs` |
//! | **I10** | A Core error cannot imply consensus acceptance. | `core::protocol_invariants::tests::i10_core_error_not_consensus_accept` → `src/core/protocol_invariants.rs` |
//!
//! ## Cargo / CI invocation (issue #89 / #109)
//!
//! See [`PROTOCOL_INVARIANT_CARGO_TEST_INVOCATION`]. Locally or in CI (covers I1–I10):
//!
//! ```text
//! cargo test --lib core::protocol_invariants::
//! ```
//!
//! Single-invariant filters (examples):
//!
//! ```text
//! cargo test --lib core::protocol_invariants::tests::i1_same_block_state_diff
//! cargo test --lib core::protocol_invariants::tests::i5_tokens_xp_persist
//! ```
//!
//! See also [`crate::core::protocol_notes`] (clocks) and
//! [`crate::core::determinism`] (determinism audit).
//!
//! ## Gateway consumers (I10)
//!
//! See [`GATEWAY_CORE_ERROR_NOT_ACCEPT_DOC`]: a Core error, uncertain result, or
//! non-COMMITTED finalize (`ok=false` / `persisted=false` / JSON-RPC error)
//! must **not** be mapped to consensus accept or block finalized.

/// Stable one-line catalog of I1–I10 for discovery / tests (issue #78).
pub const PROTOCOL_INVARIANTS_DOC: &str = concat!(
    "I1: same block and state yield same StateDiff; ",
    "I2: invalid signature never executable; ",
    "I3: nonce cannot decrease; ",
    "I4: balance cannot become negative; ",
    "I5: tokens/xp cannot disappear on persistence; ",
    "I6: finalized block cannot be applied twice; ",
    "I7: conflicting block cannot overwrite canonical; ",
    "I8: failed commit cannot expose partial state; ",
    "I9: restart preserves canonical state; ",
    "I10: Core error cannot imply consensus acceptance"
);

/// Gateway contract for I10 (issue #88): Core failure ≠ consensus acceptance.
///
/// Gateway must treat any of the following as **not accepted / not finalized**:
/// JSON-RPC `error`, finalize `ok=false`, `persisted=false`, `phase != committed`,
/// or an uncertain/missing tip — never as L1/L2 confirm or block finality.
pub const GATEWAY_CORE_ERROR_NOT_ACCEPT_DOC: &str = concat!(
    "I10 Gateway: Core error/uncertain/ok=false/persisted=false/phase!=committed/JSON-RPC error ",
    "must not be mapped to consensus accept or block finalized"
);

/// Test module/path anchors for I1–I10 (issue #89 / #109; all linked to executable tests).
///
/// Format: `cargo_module_path` → `source_file` (stable discovery string).
/// Name retained for API stability; values are real module/paths (not ellipsis placeholders).
pub const PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS: &[&str] = &[
    "core::protocol_invariants::tests::i1_same_block_state_diff → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i2_invalid_signature_never_executable → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i3_nonce_cannot_decrease → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i4_balance_cannot_become_negative → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i5_tokens_xp_persist → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i6_finalized_block_not_applied_twice → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i7_conflict_cannot_overwrite_canonical → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i8_failed_commit_no_partial_state → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i9_restart_preserves_canonical → src/core/protocol_invariants.rs",
    "core::protocol_invariants::tests::i10_core_error_not_consensus_accept → src/core/protocol_invariants.rs",
];

/// Documented `cargo test` / CI invocation for I1–I10 (issue #89 / #109).
///
/// Run in CI (or locally) to cover the full invariant suite:
/// `cargo test --lib core::protocol_invariants::`.
pub const PROTOCOL_INVARIANT_CARGO_TEST_INVOCATION: &str =
    "cargo test --lib core::protocol_invariants::";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::block_assembly::{block_finalized, BlockConfirmationResult};
    use crate::core::crash_failpoints::{assert_height_hash_invariant, read_canonical_tip};
    use crate::core::execution::{ExecutionContext, ExecutionLogic};
    use crate::core::finalize_contract::{
        finalize_to_storage, finalize_to_storage_with_tip, FinalizePhase, FinalizeTip,
    };
    use crate::core::kernel::diagnose_state_diff_mismatch;
    use crate::core::kernel::execute::{execute_ordered_batch, ExecuteOptions};
    use crate::core::kernel::ordered_batch::OrderedBatch;
    use crate::core::kernel::state_diff::AccountPostImage;
    use crate::core::state::State;
    use crate::core::transaction::Transaction;
    use crate::error::{PlatariumError, Result};
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
    use crate::storage::{
        build_commit_batch, commit_block, get_account, get_block, get_head, get_tx,
        AccountRecord, BlockCommit, BlockRecordStored, InMemoryStorageEngine, ReceiptRecord,
        RocksStore, StorageEngine,
    };
    use serde::Serialize;
    use std::collections::{BTreeMap, HashSet};
    use tempfile::TempDir;

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

    fn signed_tx(
        mnemonic: &str,
        alpha: &str,
        from: &str,
        to: &str,
        amount: u128,
        fee: u128,
        nonce: u64,
    ) -> Transaction {
        let reads: Vec<String> = vec![];
        let writes: Vec<String> = vec![];
        let message = TxHashData {
            from: from.to_string(),
            to: to.to_string(),
            asset: Asset::PLP.as_canonical(),
            amount,
            fee_uplp: fee,
            nonce,
            reads: reads.clone(),
            writes: writes.clone(),
        };
        let sig = sign_with_both_keys(&message, mnemonic, alpha).unwrap();
        let sig_main = normalize_signature_hex(&sig.signatures[0].signature_compact);
        let sig_derived = normalize_signature_hex(&sig.signatures[1].signature_compact);
        Transaction {
            hash: sig.hash.clone(),
            from: from.to_string(),
            to: to.to_string(),
            asset: Asset::PLP,
            amount,
            fee_uplp: fee,
            nonce,
            reads: HashSet::new(),
            writes: HashSet::new(),
            sig_main,
            sig_derived,
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

    fn wallet() -> (String, String, String) {
        let (mnemonic, alpha) = generate_mnemonic().unwrap();
        let from = crate::signer::signing_address_from_mnemonic(&mnemonic, &alpha).unwrap();
        (mnemonic, alpha, from)
    }

    #[test]
    fn protocol_invariants_i1_through_i10_listed() {
        for i in 1..=10 {
            let needle = format!("I{i}:");
            assert!(
                PROTOCOL_INVARIANTS_DOC.contains(&needle),
                "missing {needle} in PROTOCOL_INVARIANTS_DOC"
            );
        }
        assert_eq!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS.len(), 10);
        for (idx, path) in PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS.iter().enumerate() {
            assert!(
                path.contains(&format!("i{}", idx + 1)),
                "test path must reference i{}: {path}",
                idx + 1
            );
            assert!(
                path.contains(" → "),
                "test path must link module → file: {path}"
            );
            assert!(
                !path.contains('…'),
                "test path must not be a placeholder: {path}"
            );
        }
        assert!(
            GATEWAY_CORE_ERROR_NOT_ACCEPT_DOC.contains("must not be mapped to consensus accept"),
            "I10 Gateway consumer doc missing"
        );
        assert_eq!(
            PROTOCOL_INVARIANT_CARGO_TEST_INVOCATION,
            "cargo test --lib core::protocol_invariants::",
            "CI/cargo invocation must document the I1–I10 lib suite"
        );
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[0]
            .contains("core::protocol_invariants::tests::i1_same_block_state_diff"));
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[0]
            .contains("src/core/protocol_invariants.rs"));
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[4]
            .contains("core::protocol_invariants::tests::i5_tokens_xp_persist"));
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[4]
            .contains("src/core/protocol_invariants.rs"));
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[9]
            .contains("core::protocol_invariants::tests::i10_core_error_not_consensus_accept"));
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[9]
            .contains("src/core/protocol_invariants.rs"));
        for path in PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS {
            assert!(
                !path.contains("tests/…") && !path.contains('…'),
                "I1–I10 paths must not be ellipsis placeholders: {path}"
            );
            assert!(
                path.contains("core::protocol_invariants::tests::"),
                "each Ii must link a real unit-test module path: {path}"
            );
        }
    }

    /// Issue #79 / I1: same block + state → identical StateDiff (fails if diverges).
    #[test]
    fn i1_same_block_state_diff() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI100000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 1_000_000);
        state.set_uplp_balance(&alice, 100);
        state.set_nonce(&alice, 0);

        let tx = signed_tx(&mn, &alpha, &alice, bob, 100, 1, 0);
        assert!(tx.validate_basic().is_ok());
        let batch = OrderedBatch::new("i1-batch".into(), 1, vec![tx]).unwrap();

        let out1 =
            execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: false }).unwrap();
        let out2 =
            execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: false }).unwrap();

        diagnose_state_diff_mismatch(&out1.diff, &out2.diff)
            .expect("I1: StateDiff must not diverge for same block and state");
        assert_eq!(
            out1.diff.content_fingerprint(),
            out2.diff.content_fingerprint()
        );
        assert_eq!(out1.diff.receipts[0].status, "ok");
    }

    /// Issue #80 / I2: invalid signature rejected before apply (state unchanged).
    #[test]
    fn i2_invalid_signature_never_executable() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI200000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 1_000_000);
        state.set_uplp_balance(&alice, 100);
        state.set_nonce(&alice, 0);

        let mut tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        // Corrupt signature after signing — must fail validate before apply.
        tx.sig_main = "00".repeat(64);
        assert!(
            tx.validate_basic().is_err(),
            "corrupted sig must fail validate_basic"
        );

        let bal_before = state.get_balance(&alice);
        let nonce_before = state.get_nonce(&alice);
        let err = ExecutionLogic::execute_transaction(&state, &tx, ExecutionContext::Production)
            .expect_err("I2: invalid signature must never execute");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("signature") || msg.contains("invalid"),
            "expected signature rejection, got: {err}"
        );
        assert_eq!(
            state.get_balance(&alice),
            bal_before,
            "I2: balance must not change when sig rejected before apply"
        );
        assert_eq!(
            state.get_nonce(&alice),
            nonce_before,
            "I2: nonce must not change when sig rejected before apply"
        );
        assert_eq!(state.get_balance(&bob.to_string()), 0);
    }

    /// Issue #81 / I3: decreasing (or otherwise non-matching) nonce is rejected.
    #[test]
    fn i3_nonce_cannot_decrease() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI300000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 1_000_000);
        state.set_uplp_balance(&alice, 100);
        // Current nonce is 5; a tx with nonce 4 would decrease / replay.
        state.set_nonce(&alice, 5);

        let tx = signed_tx(&mn, &alpha, &alice, bob, 10, 1, 4);
        assert!(tx.validate_basic().is_ok(), "sig must be valid; nonce is state check");

        let err = ExecutionLogic::execute_transaction(&state, &tx, ExecutionContext::Production)
            .expect_err("I3: decreasing nonce must be rejected");
        assert!(
            err.to_string().to_lowercase().contains("nonce"),
            "expected nonce rejection, got: {err}"
        );
        assert_eq!(state.get_nonce(&alice), 5, "I3: nonce must stay at 5");
        assert_eq!(state.get_balance(&alice), 1_000_000);
        assert_eq!(state.get_balance(&bob.to_string()), 0);
    }

    /// Issue #82 / I4: overspend rejected; balance stays non-negative.
    #[test]
    fn i4_balance_cannot_become_negative() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI400000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 100);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);

        // Amount alone exceeds balance (fee covered by μPLP).
        let tx = signed_tx(&mn, &alpha, &alice, bob, 200, 1, 0);
        assert!(tx.validate_basic().is_ok());

        let err = ExecutionLogic::execute_transaction(&state, &tx, ExecutionContext::Production)
            .expect_err("I4: overspend must be rejected");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("insufficient") || msg.contains("balance"),
            "expected balance rejection, got: {err}"
        );
        assert_eq!(state.get_balance(&alice), 100, "I4: balance must stay non-negative");
        assert_eq!(state.get_nonce(&alice), 0);
        assert_eq!(state.get_balance(&bob.to_string()), 0);
    }

    fn sample_commit(height: u64) -> BlockCommit {
        BlockCommit {
            block: BlockRecordStored {
                height,
                previous_hash: "0".into(),
                timestamp: 1,
                tx_hashes: vec!["aabb".into()],
                merkle_root: "m".into(),
                state_root: "root1".into(),
                block_hash: "bh1".into(),
                producer_id: "n1".into(),
            },
            tx_jsons: vec![
                r#"{"hash":"aabb","from":"PxA","to":"PxB","asset":"PLP","amount":10,"fee_uplp":1,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
            ],
            accounts: vec![
                AccountRecord {
                    address: "PxA".into(),
                    balance: "90".into(),
                    uplp_balance: "0".into(),
                    nonce: 1,
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
                tx_hash: "aabb".into(),
                status: "ok".into(),
                fee_uplp: 1,
                block_height: height,
            }],
            state_root: "root1".into(),
        }
    }

    /// Issue #83 / I5: persist then reopen retains tokens and XP.
    #[test]
    fn i5_tokens_xp_persist() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let mut tokens = BTreeMap::new();
        tokens.insert(Asset::xp().as_canonical(), "250".into());
        tokens.insert("Token:USDT".into(), "100".into());

        let mut commit = sample_commit(1);
        commit.accounts = vec![AccountRecord {
            address: "PxI5".into(),
            balance: "5000".into(),
            uplp_balance: "7".into(),
            nonce: 2,
            tokens: tokens.clone(),
            xp: "250".into(),
        }];

        {
            let store = RocksStore::open(&db_path).unwrap();
            commit_block(&store, &commit).unwrap();
            let rec = get_account(&store, "PxI5").unwrap().unwrap();
            assert_eq!(rec.xp, "250");
            assert_eq!(rec.tokens.get("Token:USDT").map(String::as_str), Some("100"));
            assert_eq!(
                rec.tokens
                    .get(&Asset::xp().as_canonical())
                    .map(String::as_str),
                Some("250")
            );
        }

        // Simulate process restart: reopen Rocks; tokens/xp must still be present.
        let store = RocksStore::open(&db_path).unwrap();
        let rec = get_account(&store, "PxI5").unwrap().unwrap();
        assert_eq!(rec.balance, "5000");
        assert_eq!(rec.uplp_balance, "7");
        assert_eq!(rec.nonce, 2);
        assert_eq!(rec.xp, "250", "I5: xp must survive persist/restart");
        assert_eq!(
            rec.tokens.get("Token:USDT").map(String::as_str),
            Some("100"),
            "I5: tokens must survive persist/restart"
        );
        assert_eq!(
            rec.tokens
                .get(&Asset::xp().as_canonical())
                .map(String::as_str),
            Some("250"),
            "I5: Token:XP map entry must survive persist/restart"
        );
        assert_eq!(get_head(&store).unwrap(), 1);
    }

    /// Issue #84 / I6: second apply of the same finalized tip is idempotent (safe).
    #[test]
    fn i6_finalized_block_not_applied_twice() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        let c1 = sample_commit(1);
        commit_block(&store, &c1).unwrap();
        assert_eq!(get_head(&store).unwrap(), 1);
        let a_before = get_account(&store, "PxA").unwrap().unwrap();

        // Second apply of identical finalized tip → idempotent success, no double mutation.
        commit_block(&store, &c1).unwrap();
        assert_eq!(get_head(&store).unwrap(), 1);
        let a_after = get_account(&store, "PxA").unwrap().unwrap();
        assert_eq!(a_after, a_before);
        assert_eq!(a_after.balance, "90");
        assert_eq!(a_after.nonce, 1);

        // Finalize-tip path: duplicate same batch is TipDecision::Idempotent / COMMITTED.
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI600000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("i6-batch".into(), 1, vec![tx]).unwrap();
        let mut mem = InMemoryStorageEngine::from_state(&state);
        let mut tip = FinalizeTip::default();
        let (r1, _) = finalize_to_storage_with_tip(
            &state,
            &batch,
            ExecuteOptions::default(),
            &mut mem,
            &mut tip,
        )
        .unwrap();
        assert!(r1.ok && r1.persisted);
        assert_eq!(r1.phase, FinalizePhase::Committed);
        let tip_after = tip.clone();
        let bal = mem.get_account(&alice).unwrap().plp_balance.clone();

        let (r2, _) = finalize_to_storage_with_tip(
            &state,
            &batch,
            ExecuteOptions::default(),
            &mut mem,
            &mut tip,
        )
        .unwrap();
        assert!(r2.ok && r2.persisted, "I6: duplicate apply must be idempotent");
        assert_eq!(r2.phase, FinalizePhase::Committed);
        assert_eq!(tip, tip_after);
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, bal);
    }

    /// Issue #85 / I7: conflicting block at tip is rejected; canonical unchanged.
    #[test]
    fn i7_conflict_cannot_overwrite_canonical() {
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        let c1 = sample_commit(1);
        commit_block(&store, &c1).unwrap();
        let tip_before = read_canonical_tip(&store).unwrap();
        assert_eq!(tip_before.height, 1);
        assert_eq!(tip_before.block_hash, "bh1");

        let mut conflict = sample_commit(1);
        conflict.block.block_hash = "evil-hash".into();
        conflict.block.state_root = "evil-root".into();
        conflict.state_root = "evil-root".into();
        conflict.accounts[0].balance = "1".into();
        let err = commit_block(&store, &conflict).unwrap_err();
        assert!(
            err.to_string().contains("conflict") && err.to_string().contains("canonical unchanged"),
            "{err}"
        );

        let tip_after = read_canonical_tip(&store).unwrap();
        assert_eq!(tip_after, tip_before, "I7: canonical tip must not change");
        let block = get_block(&store, 1).unwrap().unwrap();
        assert_eq!(block.block_hash, "bh1");
        assert_eq!(block.state_root, "root1");
        let a = get_account(&store, "PxA").unwrap().unwrap();
        assert_eq!(a.balance, "90", "I7: account must stay at canonical post-image");
    }

    /// Issue #86 / I8: failed commit leaves only the prior canonical state.
    #[test]
    fn i8_failed_commit_no_partial_state() {
        struct FailCommitEngine {
            inner: InMemoryStorageEngine,
        }
        impl StorageEngine for FailCommitEngine {
            fn begin(&mut self) -> Result<()> {
                self.inner.begin()
            }
            fn apply_accounts(&mut self, accounts: &[AccountPostImage]) -> Result<()> {
                self.inner.apply_accounts(accounts)
            }
            fn apply_escrows(&mut self, escrows_json: &[String]) -> Result<()> {
                self.inner.apply_escrows(escrows_json)
            }
            fn commit_atomic(&mut self) -> Result<()> {
                let _ = self.inner.rollback();
                Err(PlatariumError::State(
                    "simulated durable persist failure".into(),
                ))
            }
            fn rollback(&mut self) -> Result<()> {
                self.inner.rollback()
            }
            fn get_account(&self, address: &str) -> Option<AccountPostImage> {
                self.inner.get_account(address)
            }
        }

        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI800000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("i8-fail".into(), 1, vec![tx]).unwrap();

        let mut eng = FailCommitEngine {
            inner: InMemoryStorageEngine::from_state(&state),
        };
        let err = finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut eng)
            .expect_err("I8: persist failure must surface");
        assert!(
            err.to_string().contains("durable persist") || err.to_string().contains("simulated"),
            "{err}"
        );
        assert_eq!(
            eng.get_account(&alice).unwrap().plp_balance,
            "5000",
            "I8: prior canonical balance only"
        );
        assert_eq!(eng.get_account(&alice).unwrap().nonce, 0);
        assert!(
            eng.get_account(bob).is_none(),
            "I8: no partial bob account after failed commit"
        );

        // Crash-before-write: build batch but never WriteBatch → prior COMMITTED tip only.
        let dir = TempDir::new().unwrap();
        let store = RocksStore::open(dir.path().join("db")).unwrap();
        commit_block(&store, &sample_commit(1)).unwrap();
        let prior = read_canonical_tip(&store).unwrap();
        assert_eq!(prior.height, 1);

        let mut next = sample_commit(2);
        next.block.timestamp = 2;
        next.block.previous_hash = "bh1".into();
        next.block.state_root = "root2".into();
        next.state_root = "root2".into();
        next.block.block_hash = "bh2".into();
        next.block.tx_hashes = vec!["ccdd".into()];
        next.tx_jsons = vec![
            r#"{"hash":"ccdd","from":"PxA","to":"PxB","asset":"PLP","amount":1,"fee_uplp":1,"nonce":1,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
        ];
        next.receipts[0].tx_hash = "ccdd".into();
        next.receipts[0].block_height = 2;
        let _unwritten = build_commit_batch(&next).unwrap();
        // Intentionally do not write — simulates crash before durable commit.

        let tip = read_canonical_tip(&store).unwrap();
        assert_eq!(
            tip, prior,
            "I8: failed/aborted commit must leave prior COMMITTED tip"
        );
        assert!(get_tx(&store, "ccdd").unwrap().is_none());
        assert_eq!(get_account(&store, "PxA").unwrap().unwrap().balance, "90");
    }

    /// Issue #87 / I9: after restart, tip equals last COMMITTED height/hash/root.
    #[test]
    fn i9_restart_preserves_canonical() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("db");
        let committed = {
            let store = RocksStore::open(&db_path).unwrap();
            commit_block(&store, &sample_commit(1)).unwrap();
            let tip = read_canonical_tip(&store).unwrap();
            assert_height_hash_invariant(&store).unwrap();
            tip
        };

        // Restart: reopen Rocks; tip must equal last COMMITTED.
        let store = RocksStore::open(&db_path).unwrap();
        let tip = read_canonical_tip(&store).unwrap();
        assert_eq!(tip, committed, "I9: restart tip must equal last COMMITTED");
        assert_eq!(tip.height, 1);
        assert_eq!(tip.block_hash, "bh1");
        assert_eq!(tip.state_root, "root1");
        assert_height_hash_invariant(&store).unwrap();
        assert_eq!(get_head(&store).unwrap(), 1);
        assert!(get_block(&store, 1).unwrap().is_some());
        assert_eq!(get_account(&store, "PxA").unwrap().unwrap().balance, "90");
    }

    /// Issue #88 / I10: Core error/uncertain ≠ accept/finalized (Gateway-documented).
    #[test]
    fn i10_core_error_not_consensus_accept() {
        assert!(
            GATEWAY_CORE_ERROR_NOT_ACCEPT_DOC.contains("must not be mapped to consensus accept"),
            "I10 must be documented for Gateway consumers"
        );
        assert!(GATEWAY_CORE_ERROR_NOT_ACCEPT_DOC.contains("I10"));
        assert!(
            PROTOCOL_INVARIANTS_DOC.contains("I10: Core error cannot imply consensus acceptance")
        );

        let (mn, alpha, alice) = wallet();
        let bob = "PxBobI1000000000000000000000000000000000000000000000000000000000";
        let state = State::new();
        // Insufficient balance → execute fails before persist.
        state.set_balance(&alice, 1);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("i10-err".into(), 1, vec![tx]).unwrap();

        let mut mem = InMemoryStorageEngine::from_state(&state);
        let (res, commit) =
            finalize_to_storage(&state, &batch, ExecuteOptions::default(), &mut mem).unwrap();

        // Error path must not look like accept/finalized.
        assert!(!res.ok, "I10: error path ok must be false");
        assert!(!res.persisted, "I10: error path must not persist");
        assert_ne!(
            res.phase,
            FinalizePhase::Committed,
            "I10: error path phase must not be COMMITTED"
        );
        assert!(commit.is_none(), "I10: no CommitResult on execute failure");
        assert!(res.error.is_some(), "I10: error must be surfaced");

        // Must not imply L2 finalize / confirmation accept.
        assert!(
            !block_finalized(BlockConfirmationResult::Rejected),
            "Rejected is not finalized"
        );
        assert_eq!(res.phase, FinalizePhase::Execute);
        assert_eq!(mem.get_account(&alice).unwrap().plp_balance, "1");
    }
}
