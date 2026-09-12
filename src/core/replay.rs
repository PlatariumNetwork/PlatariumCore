//! Deterministic multi-block replay fixtures (issues #67 / #68).
//!
//! Replay `State₀ + Block₁ + Block₂ + Block₃` from scratch and assert a stable tip.
//! Issue #68: replayed tip matches a tip persisted via Rocks after the same blocks.

use crate::core::consistency::{
    check_execution_vs_rocks, ConsistencyVerdict, ExecutionTipView,
};
use crate::core::crash_failpoints::read_canonical_tip;
use crate::core::execution::{ExecutionContext, ExecutionLogic};
use crate::core::kernel::clone_state;
use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::state::State;
use crate::error::{PlatariumError, Result};
use crate::storage::commit::{
    commit_block, AccountRecord, BlockCommit, BlockRecordStored, ReceiptRecord,
};
use crate::storage::rocks::RocksStore;
use std::collections::BTreeSet;

/// Tip produced by replaying an ordered sequence of blocks from `state0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayTip {
    pub height: u64,
    pub state_root: String,
    pub account_count: usize,
}

/// Replay `state0` followed by `blocks` (Block₁…Blockₙ) by applying each batch's
/// transactions in order. Completes with a deterministic tip state root.
pub fn replay_blocks_from_state0(state0: &State, blocks: &[OrderedBatch]) -> Result<(State, ReplayTip)> {
    let working = clone_state(state0);
    let mut last_height = 0u64;
    for batch in blocks {
        batch.validate()?;
        for tx in &batch.transactions {
            ExecutionLogic::execute_transaction(&working, tx, ExecutionContext::Production).map_err(
                |e| PlatariumError::State(format!("replay block {}: {}", batch.height, e)),
            )?;
        }
        last_height = batch.height;
    }
    let snap = working.snapshot();
    let tip = ReplayTip {
        height: last_height,
        state_root: snap.compute_state_root(),
        account_count: snap.balance_count(),
    };
    Ok((working, tip))
}

/// Build account records from an execution state (for Rocks persist fixtures).
pub fn accounts_from_state(state: &State) -> Vec<AccountRecord> {
    let mut addrs = BTreeSet::new();
    for (addr, _) in state.get_all_balances() {
        addrs.insert(addr);
    }
    for (addr, _) in state.get_all_nonces() {
        addrs.insert(addr);
    }
    addrs
        .into_iter()
        .map(|address| {
            let tokens = state.token_balances_of(&address);
            let xp = tokens
                .get(&crate::core::asset::Asset::xp().as_canonical())
                .cloned()
                .unwrap_or_else(|| "0".into());
            AccountRecord {
                address: address.clone(),
                balance: state.get_balance(&address).to_string(),
                uplp_balance: state.get_uplp_balance(&address).to_string(),
                nonce: state.get_nonce(&address),
                tokens,
                xp,
            }
        })
        .collect()
}

/// Persist a tip produced by applying the same blocks as a replay (issue #68 helper).
///
/// Writes one synthetic block at `tip.height` whose accounts mirror `tip_state`.
pub fn persist_tip_from_state(
    store: &RocksStore,
    tip_state: &State,
    tip: &ReplayTip,
    block_hash: &str,
) -> Result<()> {
    let accounts = accounts_from_state(tip_state);
    let commit = BlockCommit {
        block: BlockRecordStored {
            height: tip.height.max(1),
            previous_hash: "0".into(),
            timestamp: tip.height.max(1) as i64,
            tx_hashes: vec!["replay-tip".into()],
            merkle_root: "m".into(),
            state_root: tip.state_root.clone(),
            block_hash: block_hash.into(),
            producer_id: "replay".into(),
        },
        tx_jsons: vec![
            r#"{"hash":"replay-tip","from":"PxReplay","to":"PxReplay","asset":"PLP","amount":0,"fee_uplp":0,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
        ],
        accounts,
        receipts: vec![ReceiptRecord {
            tx_hash: "replay-tip".into(),
            status: "ok".into(),
            fee_uplp: 0,
            block_height: tip.height.max(1),
        }],
        state_root: tip.state_root.clone(),
    };
    commit_block(store, &commit)
}

/// Compare a replayed tip against Rocks-persisted tip (issue #68).
///
/// Equality on accounts (including tokens/xp) and height/hash via
/// [`check_execution_vs_rocks`].
pub fn compare_replay_tip_to_persisted(
    replayed: &State,
    tip: &ReplayTip,
    store: &RocksStore,
    block_hash: &str,
) -> Result<ConsistencyVerdict> {
    let _rocks = read_canonical_tip(store)?;
    let exec = ExecutionTipView {
        height: tip.height,
        block_hash: block_hash.into(),
    };
    check_execution_vs_rocks(replayed, &exec, store, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::consistency::STATUS_CONSISTENT;
    use crate::core::kernel::OrderedBatch;
    use crate::core::transaction::Transaction;
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
    use crate::storage::cache::evict_cached;
    use serde::Serialize;
    use std::collections::HashSet;
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

    fn fixture_blocks(alice: &str, mn: &str, alpha: &str) -> (State, [OrderedBatch; 3]) {
        let bob = "PxBobReplay0000000000000000000000000000000000000000000000001";
        let carol = "PxCarolReplay00000000000000000000000000000000000000000000001";
        let dave = "PxDaveReplay000000000000000000000000000000000000000000000001";

        let state0 = State::new();
        state0.set_balance(&alice.to_string(), 10_000);
        state0.set_uplp_balance(&alice.to_string(), 100);
        state0.set_nonce(&alice.to_string(), 0);
        state0.set_asset_balance(&alice.to_string(), &Asset::xp(), 7);

        let b1 = OrderedBatch::new(
            "replay-b1".into(),
            1,
            vec![signed_tx(mn, alpha, alice, bob, 100, 1, 0)],
        )
        .unwrap();
        let b2 = OrderedBatch::new(
            "replay-b2".into(),
            2,
            vec![signed_tx(mn, alpha, alice, carol, 200, 1, 1)],
        )
        .unwrap();
        let b3 = OrderedBatch::new(
            "replay-b3".into(),
            3,
            vec![signed_tx(mn, alpha, alice, dave, 300, 1, 2)],
        )
        .unwrap();
        (state0, [b1, b2, b3])
    }

    /// Issue #67: State₀ + Block₁ + Block₂ + Block₃ replay completes with deterministic tip.
    #[test]
    fn multi_block_replay_from_state0_deterministic_tip() {
        let (mn, alpha, alice) = wallet();
        let (state0, blocks) = fixture_blocks(&alice, &mn, &alpha);

        let (tip_state_a, tip_a) = replay_blocks_from_state0(&state0, &blocks).unwrap();
        let (tip_state_b, tip_b) = replay_blocks_from_state0(&state0, &blocks).unwrap();

        assert_eq!(tip_a.height, 3);
        assert_eq!(tip_a, tip_b, "deterministic tip state");
        assert_eq!(
            tip_state_a.snapshot().compute_state_root(),
            tip_state_b.snapshot().compute_state_root()
        );
        assert_eq!(tip_state_a.get_balance(&alice), tip_state_b.get_balance(&alice));
        assert_eq!(
            tip_state_a.get_asset_balance(&alice, &Asset::xp()),
            7,
            "xp preserved across replay"
        );
        let bob = "PxBobReplay0000000000000000000000000000000000000000000000001";
        let carol = "PxCarolReplay00000000000000000000000000000000000000000000001";
        let dave = "PxDaveReplay000000000000000000000000000000000000000000000001";
        assert_eq!(tip_state_a.get_balance(&bob.to_string()), 100);
        assert_eq!(tip_state_a.get_balance(&carol.to_string()), 200);
        assert_eq!(tip_state_a.get_balance(&dave.to_string()), 300);
    }

    /// Issue #68: replayed tip matches persisted tip (accounts incl. tokens/xp; height/hash).
    #[test]
    fn replay_tip_equals_persisted_tip() {
        let (mn, alpha, alice) = wallet();
        let (state0, blocks) = fixture_blocks(&alice, &mn, &alpha);

        // Path A: apply once and persist to Rocks.
        let (persisted_state, persisted_tip) =
            replay_blocks_from_state0(&state0, &blocks).unwrap();
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("rocks");
        let store = RocksStore::open(&db_path).unwrap();
        let block_hash = "replay-persisted-bh3";
        // Persist at height 3 — commit_block requires contiguous heights 1..3.
        // Seed heights 1 and 2 as placeholders, then tip at 3 with real accounts.
        for h in 1..=2 {
            let placeholder = BlockCommit {
                block: BlockRecordStored {
                    height: h,
                    previous_hash: if h == 1 {
                        "0".into()
                    } else {
                        format!("bh{}", h - 1)
                    },
                    timestamp: h as i64,
                    tx_hashes: vec![format!("ph{h}")],
                    merkle_root: "m".into(),
                    state_root: format!("root{h}"),
                    block_hash: format!("bh{h}"),
                    producer_id: "seed".into(),
                },
                tx_jsons: vec![format!(
                    r#"{{"hash":"ph{h}","from":"PxSeed","to":"PxSeed","asset":"PLP","amount":0,"fee_uplp":0,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}}"#
                )],
                accounts: vec![],
                receipts: vec![ReceiptRecord {
                    tx_hash: format!("ph{h}"),
                    status: "ok".into(),
                    fee_uplp: 0,
                    block_height: h,
                }],
                state_root: format!("root{h}"),
            };
            commit_block(&store, &placeholder).unwrap();
        }
        let tip_commit = BlockCommit {
            block: BlockRecordStored {
                height: 3,
                previous_hash: "bh2".into(),
                timestamp: 3,
                tx_hashes: vec!["replay-tip".into()],
                merkle_root: "m".into(),
                state_root: persisted_tip.state_root.clone(),
                block_hash: block_hash.into(),
                producer_id: "replay".into(),
            },
            tx_jsons: vec![
                r#"{"hash":"replay-tip","from":"PxReplay","to":"PxReplay","asset":"PLP","amount":0,"fee_uplp":0,"nonce":0,"reads":[],"writes":[],"sig_main":"aa","sig_derived":"bb"}"#.into(),
            ],
            accounts: accounts_from_state(&persisted_state),
            receipts: vec![ReceiptRecord {
                tx_hash: "replay-tip".into(),
                status: "ok".into(),
                fee_uplp: 0,
                block_height: 3,
            }],
            state_root: persisted_tip.state_root.clone(),
        };
        commit_block(&store, &tip_commit).unwrap();

        // Path B: independent replay from State₀.
        let (replayed_state, replayed_tip) =
            replay_blocks_from_state0(&state0, &blocks).unwrap();
        assert_eq!(replayed_tip.height, persisted_tip.height);
        assert_eq!(replayed_tip.state_root, persisted_tip.state_root);

        let verdict = compare_replay_tip_to_persisted(
            &replayed_state,
            &replayed_tip,
            &store,
            block_hash,
        )
        .unwrap();
        assert_eq!(verdict.status, STATUS_CONSISTENT, "{verdict:?}");
        assert_eq!(verdict.height, 3);
        assert_eq!(verdict.rocks_head, 3);

        // Explicit account equality including tokens/xp.
        assert_eq!(
            replayed_state.get_asset_balance(&alice, &Asset::xp()),
            7
        );
        assert_eq!(
            replayed_state.get_balance(&alice),
            persisted_state.get_balance(&alice)
        );
        let rocks_tip = read_canonical_tip(&store).unwrap();
        assert_eq!(rocks_tip.height, replayed_tip.height);
        assert_eq!(rocks_tip.block_hash, block_hash);
        assert_eq!(rocks_tip.state_root, replayed_tip.state_root);

        evict_cached(&db_path);
    }
}
