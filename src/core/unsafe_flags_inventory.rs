//! # Inventory: Core `auto_confirm` and unsafe / escape-hatch flags
//!
//! Audit of Core-owned entrypoints that bypass normal confirmation, ACL, or
//! signature requirements. **Gate owner** = who enforces the flag when present.
//! Gateway-only means Core does not read the env/param; Gateway must not assume
//! Core will honor it.
//!
//! | Flag / param | Entrypoint | Default | Gate owner | Notes |
//! |--------------|------------|---------|------------|-------|
//! | `auto_confirm` (JSON bool) | [`block_cycle_json`](crate::core::block_cycle::block_cycle_json) | `false` | **Core** | Synthesizes unsigned L1/L2 Confirm vote tallies for **solo/test only**; rejected when `PLATARIUM_CORE_MULTI_NODE` is set (see [`runtime_gates`](crate::core::runtime_gates)). |
//! | `PLATARIUM_CORE_RPC_INSECURE` | [`rpc_security`](crate::core::rpc_security) / serve ACL | off | **Core** | Disables RPC token ACL (local tests only). **Refused** for multi-node Melancholy at serve (#49); **any** multi-node + insecure also refused at serve because it opens remote sign/keygen (#50). |
//! | `PLATARIUM_CORE_MULTI_NODE` | [`runtime_gates`](crate::core::runtime_gates) | off | **Core** | Marks multi-node; gates `auto_confirm`, remote sign/keygen (incl. `RPC_INSECURE` bypass), unsigned votes/DAG, and insecure RPC. |
//! | `PLATARIUM_CORE_PROFILE` / `PLATARIUM_NETWORK` | [`runtime_gates`](crate::core::runtime_gates) | unset | **Core** | `melancholy` profile pairs with multi-node insecure gate. |
//! | `PLATARIUM_CORE_ALLOW_REMOTE_SIGN` | `generate_*` / `sign_*` RPC | off | **Core** | Remote keygen/sign over JSON-RPC. **Refused** when multi-node (issue #50). |
//! | `PLATARIUM_DAG_ALLOW_UNSIGNED` | DAG insert RPC | off (on under `cfg(test)` when solo) | **Core** | Allows unsigned DAG vertices. **Refused** when multi-node (issue #51). |
//! | L1/L2 vote JSON without ECDSA | `l1_process_votes_json` / `l2_process_votes_json` / `auto_confirm` path | N/A | **Core** | Vote tallies are trust-the-caller CLI helpers; **refused in multi-node** (issue #50). Signed-vote API is separate (`confirmation_layer` H3). |
//! | `PLATARIUM_DAG_ALLOW_RESET` | `dag_reset` RPC | off | **Core** | Destructive DAG wipe. |
//! | `PLATARIUM_CORE_ALLOW_EXTERNAL_COMMIT` | `kernel_commit_diff` | off | **Core** | Accept external StateDiff without local execute. |
//! | `PLATARIUM_CORE_ALLOW_EXTERNAL_ROCKS_COMMIT` | `rocks_commit_block` / `block_cycle` commit | off | **Core** | Skip verified-execution gate for Rocks block commit. |
//! | `PLATARIUM_CORE_ALLOW_ROCKS_MIGRATE` | `migrate_json_to_rocks` | off | **Core** | Admin migrate on serve. |
//! | `PLATARIUM_CORE_ALLOW_ROCKS_BOOTSTRAP` | `rocks_bootstrap_snapshot` | off | **Core** | Admin bootstrap on serve. |
//! | `PLATARIUM_CORE_TESTNET` / `PLATARIUM_TESTNET` | `state_credit*` mint paths | off (on under `cfg(test)` if unset) | **Core** | Testnet minting. |
//! | `PLATARIUM_CORE_RPC_POISON_RECOVER` | RPC dispatch mutex | off | **Core** | Recover poisoned lock instead of fail-closed. |
//! | Gateway-only mempool / fee / peer flags | Gateway process | — | **Gateway-only** | Not read by Core binaries; Core does not gate them. |
//!
//! ## Solo/dev vs Melancholy multi-node (issue #52)
//!
//! See [`crate::core::runtime_gates::MELANCHOLY_MULTI_NODE_DEFAULTS_DOC`]: Melancholy
//! multi-node defaults are fail-closed; solo/dev exceptions remain default-off and
//! require an explicit flag only when multi-node is unset.
//!
//! Primary implementations: [`crate::core::rpc_security`], [`crate::core::block_cycle`],
//! [`crate::core::core_rpc`], [`crate::core::runtime_gates`].

/// Stable label for docs/tests referencing this inventory.
pub const UNSAFE_FLAGS_INVENTORY: &str = "core_auto_confirm_and_unsafe_flags";

#[cfg(test)]
mod tests {
    #[test]
    fn inventory_marker_present() {
        assert!(super::UNSAFE_FLAGS_INVENTORY.contains("auto_confirm"));
    }

    #[test]
    fn inventory_documents_multi_node_gates() {
        let src = include_str!("unsafe_flags_inventory.rs");
        assert!(src.contains("ALLOW_REMOTE_SIGN"));
        assert!(src.contains("DAG_ALLOW_UNSIGNED"));
        assert!(src.contains("multi-node"));
        assert!(src.contains("issue #52") || src.contains("MELANCHOLY_MULTI_NODE"));
    }
}
