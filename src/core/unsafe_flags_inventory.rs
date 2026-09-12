//! # Inventory: Core `auto_confirm` and unsafe / escape-hatch flags
//!
//! Audit of Core-owned entrypoints that bypass normal confirmation, ACL, or
//! signature requirements. **Gate owner** = who enforces the flag when present.
//! Gateway-only means Core does not read the env/param; Gateway must not assume
//! Core will honor it.
//!
//! | Flag / param | Entrypoint | Default | Gate owner | Notes |
//! |--------------|------------|---------|------------|-------|
//! | `auto_confirm` (JSON bool) | [`block_cycle_json`](crate::core::block_cycle::block_cycle_json) | `false` | **Core** | Synthesizes unsigned L1/L2 Confirm vote tallies for solo/test; not an env var. |
//! | `PLATARIUM_CORE_RPC_INSECURE` | [`rpc_security`](crate::core::rpc_security) / serve ACL | off | **Core** | Disables RPC token ACL (local tests only). |
//! | `PLATARIUM_CORE_ALLOW_REMOTE_SIGN` | `generate_*` / `sign_*` RPC | off | **Core** | Remote keygen/sign over JSON-RPC. |
//! | `PLATARIUM_DAG_ALLOW_UNSIGNED` | DAG insert RPC | off (on under `cfg(test)`) | **Core** | Allows unsigned DAG vertices. |
//! | L1/L2 vote JSON without ECDSA | `l1_process_votes_json` / `l2_process_votes_json` / `auto_confirm` path | N/A | **Core** | Vote tallies are trust-the-caller CLI helpers; signed-vote API is separate (`confirmation_layer` H3). Gateway must not treat unsigned tallies as production consensus. |
//! | `PLATARIUM_DAG_ALLOW_RESET` | `dag_reset` RPC | off | **Core** | Destructive DAG wipe. |
//! | `PLATARIUM_CORE_ALLOW_EXTERNAL_COMMIT` | `kernel_commit_diff` | off | **Core** | Accept external StateDiff without local execute. |
//! | `PLATARIUM_CORE_ALLOW_EXTERNAL_ROCKS_COMMIT` | `rocks_commit_block` / `block_cycle` commit | off | **Core** | Skip verified-execution gate for Rocks block commit. |
//! | `PLATARIUM_CORE_ALLOW_ROCKS_MIGRATE` | `migrate_json_to_rocks` | off | **Core** | Admin migrate on serve. |
//! | `PLATARIUM_CORE_ALLOW_ROCKS_BOOTSTRAP` | `rocks_bootstrap_snapshot` | off | **Core** | Admin bootstrap on serve. |
//! | `PLATARIUM_CORE_TESTNET` / `PLATARIUM_TESTNET` | `state_credit*` mint paths | off (on under `cfg(test)` if unset) | **Core** | Testnet minting. |
//! | `PLATARIUM_CORE_RPC_POISON_RECOVER` | RPC dispatch mutex | off | **Core** | Recover poisoned lock instead of fail-closed. |
//! | Gateway-only mempool / fee / peer flags | Gateway process | — | **Gateway-only** | Not read by Core binaries; Core does not gate them. |
//!
//! Primary implementations: [`crate::core::rpc_security`], [`crate::core::block_cycle`],
//! [`crate::core::core_rpc`].

/// Stable label for docs/tests referencing this inventory.
pub const UNSAFE_FLAGS_INVENTORY: &str = "core_auto_confirm_and_unsafe_flags";

#[cfg(test)]
mod tests {
    #[test]
    fn inventory_marker_present() {
        assert!(super::UNSAFE_FLAGS_INVENTORY.contains("auto_confirm"));
    }
}
