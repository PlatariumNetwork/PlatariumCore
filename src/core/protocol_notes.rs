//! Core protocol notes (rustdoc / module constants).
//!
//! # Three clocks (issue #70)
//!
//! Platarium Core splits time into three distinct clocks. Callers must not
//! treat local wall clock or CLI `now_unix` as unchecked consensus time.
//!
//! | Clock | Meaning | Used for |
//! |-------|---------|----------|
//! | **Consensus timestamp** | Block / tip timestamp agreed by consensus rules (monotonic vs previous tip; subject to drift gates). | Block validity, chain ordering |
//! | **Execution timestamp** | Logical time carried on txs / escrow fields when the protocol requires it; deterministic input to execution — never `SystemTime`. | Escrow expiry fields, tx-embedded times |
//! | **Local wall clock** | Operator / process wall time (`now_unix` from CLI/RPC caller, or host clock). | Mempool wait heuristics, `MAX_DRIFT` checks against consensus timestamps |
//!
//! ## `now_unix` contract
//!
//! [`crate::core::block_proposal::block_proposal_status`] takes `now_unix` as an
//! **operator-supplied local wall clock** for mempool age / propose heuristics.
//! It is **not** a consensus timestamp and must not be written into blocks as
//! unchecked consensus time. Consensus timestamps are validated on consensus-
//! critical paths (monotonicity + drift), not by treating caller `now_unix` as
//! authoritative chain time.

/// Stable label for the three-clock split (discoverable in rustdoc / tests).
pub const THREE_CLOCK_SPLIT_DOC: &str =
    "consensus_timestamp; execution_timestamp; local_wall_clock; now_unix_is_wall_not_consensus";

/// Protocol note: caller `now_unix` is local wall clock, not consensus time.
pub const NOW_UNIX_IS_WALL_CLOCK: &str =
    "now_unix is caller local wall clock for heuristics; not unchecked consensus timestamp";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_clock_doc_mentions_all_clocks() {
        assert!(THREE_CLOCK_SPLIT_DOC.contains("consensus_timestamp"));
        assert!(THREE_CLOCK_SPLIT_DOC.contains("execution_timestamp"));
        assert!(THREE_CLOCK_SPLIT_DOC.contains("local_wall_clock"));
        assert!(NOW_UNIX_IS_WALL_CLOCK.contains("not unchecked consensus"));
    }
}
