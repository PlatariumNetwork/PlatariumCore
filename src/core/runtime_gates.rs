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
//! | `PLATARIUM_CORE_RPC_INSECURE` | off | Allowed when not multi-node Melancholy |
//! | `PLATARIUM_CORE_ALLOW_REMOTE_SIGN` | off | Allowed only when multi-node unset |
//! | Unsigned L1/L2 vote tallies (`l*_process_votes`) | N/A (trust-caller CLI) | Solo only; multi-node refused |
//! | `PLATARIUM_DAG_ALLOW_UNSIGNED` | off | Solo/test only; multi-node refused |
//!
//! ## Melancholy multi-node (fail-closed defaults)
//!
//! When [`melancholy_profile`] and [`multi_node_enabled`] are both set, Core refuses
//! insecure RPC at serve. Independently, **any** multi-node deployment fail-closes
//! `auto_confirm`, remote sign, unsigned vote tallies, and unsigned DAG admission
//! even if the env flag is set (see [`MELANCHOLY_MULTI_NODE_DEFAULTS_DOC`]).

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

/// Call from `serve` before binding listeners.
pub fn assert_serve_runtime_gates() -> Result<()> {
    assert_insecure_rpc_for_profile()?;
    // Fail closed if someone left remote-sign on under multi-node.
    assert_remote_sign_config(env_truthy("PLATARIUM_CORE_ALLOW_REMOTE_SIGN"))?;
    Ok(())
}

/// Solo-path documentation marker for tests/docs.
pub const SOLO_AUTO_CONFIRM_DOC: &str =
    "solo/dev: auto_confirm allowed when PLATARIUM_CORE_MULTI_NODE is unset/false";

/// Issue #52: Melancholy multi-node defaults are fail-closed; solo exceptions default-off.
pub const MELANCHOLY_MULTI_NODE_DEFAULTS_DOC: &str = concat!(
    "melancholy multi-node defaults fail-closed: ",
    "auto_confirm=false/rejected; RPC_INSECURE refused; ",
    "ALLOW_REMOTE_SIGN refused; unsigned L1/L2 tallies refused; ",
    "DAG_ALLOW_UNSIGNED refused; ",
    "solo/dev exceptions default-off (require explicit flag when multi-node unset)"
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
}
