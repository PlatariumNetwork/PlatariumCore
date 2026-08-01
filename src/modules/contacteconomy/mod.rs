//! Contact-economy application presets for the generic Escrow Engine.
//!
//! Owns contact purpose rules only — not social graph (that lives on Gateway).

use crate::modules::escrow::rules::{EscrowRules, RuleShare};
use crate::modules::escrow::types::{NODE_ROLE, RECEIVER_ROLE, SENDER_ROLE, TREASURY_ROLE};

pub const PURPOSE_CONTACT: &str = "contact";

pub const OUTCOME_ACCEPT: &str = "accept";
pub const OUTCOME_TIMEOUT: &str = "timeout";
pub const OUTCOME_REJECT: &str = "reject";

/// Default contact messaging settlement rules (percentages).
///
/// Accept: 70% receiver · 20% node · 10% treasury  
/// Timeout: 90% sender · 10% node  
/// Reject: 80% sender · 10% node · 10% treasury
pub fn contact_default_rules() -> EscrowRules {
    EscrowRules::new()
        .with_outcome(
            OUTCOME_ACCEPT,
            vec![
                RuleShare {
                    address: RECEIVER_ROLE.into(),
                    percentage: 70,
                },
                RuleShare {
                    address: NODE_ROLE.into(),
                    percentage: 20,
                },
                RuleShare {
                    address: TREASURY_ROLE.into(),
                    percentage: 10,
                },
            ],
        )
        .with_outcome(
            OUTCOME_TIMEOUT,
            vec![
                RuleShare {
                    address: SENDER_ROLE.into(),
                    percentage: 90,
                },
                RuleShare {
                    address: NODE_ROLE.into(),
                    percentage: 10,
                },
            ],
        )
        .with_outcome(
            OUTCOME_REJECT,
            vec![
                RuleShare {
                    address: SENDER_ROLE.into(),
                    percentage: 80,
                },
                RuleShare {
                    address: NODE_ROLE.into(),
                    percentage: 10,
                },
                RuleShare {
                    address: TREASURY_ROLE.into(),
                    percentage: 10,
                },
            ],
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_rules_valid() {
        let r = contact_default_rules();
        assert!(r.validate_outcome(OUTCOME_ACCEPT).is_ok());
        assert!(r.validate_outcome(OUTCOME_TIMEOUT).is_ok());
        assert!(r.validate_outcome(OUTCOME_REJECT).is_ok());
    }
}
