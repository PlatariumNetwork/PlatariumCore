//! Startup/config gates for multi-node and Melancholy unsafe modes.
//!
//! ## Solo / dev exceptions (default-off; issue #52)
//!
//! When [`multi_node_enabled`] is **false** (default), these escape hatches may be
//! enabled explicitly for local tooling. Defaults remain **off**:
//!
//! | Flag / mode | Default | Solo/dev exception |
//! |-------------|---------|--------------------|
//! | `auto_confirm` | `false` | Allowed when multi-node unset |
//! | `PLATARIUM_CORE_RPC_INSECURE` | off | Allowed only when multi-node unset |
//! | `PLATARIUM_CORE_ALLOW_REMOTE_SIGN` | off | Allowed only when multi-node unset |
//! | Unsigned L1/L2 vote tallies (`l*_process_votes`) | N/A (trust-caller CLI) | Solo only; multi-node refused |
//! | `PLATARIUM_DAG_ALLOW_UNSIGNED` | off | Solo/test only; multi-node refused |
//!
//! ## Melancholy multi-node (fail-closed defaults)
//!
//! When [`melancholy_profile`] and [`multi_node_enabled`] are both set, Core refuses
//! insecure RPC at serve. Independently, **any** multi-node deployment fail-closes
//! `auto_confirm`, remote sign (including the `RPC_INSECURE` bypass of secret RPC),
//! unsigned vote tallies, and unsigned DAG admission even if the env flag is set
//! (see [`MELANCHOLY_MULTI_NODE_DEFAULTS_DOC`]).

use crate::error::{PlatariumError, Result};

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

fn env_profile() -> String {
    std::env::var("PLATARIUM_CORE_PROFILE")
        .or_else(|_| std::env::var("PLATARIUM_NETWORK"))
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// `PLATARIUM_CORE_MULTI_NODE=1` (or truthy) marks a multi-node deployment.
pub fn multi_node_enabled() -> bool {
    env_truthy("PLATARIUM_CORE_MULTI_NODE")
}

/// Melancholy (or melancholy-*) profile from `PLATARIUM_CORE_PROFILE` / `PLATARIUM_NETWORK`.
pub fn melancholy_profile() -> bool {
    let p = env_profile();
    p == "melancholy" || p.starts_with("melancholy") || p.contains("melancholy")
}

/// Solo/dev may use auto_confirm; multi-node + auto_confirm is a config error (issue #48).
pub fn assert_auto_confirm_config(auto_confirm: bool) -> Result<()> {
    if auto_confirm && multi_node_enabled() {
        return Err(PlatariumError::State(
            "config error: multi-node + auto_confirm=true is forbidden (solo/dev only; unset PLATARIUM_CORE_MULTI_NODE or set auto_confirm=false)"
                .into(),
        ));
    }
    Ok(())
}

/// Whether the env requests insecure RPC (raw flag, before Melancholy gate).
pub fn insecure_rpc_env_set() -> bool {
    env_truthy("PLATARIUM_CORE_RPC_INSECURE")
}

/// Multi-node Melancholy must not enable insecure RPC (issue #49).
///
/// Broader multi-node + `RPC_INSECURE` refusal (including non-Melancholy) is
/// enforced by [`assert_multi_node_insecure_remote_sign`] (issue #50).
pub fn assert_insecure_rpc_for_profile() -> Result<()> {
    if insecure_rpc_env_set() && multi_node_enabled() && melancholy_profile() {
        return Err(PlatariumError::State(
            "config error: PLATARIUM_CORE_RPC_INSECURE refused for multi-node Melancholy profile (fail closed)"
                .into(),
        ));
    }
    Ok(())
}

/// Multi-node must not enable remote signing even if ALLOW is set (issue #50).
pub fn assert_remote_sign_config(requested: bool) -> Result<()> {
    if requested && multi_node_enabled() {
        return Err(PlatariumError::State(
            "config error: multi-node + remote sign is forbidden (solo/dev only; unset PLATARIUM_CORE_MULTI_NODE or PLATARIUM_CORE_ALLOW_REMOTE_SIGN)"
                .into(),
        ));
    }
    Ok(())
}

/// Unsigned L1/L2 vote tallies are solo/dev CLI helpers; multi-node fail-closed (issue #50).
pub fn assert_unsigned_votes_allowed() -> Result<()> {
    if multi_node_enabled() {
        return Err(PlatariumError::State(
            "config error: unsigned L1/L2 vote tallies refused in multi-node (fail closed; use signed confirmation APIs)"
                .into(),
        ));
    }
    Ok(())
}

/// Unsigned DAG admission is Core-owned; multi-node must reject (issue #51).
pub fn assert_unsigned_dag_allowed() -> Result<()> {
    if multi_node_enabled() {
        return Err(PlatariumError::State(
            "config error: unsigned DAG admission refused in multi-node (fail closed; unset PLATARIUM_CORE_MULTI_NODE or supply author_sig)"
                .into(),
        ));
    }
    Ok(())
}

/// Multi-node + `RPC_INSECURE` must not open remote sign/keygen (issue #50).
///
/// Melancholy multi-node already refuses insecure via [`assert_insecure_rpc_for_profile`];
/// this catches the non-Melancholy path where `rpc_insecure_allowed` would otherwise
/// bypass secret-method ACL.
pub fn assert_multi_node_insecure_remote_sign() -> Result<()> {
    if multi_node_enabled() && insecure_rpc_env_set() {
        return Err(PlatariumError::State(
            "config error: multi-node + PLATARIUM_CORE_RPC_INSECURE refused (opens remote sign/keygen bypass; fail closed)"
                .into(),
        ));
    }
    Ok(())
}

/// Call from `serve` before binding listeners.
pub fn assert_serve_runtime_gates() -> Result<()> {
    assert_insecure_rpc_for_profile()?;
    // Fail closed if someone left remote-sign on under multi-node (issue #50).
    assert_remote_sign_config(env_truthy("PLATARIUM_CORE_ALLOW_REMOTE_SIGN"))?;
    // Fail closed if INSECURE would bypass remote-sign ACL under multi-node (issue #50).
    assert_multi_node_insecure_remote_sign()?;
    // Fail closed if unsigned DAG admission was left enabled under multi-node (issue #51).
    if env_truthy("PLATARIUM_DAG_ALLOW_UNSIGNED") {
        assert_unsigned_dag_allowed()?;
    }
    Ok(())
}

/// Solo-path documentation marker for tests/docs.
pub const SOLO_AUTO_CONFIRM_DOC: &str =
    "solo/dev: auto_confirm allowed when PLATARIUM_CORE_MULTI_NODE is unset/false";

/// Issue #52: Melancholy multi-node defaults are fail-closed; solo exceptions default-off.
pub const MELANCHOLY_MULTI_NODE_DEFAULTS_DOC: &str = concat!(
    "melancholy multi-node defaults fail-closed: ",
    "auto_confirm=false/rejected; RPC_INSECURE refused; ",
    "ALLOW_REMOTE_SIGN refused; RPC_INSECURE remote-sign bypass refused; ",
    "unsigned L1/L2 tallies refused; ",
    "DAG_ALLOW_UNSIGNED refused; ",
    "solo/dev exceptions default-off (require explicit flag when multi-node unset)"
);

/// Serializes tests that mutate Core gate env vars (issue #53 / runtime_gates).
#[cfg(test)]
pub(crate) static GATE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use super::GATE_ENV_LOCK as ENV_LOCK;

    fn clear_gate_env() {
        std::env::remove_var("PLATARIUM_CORE_MULTI_NODE");
        std::env::remove_var("PLATARIUM_CORE_PROFILE");
        std::env::remove_var("PLATARIUM_NETWORK");
        std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
        std::env::remove_var("PLATARIUM_CORE_ALLOW_REMOTE_SIGN");
        std::env::remove_var("PLATARIUM_DAG_ALLOW_UNSIGNED");
    }

    #[test]
    fn solo_auto_confirm_allowed() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        assert!(assert_auto_confirm_config(true).is_ok());
        assert!(SOLO_AUTO_CONFIRM_DOC.contains("solo"));
    }

    #[test]
    fn multi_node_auto_confirm_rejected() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        let err = assert_auto_confirm_config(true).unwrap_err();
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("auto_confirm"),
            "{err}"
        );
        assert!(assert_auto_confirm_config(false).is_ok());
        clear_gate_env();
    }

    #[test]
    fn multi_node_melancholy_insecure_refused() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        std::env::set_var("PLATARIUM_CORE_PROFILE", "melancholy");
        std::env::set_var("PLATARIUM_CORE_RPC_INSECURE", "1");
        let err = assert_serve_runtime_gates().unwrap_err();
        assert!(
            err.to_string().contains("INSECURE") || err.to_string().contains("Melancholy"),
            "{err}"
        );
        // Solo melancholy + insecure still allowed (gate is multi-node Melancholy).
        std::env::remove_var("PLATARIUM_CORE_MULTI_NODE");
        assert!(assert_serve_runtime_gates().is_ok());
        clear_gate_env();
    }

    #[test]
    fn multi_node_remote_sign_and_unsigned_votes_rejected() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        assert!(assert_remote_sign_config(true).is_ok());
        assert!(assert_unsigned_votes_allowed().is_ok());
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        let err = assert_remote_sign_config(true).unwrap_err();
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("remote sign"),
            "{err}"
        );
        let err = assert_unsigned_votes_allowed().unwrap_err();
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("unsigned"),
            "{err}"
        );
        // Serve must refuse ALLOW_REMOTE_SIGN under multi-node.
        std::env::set_var("PLATARIUM_CORE_ALLOW_REMOTE_SIGN", "1");
        let err = assert_serve_runtime_gates().unwrap_err();
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("remote sign"),
            "{err}"
        );
        clear_gate_env();
    }

    #[test]
    fn multi_node_insecure_remote_sign_bypass_refused_at_serve() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        // Non-Melancholy multi-node + INSECURE must not start serve (issue #50).
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        std::env::set_var("PLATARIUM_CORE_RPC_INSECURE", "1");
        let err = assert_serve_runtime_gates().unwrap_err();
        assert!(
            err.to_string().contains("multi-node")
                && (err.to_string().contains("INSECURE") || err.to_string().contains("remote sign")),
            "{err}"
        );
        clear_gate_env();
    }

    #[test]
    fn multi_node_unsigned_dag_rejected() {
        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        assert!(assert_unsigned_dag_allowed().is_ok());
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        let err = assert_unsigned_dag_allowed().unwrap_err();
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("DAG"),
            "{err}"
        );
        // Serve must refuse DAG_ALLOW_UNSIGNED under multi-node (issue #51).
        std::env::set_var("PLATARIUM_DAG_ALLOW_UNSIGNED", "1");
        let err = assert_serve_runtime_gates().unwrap_err();
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("DAG"),
            "{err}"
        );
        clear_gate_env();
    }

    #[test]
    fn melancholy_defaults_doc_lists_fail_closed() {
        assert!(MELANCHOLY_MULTI_NODE_DEFAULTS_DOC.contains("fail-closed"));
        assert!(MELANCHOLY_MULTI_NODE_DEFAULTS_DOC.contains("solo/dev"));
        assert!(MELANCHOLY_MULTI_NODE_DEFAULTS_DOC.contains("default-off"));
        assert!(MELANCHOLY_MULTI_NODE_DEFAULTS_DOC.contains("ALLOW_REMOTE_SIGN"));
        assert!(MELANCHOLY_MULTI_NODE_DEFAULTS_DOC.contains("DAG_ALLOW_UNSIGNED"));
    }

    /// Issue #53: Core entrypoints fail-closed for unsafe combos without Gateway discipline.
    #[test]
    fn unsafe_core_modes_fail_closed_without_gateway() {
        use crate::core::consensus_cli::l1_process_votes_json;
        use crate::core::block_cycle::block_cycle_json;
        use crate::core::state_file::init_state_file;
        use serde_json::json;
        use tempfile::TempDir;

        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();

        // Path 1: auto_confirm + multi-node via block_cycle (Core-owned).
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        let dir = TempDir::new().unwrap();
        let state = dir.path().join("state.json");
        init_state_file(&state).unwrap();
        let err = block_cycle_json(&json!({
            "state_file": state.to_string_lossy(),
            "mempool_txs": "[]",
            "block_number": 1u64,
            "previous_hash": "0",
            "timestamp": 1i64,
            "producer_id": "n0",
            "auto_confirm": true,
            "apply_txs": false,
        }))
        .expect_err("must not silently accept multi-node auto_confirm");
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("auto_confirm"),
            "{err}"
        );

        // Path 2: unsigned L1 vote tallies refused under multi-node (no Gateway).
        let err = l1_process_votes_json(r#"[{"node_id":"v1","yes":true}]"#)
            .expect_err("must not silently accept unsigned votes in multi-node");
        assert!(
            err.to_string().contains("multi-node") || err.to_string().contains("unsigned"),
            "{err}"
        );

        // Path 3: unsigned L2 vote tallies likewise refused (issue #50).
        use crate::core::consensus_cli::l2_process_votes_json;
        let err = l2_process_votes_json(r#"[{"node_id":"v1","yes":true}]"#)
            .expect_err("must not silently accept unsigned L2 votes in multi-node");
        assert!(
            err.to_string().contains("multi-node") || err.to_string().contains("unsigned"),
            "{err}"
        );
        clear_gate_env();
    }

    /// Dedicated multi-node rejection path for `l2_process_votes_json` (issues #50 / #102).
    #[test]
    fn multi_node_l2_unsigned_votes_path_rejected() {
        use crate::core::consensus_cli::l2_process_votes_json;

        let _g = ENV_LOCK.lock().unwrap();
        clear_gate_env();
        assert!(l2_process_votes_json(r#"[{"node_id":"v1","yes":true}]"#).is_ok());
        std::env::set_var("PLATARIUM_CORE_MULTI_NODE", "1");
        let err = l2_process_votes_json(r#"[{"node_id":"v1","yes":true}]"#)
            .expect_err("l2_process_votes_json must fail closed under multi-node");
        assert!(
            err.to_string().contains("multi-node") && err.to_string().contains("unsigned"),
            "{err}"
        );
        clear_gate_env();
    }
}
