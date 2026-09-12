//! Startup/config gates for multi-node and Melancholy unsafe modes.
//!
//! ## Solo path (documented exception)
//!
//! `auto_confirm=true` remains allowed for **solo** / local test when
//! [`multi_node_enabled`] is false (default). Multi-node must never synthesize
//! unsigned L1/L2 tallies via Core.
//!
//! ## Melancholy multi-node
//!
//! When [`melancholy_profile`] and [`multi_node_enabled`] are both set,
//! `PLATARIUM_CORE_RPC_INSECURE` is refused at serve (fail closed).

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

/// Call from `serve` before binding listeners.
pub fn assert_serve_runtime_gates() -> Result<()> {
    assert_insecure_rpc_for_profile()?;
    Ok(())
}

/// Solo-path documentation marker for tests/docs.
pub const SOLO_AUTO_CONFIRM_DOC: &str =
    "solo/dev: auto_confirm allowed when PLATARIUM_CORE_MULTI_NODE is unset/false";

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
}
