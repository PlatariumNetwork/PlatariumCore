//! Apply StateDiff through StorageEngine (no execution logic here).

use crate::core::kernel::state_diff::StateDiff;
use crate::error::{PlatariumError, Result};
use crate::storage::engine::StorageEngine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitResult {
    pub ok: bool,
    pub post_state_root: String,
    #[serde(default)]
    pub height: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Atomically apply account post-images from a StateDiff.
pub fn commit_state_diff(storage: &mut dyn StorageEngine, diff: &StateDiff) -> Result<CommitResult> {
    if diff.schema_version != crate::core::kernel::state_diff::STATE_DIFF_SCHEMA_VERSION {
        return Err(PlatariumError::State(format!(
            "unsupported StateDiff schema {}",
            diff.schema_version
        )));
    }
    storage.begin()?;
    match storage.apply_accounts(&diff.accounts) {
        Ok(()) => {}
        Err(e) => {
            let _ = storage.rollback();
            return Ok(CommitResult {
                ok: false,
                post_state_root: diff.post_state_root.clone(),
                height: 0,
                error: Some(e.to_string()),
            });
        }
    }
    storage.commit_atomic()?;
    Ok(CommitResult {
        ok: true,
        post_state_root: diff.post_state_root.clone(),
        height: 0,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::kernel::execute::{execute_ordered_batch, ExecuteOptions};
    use crate::core::kernel::ordered_batch::OrderedBatch;
    use crate::core::kernel::state_diff::{AccountPostImage, STATE_DIFF_SCHEMA_VERSION};
    use crate::core::state::State;
    use crate::core::transaction::Transaction;
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
    use crate::storage::engine::InMemoryStorageEngine;
    use crate::KeyGenerator;
    use serde::Serialize;
    use std::collections::{BTreeMap, HashSet};

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
        let kg = KeyGenerator::new(0, None, None, None).unwrap();
        let keys = kg.restore_keys(&mnemonic, &alpha, 0, None).unwrap();
        let from = if keys.public_key.starts_with("Px") {
            keys.public_key
        } else {
            format!("Px{}", keys.public_key)
        };
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
    fn execute_commit_roundtrip_inmemory() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobCommit0000000000000000000000000000000000000000000000000001";
        let state = State::new();
        state.set_balance(&alice, 5000);
        state.set_uplp_balance(&alice, 10);
        state.set_nonce(&alice, 0);
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);
        let batch = OrderedBatch::new("c1".into(), 1, vec![tx]).unwrap();
        let out = execute_ordered_batch(&state, &batch, ExecuteOptions::default()).unwrap();

        let mut mem = InMemoryStorageEngine::from_state(&state);
        let res = commit_state_diff(&mut mem, &out.diff).unwrap();
        assert!(res.ok);
        let a = mem.get_account(&alice).unwrap();
        assert_eq!(a.plp_balance, "4950");
        assert_eq!(a.nonce, 1);
        let b = mem.get_account(bob).unwrap();
        assert_eq!(b.plp_balance, "50");
    }

    #[test]
    fn rollback_leaves_memory_unchanged() {
        let mut mem = InMemoryStorageEngine::default();
        mem.begin().unwrap();
        mem.apply_accounts(&[AccountPostImage {
            address: "X".into(),
            plp_balance: "9".into(),
            uplp_balance: "1".into(),
            nonce: 0,
            token_balances: BTreeMap::new(),
        }])
        .unwrap();
        mem.rollback().unwrap();
        assert!(mem.get_account("X").is_none());

        let diff = StateDiff {
            schema_version: STATE_DIFF_SCHEMA_VERSION,
            batch_id: "x".into(),
            receipts: vec![],
            accounts: vec![],
            pre_state_root: None,
            post_state_root: "r".into(),
        };
        // commit empty still ok
        assert!(commit_state_diff(&mut mem, &diff).unwrap().ok);
    }
}
