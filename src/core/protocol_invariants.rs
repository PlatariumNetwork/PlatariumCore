//! Core protocol invariants I1–I10 (issue #78).
//!
//! Short freeze of consensus/execution safety rules. Executable coverage is
//! filled by follow-on tests; paths below are placeholders until linked.
//!
//! | Id | Statement | Test path (placeholder) |
//! |----|-----------|-------------------------|
//! | **I1** | Same block and state yield the same [`StateDiff`](crate::core::kernel::StateDiff). | `tests/…#i1_same_block_state_diff` |
//! | **I2** | An invalid signature is never executable. | `tests/…#i2_invalid_signature_never_executable` |
//! | **I3** | Nonce cannot decrease. | `tests/…#i3_nonce_cannot_decrease` |
//! | **I4** | Balance cannot become negative. | `tests/…#i4_balance_cannot_become_negative` |
//! | **I5** | Tokens/XP cannot disappear on persistence. | `tests/…#i5_tokens_xp_persist` |
//! | **I6** | A finalized block cannot be applied twice. | `tests/…#i6_finalized_block_not_applied_twice` |
//! | **I7** | A conflicting block cannot overwrite the canonical tip. | `tests/…#i7_conflict_cannot_overwrite_canonical` |
//! | **I8** | A failed commit cannot expose partial state. | `tests/…#i8_failed_commit_no_partial_state` |
//! | **I9** | Restart preserves canonical state. | `tests/…#i9_restart_preserves_canonical` |
//! | **I10** | A Core error cannot imply consensus acceptance. | `tests/…#i10_core_error_not_consensus_accept` |
//!
//! See also [`crate::core::protocol_notes`] (clocks) and
//! [`crate::core::determinism`] (determinism audit).

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

/// Placeholder test-path anchors for I1–I10 (filled by later linkage tasks).
pub const PROTOCOL_INVARIANT_TEST_PATH_PLACEHOLDERS: &[&str] = &[
    "tests/…#i1_same_block_state_diff",
    "tests/…#i2_invalid_signature_never_executable",
    "tests/…#i3_nonce_cannot_decrease",
    "tests/…#i4_balance_cannot_become_negative",
    "tests/…#i5_tokens_xp_persist",
    "tests/…#i6_finalized_block_not_applied_twice",
    "tests/…#i7_conflict_cannot_overwrite_canonical",
    "tests/…#i8_failed_commit_no_partial_state",
    "tests/…#i9_restart_preserves_canonical",
    "tests/…#i10_core_error_not_consensus_accept",
];

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
