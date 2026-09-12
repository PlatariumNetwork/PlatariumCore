//! Core JSON-RPC authentication and method ACL.
//!
//! Tiered secrets (R2-M2 / R2-H1):
//! - `PLATARIUM_CORE_RPC_READ_TOKEN` — read-only methods (falls back to `PLATARIUM_CORE_RPC_TOKEN`)
//! - `PLATARIUM_CORE_RPC_MUTATE_TOKEN` — state-mutating methods (falls back to `PLATARIUM_CORE_RPC_TOKEN`)
//! - `PLATARIUM_CORE_ADMIN_TOKEN` — destructive rocks/admin (never satisfied by Gateway token alone)
//!
//! `PLATARIUM_CORE_RPC_INSECURE=1` disables ACL (local tests only).

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

/// Read-only JSON-RPC methods (R2-M2). Anything else privileged is mutate-tier unless listed as rocks-admin.
const READ_METHODS: &[&str] = &[
    "state_query",
    "state_root",
    "state_validate_tx",
    "rocks_get_head",
    "rocks_get_tx",
    "rocks_get_block",
    "rocks_get_account",
    "rocks_get_receipt",
    "rocks_get_state_root",
    "rocks_get_snapshot",
    "rocks_list_address_txs",
    "rocks_list_snapshots",
    "block_proposal_status",
    "selection_percent_from_load",
    "committee_count",
    "select_committee",
    "min_fee_from_load",
    "verify_signature",
    "normalize_signature",
    "validate_mnemonic",
];

/// Destructive Rocks storage-admin methods (R2-H1 / R2-M2).
const ROCKS_ADMIN_METHODS: &[&str] = &[
    "rocks_commit_block",
    "rocks_bootstrap_snapshot",
    "migrate_json_to_rocks",
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

/// Max JSON-RPC request line size in bytes (M2 / R2-M1).
pub const MAX_RPC_LINE_BYTES: usize = 2 * 1024 * 1024;

/// Allow remote keygen/sign over JSON-RPC (`PLATARIUM_CORE_ALLOW_REMOTE_SIGN=1`).
pub fn remote_sign_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_ALLOW_REMOTE_SIGN")
}

/// Shared Gateway secret (`PLATARIUM_CORE_RPC_TOKEN`) — legacy fallback for read/mutate tiers.
pub fn configured_rpc_token() -> Option<String> {
    std::env::var("PLATARIUM_CORE_RPC_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Read-tier secret (`PLATARIUM_CORE_RPC_READ_TOKEN`), else [`configured_rpc_token`] (R2-M2).
pub fn configured_read_token() -> Option<String> {
    std::env::var("PLATARIUM_CORE_RPC_READ_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(configured_rpc_token)
}

/// Mutate-tier secret (`PLATARIUM_CORE_RPC_MUTATE_TOKEN`), else [`configured_rpc_token`] (R2-M2).
pub fn configured_mutate_token() -> Option<String> {
    std::env::var("PLATARIUM_CORE_RPC_MUTATE_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(configured_rpc_token)
}

/// Separate storage-admin secret (`PLATARIUM_CORE_ADMIN_TOKEN`). Distinct from Gateway tokens.
pub fn configured_admin_token() -> Option<String> {
    std::env::var("PLATARIUM_CORE_ADMIN_TOKEN")
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

/// Allow raw `rocks_commit_block` over RPC without verified execution
/// (`PLATARIUM_CORE_ALLOW_EXTERNAL_ROCKS_COMMIT=1`). Default: blocked (R2-H1).
pub fn external_rocks_commit_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_ALLOW_EXTERNAL_ROCKS_COMMIT")
}

/// Allow `migrate_json_to_rocks` on serve (`PLATARIUM_CORE_ALLOW_ROCKS_MIGRATE=1`). Default: blocked.
pub fn rocks_migrate_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_ALLOW_ROCKS_MIGRATE")
}

/// Allow `rocks_bootstrap_snapshot` on serve (`PLATARIUM_CORE_ALLOW_ROCKS_BOOTSTRAP=1`). Default: blocked.
pub fn rocks_bootstrap_allowed() -> bool {
    env_truthy("PLATARIUM_CORE_ALLOW_ROCKS_BOOTSTRAP")
}

pub fn is_rocks_admin_method(method: &str) -> bool {
    ROCKS_ADMIN_METHODS.iter().any(|m| *m == method)
}

pub fn is_read_method(method: &str) -> bool {
    READ_METHODS.iter().any(|m| *m == method)
}

fn is_public_method(method: &str) -> bool {
    PUBLIC_METHODS.iter().any(|m| *m == method)
}

fn is_secret_method(method: &str) -> bool {
    SECRET_METHODS.iter().any(|m| *m == method)
}

fn token_matches(provided: Option<&str>, expected: &str) -> bool {
    matches!(provided, Some(t) if t == expected)
}

/// Enforce auth for a JSON-RPC method. `auth_token` is taken from the request top-level field.
pub fn authorize_rpc_method(method: &str, auth_token: Option<&str>) -> Result<()> {
    authorize_rpc_method_with_admin(method, auth_token, None)
}

/// Like [`authorize_rpc_method`], with optional `admin_token` for storage-admin methods (R2-H1 / R2-M2).
pub fn authorize_rpc_method_with_admin(
    method: &str,
    auth_token: Option<&str>,
    admin_token: Option<&str>,
) -> Result<()> {
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

    // R2-H1 / R2-M2: rocks admin never accepts Gateway read/mutate token alone.
    if is_rocks_admin_method(method) {
        // Still require a mutate-tier token (or legacy RPC token) plus admin.
        let Some(expected_mutate) = configured_mutate_token() else {
            return Err(PlatariumError::State(
                "RPC auth required: set PLATARIUM_CORE_RPC_MUTATE_TOKEN or PLATARIUM_CORE_RPC_TOKEN (or PLATARIUM_CORE_RPC_INSECURE=1 for local only)"
                    .into(),
            ));
        };
        if !token_matches(auth_token, &expected_mutate) {
            return Err(PlatariumError::State(
                "RPC unauthorized: missing or invalid auth_token".into(),
            ));
        }
        return authorize_rocks_admin_token(admin_token);
    }

    if is_read_method(method) {
        let Some(expected) = configured_read_token() else {
            return Err(PlatariumError::State(
                "RPC auth required: set PLATARIUM_CORE_RPC_READ_TOKEN or PLATARIUM_CORE_RPC_TOKEN (or PLATARIUM_CORE_RPC_INSECURE=1 for local only)"
                    .into(),
            ));
        };
        // Read tier accepts read token; mutate token also permitted (higher privilege).
        if token_matches(auth_token, &expected) {
            return Ok(());
        }
        if let Some(mutate) = configured_mutate_token() {
            if token_matches(auth_token, &mutate) {
                return Ok(());
            }
        }
        return Err(PlatariumError::State(
            "RPC unauthorized: missing or invalid auth_token".into(),
        ));
    }

    // Mutate tier (default for privileged non-read methods).
    let Some(expected) = configured_mutate_token() else {
        return Err(PlatariumError::State(
            "RPC auth required: set PLATARIUM_CORE_RPC_MUTATE_TOKEN or PLATARIUM_CORE_RPC_TOKEN (or PLATARIUM_CORE_RPC_INSECURE=1 for local only)"
                .into(),
        ));
    };
    if token_matches(auth_token, &expected) {
        Ok(())
    } else {
        Err(PlatariumError::State(
            "RPC unauthorized: missing or invalid auth_token".into(),
        ))
    }
}

/// Require storage-admin secret for rocks write methods on serve.
pub fn authorize_rocks_admin_token(admin_token: Option<&str>) -> Result<()> {
    if rpc_insecure_allowed() {
        return Ok(());
    }
    let Some(expected) = configured_admin_token() else {
        return Err(PlatariumError::State(
            "RPC admin auth required: set PLATARIUM_CORE_ADMIN_TOKEN for rocks_commit/migrate/bootstrap"
                .into(),
        ));
    };
    match admin_token {
        Some(t) if t == expected => Ok(()),
        _ => Err(PlatariumError::State(
            "RPC unauthorized: missing or invalid admin_token".into(),
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
        std::env::remove_var("PLATARIUM_CORE_RPC_READ_TOKEN");
        std::env::remove_var("PLATARIUM_CORE_RPC_MUTATE_TOKEN");
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

    #[test]
    fn rocks_admin_requires_separate_admin_token() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
        std::env::remove_var("PLATARIUM_CORE_RPC_READ_TOKEN");
        std::env::remove_var("PLATARIUM_CORE_RPC_MUTATE_TOKEN");
        std::env::set_var("PLATARIUM_CORE_RPC_TOKEN", "rpc-tok");
        std::env::set_var("PLATARIUM_CORE_ADMIN_TOKEN", "admin-tok");
        assert!(authorize_rpc_method_with_admin(
            "rocks_commit_block",
            Some("rpc-tok"),
            None
        )
        .is_err());
        assert!(authorize_rpc_method_with_admin(
            "rocks_commit_block",
            Some("rpc-tok"),
            Some("wrong")
        )
        .is_err());
        assert!(authorize_rpc_method_with_admin(
            "rocks_commit_block",
            Some("rpc-tok"),
            Some("admin-tok")
        )
        .is_ok());
        assert!(authorize_rpc_method_with_admin(
            "rocks_get_head",
            Some("rpc-tok"),
            None
        )
        .is_ok());
        std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
        std::env::remove_var("PLATARIUM_CORE_ADMIN_TOKEN");
    }

    #[test]
    fn rocks_allow_flags_default_off() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PLATARIUM_CORE_ALLOW_EXTERNAL_ROCKS_COMMIT");
        std::env::remove_var("PLATARIUM_CORE_ALLOW_ROCKS_MIGRATE");
        std::env::remove_var("PLATARIUM_CORE_ALLOW_ROCKS_BOOTSTRAP");
        assert!(!external_rocks_commit_allowed());
        assert!(!rocks_migrate_allowed());
        assert!(!rocks_bootstrap_allowed());
    }

    #[test]
    fn r2_m2_read_token_cannot_mutate_or_admin() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("PLATARIUM_CORE_RPC_INSECURE");
        std::env::remove_var("PLATARIUM_CORE_RPC_TOKEN");
        std::env::set_var("PLATARIUM_CORE_RPC_READ_TOKEN", "read-only");
        std::env::set_var("PLATARIUM_CORE_RPC_MUTATE_TOKEN", "mutate-secret");
        std::env::set_var("PLATARIUM_CORE_ADMIN_TOKEN", "admin-secret");

        assert!(authorize_rpc_method("state_query", Some("read-only")).is_ok());
        assert!(authorize_rpc_method("state_apply_tx", Some("read-only")).is_err());
        assert!(authorize_rpc_method("state_apply_tx", Some("mutate-secret")).is_ok());
        assert!(authorize_rpc_method("state_credit", Some("read-only")).is_err());
        assert!(authorize_rpc_method_with_admin(
            "rocks_commit_block",
            Some("read-only"),
            Some("admin-secret")
        )
        .is_err());
        assert!(authorize_rpc_method_with_admin(
            "rocks_commit_block",
            Some("mutate-secret"),
            Some("admin-secret")
        )
        .is_ok());

        std::env::remove_var("PLATARIUM_CORE_RPC_READ_TOKEN");
        std::env::remove_var("PLATARIUM_CORE_RPC_MUTATE_TOKEN");
        std::env::remove_var("PLATARIUM_CORE_ADMIN_TOKEN");
    }
}
