//! Deterministic multi-block replay fixtures (issue #67).
//!
//! Replay `State₀ + Block₁ + Block₂ + Block₃` from scratch and assert a stable tip.

use crate::core::execution::{ExecutionContext, ExecutionLogic};
use crate::core::kernel::clone_state;
use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::state::State;
use crate::error::{PlatariumError, Result};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::kernel::OrderedBatch;
    use crate::core::transaction::Transaction;
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
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

    /// Issue #67: State₀ + Block₁ + Block₂ + Block₃ replay completes with deterministic tip.
    #[test]
    fn multi_block_replay_from_state0_deterministic_tip() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobReplay0000000000000000000000000000000000000000000000001";
        let carol = "PxCarolReplay00000000000000000000000000000000000000000000001";
        let dave = "PxDaveReplay000000000000000000000000000000000000000000000001";

        let state0 = State::new();
        state0.set_balance(&alice, 10_000);
        state0.set_uplp_balance(&alice, 100);
        state0.set_nonce(&alice, 0);
        state0.set_asset_balance(&alice, &Asset::xp(), 7);

        let b1 = OrderedBatch::new(
            "replay-b1".into(),
            1,
            vec![signed_tx(&mn, &alpha, &alice, bob, 100, 1, 0)],
        )
        .unwrap();
        let b2 = OrderedBatch::new(
            "replay-b2".into(),
            2,
            vec![signed_tx(&mn, &alpha, &alice, carol, 200, 1, 1)],
        )
        .unwrap();
        let b3 = OrderedBatch::new(
            "replay-b3".into(),
            3,
            vec![signed_tx(&mn, &alpha, &alice, dave, 300, 1, 2)],
        )
        .unwrap();
        let blocks = [b1, b2, b3];

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
        assert_eq!(tip_state_a.get_balance(&bob.to_string()), 100);
        assert_eq!(tip_state_a.get_balance(&carol.to_string()), 200);
        assert_eq!(tip_state_a.get_balance(&dave.to_string()), 300);
    }
}
