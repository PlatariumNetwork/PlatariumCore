//! Core protocol invariants I1–I10 (issue #78).
//!
//! Short freeze of consensus/execution safety rules. Executable coverage for
//! I1–I4 lands in this module; I5–I10 integration tests live under `tests/`.
//!
//! | Id | Statement | Test path |
//! |----|-----------|-----------|
//! | **I1** | Same block and state yield the same [`StateDiff`](crate::core::kernel::StateDiff). | `src/core/protocol_invariants.rs#i1_same_block_state_diff` |
//! | **I2** | An invalid signature is never executable. | `src/core/protocol_invariants.rs#i2_invalid_signature_never_executable` |
//! | **I3** | Nonce cannot decrease. | `src/core/protocol_invariants.rs#i3_nonce_cannot_decrease` |
//! | **I4** | Balance cannot become negative. | `src/core/protocol_invariants.rs#i4_balance_cannot_become_negative` |
//! | **I5** | Tokens/XP cannot disappear on persistence. | `tests/protocol_invariants_i5_i10_test.rs#i5_tokens_xp_persist` |
//! | **I6** | A finalized block cannot be applied twice. | `tests/protocol_invariants_i5_i10_test.rs#i6_finalized_block_not_applied_twice` |
//! | **I7** | A conflicting block cannot overwrite the canonical tip. | `tests/protocol_invariants_i5_i10_test.rs#i7_conflict_cannot_overwrite_canonical` |
//! | **I8** | A failed commit cannot expose partial state. | `tests/protocol_invariants_i5_i10_test.rs#i8_failed_commit_no_partial_state` |
//! | **I9** | Restart preserves canonical state. | `tests/protocol_invariants_i5_i10_test.rs#i9_restart_preserves_canonical` |
//! | **I10** | A Core error cannot imply consensus acceptance. | `tests/protocol_invariants_i5_i10_test.rs#i10_core_error_not_consensus_accept` |
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

/// Test-path anchors for I1–I10 (all linked to executable tests).
pub const PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS: &[&str] = &[
    "src/core/protocol_invariants.rs#i1_same_block_state_diff",
    "src/core/protocol_invariants.rs#i2_invalid_signature_never_executable",
    "src/core/protocol_invariants.rs#i3_nonce_cannot_decrease",
    "src/core/protocol_invariants.rs#i4_balance_cannot_become_negative",
    "tests/protocol_invariants_i5_i10_test.rs#i5_tokens_xp_persist",
    "tests/protocol_invariants_i5_i10_test.rs#i6_finalized_block_not_applied_twice",
    "tests/protocol_invariants_i5_i10_test.rs#i7_conflict_cannot_overwrite_canonical",
    "tests/protocol_invariants_i5_i10_test.rs#i8_failed_commit_no_partial_state",
    "tests/protocol_invariants_i5_i10_test.rs#i9_restart_preserves_canonical",
    "tests/protocol_invariants_i5_i10_test.rs#i10_core_error_not_consensus_accept",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::asset::Asset;
    use crate::core::execution::{ExecutionContext, ExecutionLogic};
    use crate::core::kernel::diagnose_state_diff_mismatch;
    use crate::core::kernel::execute::{execute_ordered_batch, ExecuteOptions};
    use crate::core::kernel::ordered_batch::OrderedBatch;
    use crate::core::state::State;
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
                "placeholder path must reference i{}: {path}",
                idx + 1
            );
        }
        assert!(
            GATEWAY_CORE_ERROR_NOT_ACCEPT_DOC.contains("must not be mapped to consensus accept"),
            "I10 Gateway consumer doc missing"
        );
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[4]
            .contains("protocol_invariants_i5_i10_test.rs#i5_"));
        assert!(PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS[9]
            .contains("protocol_invariants_i5_i10_test.rs#i10_"));
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
}
