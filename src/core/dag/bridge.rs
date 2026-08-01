//! Bridge linearized digests → OrderedBatch (014).

use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::kernel::ordering::build_ordered_batch;
use crate::core::transaction::Transaction;
use crate::error::Result;
use std::collections::HashMap;

/// Convert digest order + payloads into an OrderedBatch.
pub fn dag_to_ordered_batch(
    batch_id: String,
    height: u64,
    digests: &[String],
    payloads: &HashMap<String, Transaction>,
) -> Result<OrderedBatch> {
    build_ordered_batch(batch_id, height, digests, payloads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::dag::linearize::linearize;
    use crate::core::dag::store::DagStore;
    use crate::core::dag::types::DagVertex;
    use crate::core::kernel::execute::{execute_ordered_batch, ExecuteOptions};
    use crate::core::state::State;
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
    use crate::KeyGenerator;
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
            from: from.to_string(),
            to: to.to_string(),
            asset: Asset::PLP.as_canonical(),
            amount,
            fee_uplp: fee,
            nonce,
            reads: vec![],
            writes: vec![],
        };
        let sig = sign_with_both_keys(&message, mnemonic, alpha).unwrap();
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

    #[test]
    fn missing_payload_errors() {
        let err = dag_to_ordered_batch("b".into(), 1, &["h".into()], &HashMap::new());
        assert!(err.unwrap_err().to_string().contains("availability"));
    }

    #[test]
    fn bridge_to_execute_smoke() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobDag00000000000000000000000000000000000000000000000000000000";
        let tx = signed_tx(&mn, &alpha, &alice, bob, 50, 1, 0);

        let mut store = DagStore::new();
        let g = DagVertex::genesis("n0".into());
        let gid = store.insert(g).unwrap();
        let v = DagVertex::new(1, "n1".into(), vec![gid], vec![tx.hash.clone()]);
        let vid = store.insert(v).unwrap();
        let lin = linearize(&store, &vid).unwrap();

        let mut payloads = HashMap::new();
        payloads.insert(tx.hash.clone(), tx);
        let batch = dag_to_ordered_batch("dag-smoke".into(), 1, &lin.digests, &payloads).unwrap();

        let state = State::new();
        state.set_balance(&alice, 1_000_000);
        state.set_uplp_balance(&alice, 100);
        state.set_nonce(&alice, 0);
        let out = execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: false }).unwrap();
        assert_eq!(out.diff.receipts[0].status, "ok");
    }
}
