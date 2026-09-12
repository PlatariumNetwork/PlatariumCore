//! Finalize failpoint harness (issue #54).
//!
//! Named failpoints inject errors at finalize/persist phases under `cfg(test)`
//! or the `failpoints` Cargo feature. In production builds (neither), every
//! helper is a compile-time no-op so there is zero runtime cost or arming path.

use crate::error::{PlatariumError, Result};

/// Trip before durable persist begins in [`finalize_to_storage`](super::finalize_contract::finalize_to_storage).
pub const FP_FINALIZE_BEFORE_PERSIST: &str = "finalize_before_persist";
/// Trip inside JSON state-file staging `commit_atomic` (save path).
pub const FP_JSON_STAGING_COMMIT: &str = "json_staging_commit";
/// Trip inside Rocks account-engine `commit_atomic` before WriteBatch.
pub const FP_ROCKS_COMMIT_ATOMIC: &str = "rocks_commit_atomic";
/// Trip inside [`RocksStore::write_batch`](crate::storage::rocks::RocksStore::write_batch).
pub const FP_ROCKS_WRITE_BATCH: &str = "rocks_write_batch";

#[cfg(any(test, feature = "failpoints"))]
mod active {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};

    fn armed() -> &'static Mutex<HashSet<String>> {
        static ARMED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        ARMED.get_or_init(|| Mutex::new(HashSet::new()))
    }

    pub fn arm(name: &str) {
        if let Ok(mut g) = armed().lock() {
            g.insert(name.to_string());
        }
    }

    pub fn clear(name: &str) {
        if let Ok(mut g) = armed().lock() {
            g.remove(name);
        }
    }

    pub fn clear_all() {
        if let Ok(mut g) = armed().lock() {
            g.clear();
        }
    }

    pub fn is_armed(name: &str) -> bool {
        armed()
            .lock()
            .map(|g| g.contains(name))
            .unwrap_or(false)
    }
}

/// Arm a named failpoint (no-op in production builds).
pub fn arm(name: &str) {
    #[cfg(any(test, feature = "failpoints"))]
    active::arm(name);
    #[cfg(not(any(test, feature = "failpoints")))]
    let _ = name;
}

/// Clear one armed failpoint (no-op in production builds).
pub fn clear(name: &str) {
    #[cfg(any(test, feature = "failpoints"))]
    active::clear(name);
    #[cfg(not(any(test, feature = "failpoints")))]
    let _ = name;
}

/// Clear all armed failpoints (no-op in production builds).
pub fn clear_all() {
    #[cfg(any(test, feature = "failpoints"))]
    active::clear_all();
}

/// Whether `name` is currently armed (always false in production builds).
pub fn is_armed(name: &str) -> bool {
    #[cfg(any(test, feature = "failpoints"))]
    {
        return active::is_armed(name);
    }
    #[cfg(not(any(test, feature = "failpoints")))]
    {
        let _ = name;
        false
    }
}

/// Hit a named failpoint: Err when armed, Ok otherwise (always Ok in production).
pub fn hit(name: &str) -> Result<()> {
    #[cfg(any(test, feature = "failpoints"))]
    {
        if active::is_armed(name) {
            return Err(PlatariumError::State(format!("failpoint:{name}")));
        }
    }
    let _ = name;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_trips_named_failpoint_and_clears() {
        clear_all();
        assert!(!is_armed(FP_FINALIZE_BEFORE_PERSIST));
        assert!(hit(FP_FINALIZE_BEFORE_PERSIST).is_ok());
        arm(FP_FINALIZE_BEFORE_PERSIST);
        assert!(is_armed(FP_FINALIZE_BEFORE_PERSIST));
        let err = hit(FP_FINALIZE_BEFORE_PERSIST).unwrap_err();
        assert!(err.to_string().contains("failpoint:finalize_before_persist"), "{err}");
        clear(FP_FINALIZE_BEFORE_PERSIST);
        assert!(hit(FP_FINALIZE_BEFORE_PERSIST).is_ok());
        clear_all();
    }
}
