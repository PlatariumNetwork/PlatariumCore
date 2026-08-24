//! Core JSON-RPC authentication and method ACL.
//!
//! Privileged methods require `PLATARIUM_CORE_RPC_TOKEN` unless
//! `PLATARIUM_CORE_RPC_INSECURE=1` is set (local tests only).

use crate::error::{PlatariumError, Result};

/// Methods that never require a token (liveness / handshake only).
const PUBLIC_METHODS: &[&str] = &["ping", "handshake"];

/// Methods that accept mnemonics / return private keys — blocked on serve unless explicitly allowed (H7).
const SECRET_METHODS: &[&str] = &[
    "generate_keys",
    "generate_mnemonic",
    "sign_message",
    "sign_transaction",
];

fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    }
}

/// Allow `dag_reset` over RPC (`PLATARIUM_DAG_ALLOW_RESET=1`). Default: blocked (M1).
pub fn dag_reset_allowed() -> bool {
    env_truthy("PLATARIUM_DAG_ALLOW_RESET")
}

/// Allow unsigned DAG vertices (`PLATARIUM_DAG_ALLOW_UNSIGNED=1`). Default: require sig on RPC insert.
pub fn dag_unsigned_allowed() -> bool {
    env_truthy("PLATARIUM_DAG_ALLOW_UNSIGNED") || cfg!(test)
}

/// Recover from poisoned mutex (`PLATARIUM_CORE_RPC_POISON_RECOVER=1`). Default: fail closed (M5).
pub fn rpc_poison_recover_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_RPC_POISON_RECOVER")
}

/// Max JSON-RPC request line size in bytes (M2).
pub const MAX_RPC_LINE_BYTES: usize = 2 * 1024 * 1024;

/// Allow remote keygen/sign over JSON-RPC (`PLATARIUM_CORE_ALLOW_REMOTE_SIGN=1`).
pub fn remote_sign_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_ALLOW_REMOTE_SIGN")
}

/// Shared secret expected from Gateway (`PLATARIUM_CORE_RPC_TOKEN`).
pub fn configured_rpc_token() -> Option<String> {
    std::env::var("PLATARIUM_CORE_RPC_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Dev/test escape hatch — privileged RPC without token.
pub fn rpc_insecure_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_RPC_INSECURE")
}

/// Server-side testnet minting gate (`PLATARIUM_CORE_TESTNET` or `PLATARIUM_TESTNET`).
/// Under `cfg(test)`, unset env defaults to enabled so unit tests need not set the flag;
/// set `PLATARIUM_CORE_TESTNET=0` to exercise the deny path.
pub fn server_testnet_enabled() -> bool {
    match std::env::var("PLATARIUM_CORE_TESTNET").or_else(|_| std::env::var("PLATARIUM_TESTNET")) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !matches!(v.as_str(), "0" | "false" | "no" | "off" | "")
        }
        Err(_) => cfg!(test),
    }
}

/// Whether `kernel_commit_diff` may accept an external StateDiff.
/// Default off — use atomic `kernel_apply_batch` instead.
pub fn external_kernel_commit_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_ALLOW_EXTERNAL_COMMIT")
}

fn is_public_method(method: &str) -> bool {
    PUBLIC_METHODS.iter().any(|m| *m == method)
}

fn is_secret_method(method: &str) -> bool {
    SECRET_METHODS.iter().any(|m| *m == method)
}

/// Enforce auth for a JSON-RPC method. `auth_token` is taken from the request top-level field.
pub fn authorize_rpc_method(method: &str, auth_token: Option<&str>) -> Result<()> {
    if is_public_method(method) {
        return Ok(());
    }
    // H7: mnemonic/key material must not cross the serve boundary by default.
    if is_secret_method(method) && !remote_sign_allowed() && !rpc_insecure_allowed() {
        return Err(PlatariumError::State(
            "RPC method blocked: remote sign/keygen disabled (set PLATARIUM_CORE_ALLOW_REMOTE_SIGN=1 only for local tooling)"
                .into(),
        ));
    }
    if rpc_insecure_allowed() {
        return Ok(());
    }
    let Some(expected) = configured_rpc_token() else {
        return Err(PlatariumError::State(
            "RPC auth required: set PLATARIUM_CORE_RPC_TOKEN (or PLATARIUM_CORE_RPC_INSECURE=1 for local only)"
                .into(),
        ));
    };
    match auth_token {
        Some(t) if t == expected => Ok(()),
        _ => Err(PlatariumError::State(
            "RPC unauthorized: missing or invalid auth_token".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn public_ping_always_ok() {
        let _g = ENV_LOCK.lock().unwrap();
        assert!(authorize_rpc_method("ping", None).is_ok());
    }

    #[test]
    fn privileged_allowed_when_insecure() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("PLATARIUM_CORE_RPC_INSECURE", "1");
        assert!(authorize_rpc_method("state_apply_tx", None).is_ok());
        std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
    }

    #[test]
    fn privileged_rejects_without_token_when_secure() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
        std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "secret-test-token");
        assert!(authorize_rpc_method("state_apply_tx", None).is_err());
        assert!(authorize_rpc_method("state_apply_tx", Some("wrong")).is_err());
        assert!(authorize_rpc_method("state_apply_tx", Some("secret-test-token")).is_ok());
        std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
    }

    #[test]
    fn server_testnet_flag() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("PLATARIUM_CORE_TESTNET", "0");
        assert!(!server_testnet_enabled());
        std::env::set_var("PLATARIUM_CORE_TESTNET", "true");
        assert!(server_testnet_enabled());
        std::env::remove_var("PLATARIUM_CORE_TESTNET");
    }

    #[test]
    fn external_commit_default_off() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PLATARIUM_CORE_ALLOW_EXTERNAL_COMMIT");
        assert!(!external_kernel_commit_allowed());
    }
}
