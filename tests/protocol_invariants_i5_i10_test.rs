//! Executable coverage for protocol invariants I5–I10 (issues #83–#88).
//!
//! | Id | Statement |
//! |----|-----------|
//! | I5 | Tokens/XP cannot disappear on persistence |
//! | I6 | A finalized block cannot be applied twice |
//! | I7 | A conflicting block cannot overwrite the canonical tip |
//! | I8 | A failed commit cannot expose partial state |
//! | I9 | Restart preserves canonical state |
//! | I10 | A Core error cannot imply consensus acceptance |

use platarium_core::*;
use std::collections::{BTreeMap, HashSet};
use tempfile::TempDir;

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

fn wallet() -> (String, String, String) {
    let (mnemonic, alpha) = generate_mnemonic().unwrap();
    let from = signing_address_from_mnemonic(&mnemonic, &alpha).unwrap();
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
    #[derive(serde::Serialize)]
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
    let (r1, _) =
        finalize_to_storage_with_tip(&state, &batch, ExecuteOptions::default(), &mut mem, &mut tip)
            .unwrap();
    assert!(r1.ok && r1.persisted);
    assert_eq!(r1.phase, FinalizePhase::Committed);
    let tip_after = tip.clone();
    let bal = mem.get_account(&alice).unwrap().plp_balance.clone();

    let (r2, _) =
        finalize_to_storage_with_tip(&state, &batch, ExecuteOptions::default(), &mut mem, &mut tip)
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
    assert_eq!(tip, prior, "I8: failed/aborted commit must leave prior COMMITTED tip");
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
    assert!(PROTOCOL_INVARIANTS_DOC.contains("I10: Core error cannot imply consensus acceptance"));

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
