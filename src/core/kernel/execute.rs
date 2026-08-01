//! Execute OrderedBatch → StateDiff (no durable I/O).

use crate::core::asset::Asset;
use crate::core::execution::{ExecutionContext, ExecutionLogic};
use crate::core::kernel::ordered_batch::OrderedBatch;
use crate::core::kernel::scheduler::{compute_waves, ExecutionWave};
use crate::core::kernel::state_diff::{
    AccountPostImage, StateDiff, TxReceipt, STATE_DIFF_SCHEMA_VERSION,
};
use crate::core::kernel::touch::touch_set;
use crate::core::state::{State, TREASURY_ADDRESS};
use crate::core::transaction::Transaction;
use crate::error::{PlatariumError, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, Default)]
pub struct ExecuteOptions {
    /// When true, schedule waves and execute non-conflicting txs on forked state, then merge.
    pub parallel: bool,
}

#[derive(Debug, Clone)]
pub struct ExecuteOutcome {
    pub diff: StateDiff,
    pub waves: Vec<ExecutionWave>,
}

/// Deep-clone state via snapshot restore (safe across threads after clone).
pub fn clone_state(state: &State) -> State {
    let s = State::new();
    s.restore(&state.snapshot());
    s
}

/// Execute an ordered batch against `pre_state` without writing storage.
pub fn execute_ordered_batch(
    pre_state: &State,
    batch: &OrderedBatch,
    opts: ExecuteOptions,
) -> Result<ExecuteOutcome> {
    batch.validate()?;
    let pre_root = pre_state.snapshot().compute_state_root();
    let waves = if opts.parallel {
        compute_waves(batch)
    } else {
        // One tx per wave preserves sequential semantics explicitly.
        (0..batch.transactions.len())
            .map(|i| ExecutionWave {
                wave_index: i as u32,
                tx_indices: vec![i as u32],
            })
            .collect()
    };

    let working = clone_state(pre_state);
    let mut receipts: Vec<TxReceipt> = Vec::with_capacity(batch.transactions.len());
    receipts.resize(
        batch.transactions.len(),
        TxReceipt {
            tx_hash: String::new(),
            index: 0,
            status: "failed".into(),
            error: Some("uninitialized".into()),
            fee_uplp: 0,
        },
    );
    let mut touched: BTreeSet<String> = BTreeSet::new();

    if opts.parallel {
        execute_parallel_waves(&working, batch, &waves, &mut receipts, &mut touched)?;
    } else {
        for (i, tx) in batch.transactions.iter().enumerate() {
            let receipt = run_one_tx(&working, i as u32, tx);
            if receipt.status == "ok" {
                for a in touch_set(tx) {
                    touched.insert(a);
                }
            }
            receipts[i] = receipt;
        }
    }

    let accounts = collect_account_images(&working, &touched);
    let post_root = working.snapshot().compute_state_root();
    let mut diff = StateDiff {
        schema_version: STATE_DIFF_SCHEMA_VERSION,
        batch_id: batch.batch_id.clone(),
        receipts,
        accounts,
        pre_state_root: Some(pre_root),
        post_state_root: post_root,
    };
    // Ensure accounts sorted
    diff.accounts.sort_by(|a, b| a.address.cmp(&b.address));

    Ok(ExecuteOutcome { diff, waves })
}

fn run_one_tx(state: &State, index: u32, tx: &Transaction) -> TxReceipt {
    match ExecutionLogic::execute_transaction(state, tx, ExecutionContext::Production) {
        Ok(()) => TxReceipt {
            tx_hash: tx.hash.clone(),
            index,
            status: "ok".into(),
            error: None,
            fee_uplp: tx.fee_uplp,
        },
        Err(e) => TxReceipt {
            tx_hash: tx.hash.clone(),
            index,
            status: "failed".into(),
            error: Some(e.to_string()),
            fee_uplp: 0,
        },
    }
}

/// Parallel waves: fork from wave parent, apply each tx on its own fork, then
/// commit successful txs onto shared state in ascending index order (re-apply)
/// so the durable StateDiff matches sequential semantics.
fn execute_parallel_waves(
    working: &State,
    batch: &OrderedBatch,
    waves: &[ExecutionWave],
    receipts: &mut [TxReceipt],
    touched: &mut BTreeSet<String>,
) -> Result<()> {
    for wave in waves {
        let parent = clone_state(working);
        let results: Mutex<Vec<(u32, TxReceipt, bool)>> = Mutex::new(Vec::new());

        std::thread::scope(|scope| {
            for &idx in &wave.tx_indices {
                let parent_ref = &parent;
                let tx = &batch.transactions[idx as usize];
                let results = &results;
                scope.spawn(move || {
                    let fork = clone_state(parent_ref);
                    let receipt = run_one_tx(&fork, idx, tx);
                    let ok = receipt.status == "ok";
                    results.lock().unwrap().push((idx, receipt, ok));
                });
            }
        });

        let mut wave_results = results.into_inner().unwrap();
        wave_results.sort_by_key(|(i, _, _)| *i);
        for (idx, receipt, ok) in wave_results {
            receipts[idx as usize] = receipt;
            if ok {
                let tx = &batch.transactions[idx as usize];
                // Re-apply on shared working state in index order for determinism.
                ExecutionLogic::execute_transaction(working, tx, ExecutionContext::Production)
                    .map_err(|e| {
                        PlatariumError::State(format!(
                            "wave merge re-apply failed for {}: {}",
                            tx.hash, e
                        ))
                    })?;
                for a in touch_set(tx) {
                    touched.insert(a);
                }
            }
        }
    }
    Ok(())
}

fn collect_account_images(state: &State, touched: &BTreeSet<String>) -> Vec<AccountPostImage> {
    let snap = state.snapshot();
    let mut out = Vec::new();
    for addr in touched {
        let mut token_balances = BTreeMap::new();
        for ((a, asset), bal) in snap.asset_balances_arc().iter() {
            if a == addr && asset != &Asset::PLP.as_canonical() {
                token_balances.insert(asset.clone(), bal.to_string());
            }
        }
        out.push(AccountPostImage {
            address: addr.clone(),
            plp_balance: state.get_balance(addr).to_string(),
            uplp_balance: state.get_uplp_balance(addr).to_string(),
            nonce: state.get_nonce(addr),
            token_balances,
        });
    }
    // Always include treasury if any fee may have landed
    if !touched.contains(TREASURY_ADDRESS) && state.get_uplp_balance(&TREASURY_ADDRESS.to_string()) > 0
    {
        // keep only touched set as specified
    }
    out.sort_by(|a, b| a.address.cmp(&b.address));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::kernel::state_diff::STATE_DIFF_SCHEMA_VERSION;
    use crate::generate_mnemonic;
    use crate::signer::sign_with_both_keys;
    use crate::signature::normalize_signature_hex;
    use crate::KeyGenerator;
    use serde::Serialize;
    use std::collections::HashSet;
    use std::path::PathBuf;

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
        let kg = KeyGenerator::new(0, None, None, None).unwrap();
        let keys = kg.restore_keys(&mnemonic, &alpha, 0, None).unwrap();
        let from = if keys.public_key.starts_with("Px") {
            keys.public_key.clone()
        } else {
            format!("Px{}", keys.public_key)
        };
        (mnemonic, alpha, from)
    }

    #[test]
    fn golden_alice_bob_transfer() {
        let (mn, alpha, alice) = wallet();
        let bob = "PxBobReceiver0000000000000000000000000000000000000000000000000000";
        let state = State::new();
        state.set_balance(&alice, 1_000_000);
        state.set_uplp_balance(&alice, 100);
        state.set_nonce(&alice, 0);

        let tx = signed_tx(&mn, &alpha, &alice, bob, 100, 1, 0);
        assert!(tx.validate_basic().is_ok(), "sig must verify");

        let batch = OrderedBatch::new("golden-1".into(), 1, vec![tx]).unwrap();
        let path_marker = PathBuf::from("/tmp/platarium-kernel-should-not-touch");
        let before_exists = path_marker.exists();

        let out = execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: false }).unwrap();
        assert_eq!(out.diff.schema_version, STATE_DIFF_SCHEMA_VERSION);
        assert_eq!(out.diff.receipts.len(), 1);
        assert_eq!(out.diff.receipts[0].status, "ok");
        assert_eq!(path_marker.exists(), before_exists, "execute must not create paths");

        let alice_img = out
            .diff
            .accounts
            .iter()
            .find(|a| a.address == alice)
            .expect("alice in diff");
        assert_eq!(alice_img.plp_balance, "999900");
        assert_eq!(alice_img.nonce, 1);
        let bob_img = out
            .diff
            .accounts
            .iter()
            .find(|a| a.address == bob)
            .expect("bob in diff");
        assert_eq!(bob_img.plp_balance, "100");
    }

    #[test]
    fn parallel_matches_sequential_two_independent() {
        let (mn1, a1, from1) = wallet();
        let (mn2, a2, from2) = wallet();
        let to1 = "PxToOne000000000000000000000000000000000000000000000000000000001";
        let to2 = "PxToTwo000000000000000000000000000000000000000000000000000000002";

        let state = State::new();
        state.set_balance(&from1, 1_000_000);
        state.set_uplp_balance(&from1, 50);
        state.set_nonce(&from1, 0);
        state.set_balance(&from2, 1_000_000);
        state.set_uplp_balance(&from2, 50);
        state.set_nonce(&from2, 0);

        let tx1 = signed_tx(&mn1, &a1, &from1, to1, 10, 1, 0);
        let tx2 = signed_tx(&mn2, &a2, &from2, to2, 20, 1, 0);
        let batch = OrderedBatch::new("p1".into(), 1, vec![tx1, tx2]).unwrap();

        let seq = execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: false }).unwrap();
        let par = execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: true }).unwrap();
        assert_eq!(seq.diff.content_fingerprint(), par.diff.content_fingerprint());
        assert_eq!(seq.diff.post_state_root, par.diff.post_state_root);
    }

    #[test]
    fn parallel_matches_sequential_chain_and_disjoint() {
        // Mix: two independent + one that shares `from` with the first (must serialize).
        let (mn1, a1, from1) = wallet();
        let (mn2, a2, from2) = wallet();
        let to_a = "PxToA0000000000000000000000000000000000000000000000000000000000A";
        let to_b = "PxToB0000000000000000000000000000000000000000000000000000000000B";
        let to_c = "PxToC0000000000000000000000000000000000000000000000000000000000C";

        let state = State::new();
        state.set_balance(&from1, 1_000_000);
        state.set_uplp_balance(&from1, 50);
        state.set_nonce(&from1, 0);
        state.set_balance(&from2, 1_000_000);
        state.set_uplp_balance(&from2, 50);
        state.set_nonce(&from2, 0);

        let tx0 = signed_tx(&mn1, &a1, &from1, to_a, 10, 1, 0);
        let tx1 = signed_tx(&mn2, &a2, &from2, to_b, 20, 1, 0);
        let tx2 = signed_tx(&mn1, &a1, &from1, to_c, 5, 1, 1);
        let batch = OrderedBatch::new("mix".into(), 1, vec![tx0, tx1, tx2]).unwrap();

        let seq = execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: false }).unwrap();
        let par = execute_ordered_batch(&state, &batch, ExecuteOptions { parallel: true }).unwrap();
        assert_eq!(seq.diff.content_fingerprint(), par.diff.content_fingerprint());
        assert_eq!(seq.diff.post_state_root, par.diff.post_state_root);
        assert!(par.waves.len() >= 2, "shared from must force extra wave");
    }
}
