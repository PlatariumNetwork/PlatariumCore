//! # Core finalize contract (ADR)
//!
//! Documented phases for applying an ordered batch and making state durable.
//! Owned by Core (`kernel` + `StorageEngine`); Gateway may orchestrate RPC but
//! does not redefine these phases.
//!
//! ## Phases
//!
//! | Phase | Name | What happens |
//! |-------|------|----------------|
//! | 1 | **PREPARE** | Build [`OrderedBatch`](crate::core::kernel::OrderedBatch), open staging via [`StorageEngine::begin`](crate::storage::engine::StorageEngine::begin). |
//! | 2 | **execute** | [`execute_ordered_batch`](crate::core::kernel::execute_ordered_batch) produces a [`StateDiff`](crate::core::kernel::StateDiff) (receipts + account/escrow post-images). |
//! | 3 | **validate** | Diff schema check; escrow JSON parse; optional verified-execution gate before Rocks block commit (`assert_commit_allowed_after_execution`). |
//! | 4 | **persist** | [`commit_state_diff`](crate::core::kernel::commit_state_diff) → `apply_accounts` / `apply_escrows` staged, then `commit_atomic` (Rocks `WriteBatch` or state-file replace). |
//! | 5 | **COMMITTED** | Staging cleared; durable store reflects post-images; callers may treat `CommitResult.ok == true` as success. |
//!
//! ## Failure → result table
//!
//! | Failure mode | Observable result | Durability |
//! |--------------|-------------------|------------|
//! | **Execution** error (invalid tx / apply panic path) | Batch aborts before persist; `ExecuteOutcome` / RPC error | Prior commit unchanged |
//! | **JSON** decode/encode error (StateDiff, escrow, BlockCommit) | `PlatariumError::State` or `CommitResult { ok: false, error: Some(...) }` | Staging rolled back; prior commit unchanged |
//! | **Rocks** write / open error | Error from `write_batch` / open | No partial head advance for atomic account batch |
//! | **Crash before** `commit_atomic` / `write_batch` | Process dies with staging only | On restart: last **COMMITTED** height/state remains |
//! | **Crash after** successful `write_batch` | Process dies post-persist | Restart loads new post-images / head (idempotent re-read) |
//! | **Duplicate** batch / replay of same post-images | Re-apply same absolute post-images | Same durable values (absolute replace, not delta) |
//! | **Conflict** (touch-set / concurrent staging) | Scheduler / begin contract rejects or caller serializes | No cross-batch interleaving inside one engine begin/commit |
//!
//! Related entrypoints: `kernel_apply_batch` (RPC), `block_cycle` (optional Rocks commit after `apply_txs`).

/// Marker so the finalize ADR module is linked and discoverable in rustdoc.
pub const FINALIZE_CONTRACT_DOC: &str = "PREPARE → execute → validate → persist → COMMITTED";

#[cfg(test)]
mod tests {
    #[test]
    fn phases_marker_lists_contract() {
        assert!(super::FINALIZE_CONTRACT_DOC.contains("PREPARE"));
        assert!(super::FINALIZE_CONTRACT_DOC.contains("COMMITTED"));
    }
}
