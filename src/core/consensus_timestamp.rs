//! Consensus timestamp gates (issues #71 / #72).
//!
//! On consensus-critical Core paths, block timestamps must be:
//! 1. **Strictly monotonic** vs the previous tip timestamp (`timestamp > previous_timestamp`)
//! 2. **Within [`MAX_DRIFT`](crate::core::consensus_params::MAX_DRIFT)** of the caller's
//!    local wall clock (`timestamp <= local_time + MAX_DRIFT`)
//!
//! Local wall clock is operator-supplied (`now_unix` / `local_time`); it is not
//! treated as unchecked consensus time — see [`crate::core::protocol_notes`].

use crate::core::consensus_params::MAX_DRIFT;
use crate::error::{PlatariumError, Result};

/// Reject non-monotonic consensus timestamps (issue #71).
pub fn validate_timestamp_monotonic(timestamp: i64, previous_timestamp: i64) -> Result<()> {
    if timestamp <= previous_timestamp {
        return Err(PlatariumError::State(format!(
            "non-monotonic consensus timestamp: timestamp={timestamp} previous_timestamp={previous_timestamp} (require timestamp > previous_timestamp)"
        )));
    }
    Ok(())
}

/// Reject consensus timestamps too far ahead of local wall clock (issue #72).
///
/// Rule: `timestamp > local_time + MAX_DRIFT` → rejected.
pub fn validate_timestamp_drift(timestamp: i64, local_time: i64) -> Result<()> {
    let max_allowed = local_time.saturating_add(MAX_DRIFT);
    if timestamp > max_allowed {
        return Err(PlatariumError::State(format!(
            "consensus timestamp exceeds MAX_DRIFT: timestamp={timestamp} local_time={local_time} MAX_DRIFT={MAX_DRIFT} max_allowed={max_allowed}"
        )));
    }
    Ok(())
}

/// Full consensus timestamp check: monotonicity + drift (issues #71 / #72).
pub fn validate_consensus_timestamp(
    timestamp: i64,
    previous_timestamp: i64,
    local_time: i64,
) -> Result<()> {
    validate_timestamp_monotonic(timestamp, previous_timestamp)?;
    validate_timestamp_drift(timestamp, local_time)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_monotonic_timestamp() {
        // Issue #71: equal or older previous → rejected.
        let err = validate_timestamp_monotonic(100, 100).unwrap_err();
        assert!(
            err.to_string().contains("non-monotonic"),
            "{err}"
        );
        let err = validate_timestamp_monotonic(99, 100).unwrap_err();
        assert!(
            err.to_string().contains("non-monotonic"),
            "{err}"
        );
        assert!(validate_timestamp_monotonic(101, 100).is_ok());
        assert!(validate_timestamp_monotonic(1, 0).is_ok());
    }

    #[test]
    fn rejects_excessive_drift_against_wall_clock() {
        // Issue #72: timestamp > local_time + MAX_DRIFT → rejected.
        let local = 1_000_000i64;
        let ok_ts = local + MAX_DRIFT;
        assert!(validate_timestamp_drift(ok_ts, local).is_ok());
        let bad_ts = local + MAX_DRIFT + 1;
        let err = validate_timestamp_drift(bad_ts, local).unwrap_err();
        assert!(
            err.to_string().contains("MAX_DRIFT"),
            "{err}"
        );
        assert!(MAX_DRIFT > 0, "MAX_DRIFT must be documented positive constant");
    }

    #[test]
    fn combined_validate_covers_both_gates() {
        assert!(validate_consensus_timestamp(101, 100, 100).is_ok());
        assert!(validate_consensus_timestamp(100, 100, 100).is_err());
        assert!(validate_consensus_timestamp(100 + MAX_DRIFT + 1, 0, 100).is_err());
    }
}
