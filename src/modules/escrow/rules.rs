//! Configurable escrow settlement rules engine.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::types::{BURN_ROLE, NODE_ROLE, RECEIVER_ROLE, SENDER_ROLE, TREASURY_ROLE};

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum RulesError {
    #[error("empty rule set")]
    Empty,
    #[error("percentages must sum to 100, got {0}")]
    PercentSum(u32),
    #[error("invalid percentage")]
    InvalidPercentage,
    #[error("unknown outcome key: {0}")]
    UnknownOutcome(String),
}

/// One share in a settlement outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleShare {
    /// Role placeholder (`receiver`, `node`, `treasury`, `sender`, `burn`) or concrete address.
    pub address: String,
    /// Percentage 0..=100.
    pub percentage: u8,
}

/// Outcome key → list of shares. Example keys: `accept`, `timeout`, `reject`, `cancel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EscrowRules {
    pub outcomes: std::collections::BTreeMap<String, Vec<RuleShare>>,
}

impl EscrowRules {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_outcome(mut self, key: impl Into<String>, shares: Vec<RuleShare>) -> Self {
        self.outcomes.insert(key.into(), shares);
        self
    }

    pub fn validate_outcome(&self, key: &str) -> Result<(), RulesError> {
        let shares = self
            .outcomes
            .get(key)
            .ok_or_else(|| RulesError::UnknownOutcome(key.to_string()))?;
        if shares.is_empty() {
            return Err(RulesError::Empty);
        }
        let mut sum: u32 = 0;
        for s in shares {
            if s.percentage > 100 {
                return Err(RulesError::InvalidPercentage);
            }
            sum += s.percentage as u32;
        }
        if sum != 100 {
            return Err(RulesError::PercentSum(sum));
        }
        Ok(())
    }

    pub fn hash_hex(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        hex::encode(digest)
    }
}

/// Address bindings for role resolution at settle time.
#[derive(Debug, Clone, Default)]
pub struct AddressBindings {
    pub sender: String,
    pub receiver: String,
    pub node: String,
    pub treasury: String,
    pub burn: String,
}

pub fn resolve_share_address(share_addr: &str, bindings: &AddressBindings) -> Option<String> {
    let t = share_addr.trim();
    if t.is_empty() {
        return None;
    }
    match t {
        SENDER_ROLE => Some(bindings.sender.clone()),
        RECEIVER_ROLE => Some(bindings.receiver.clone()),
        NODE_ROLE => Some(bindings.node.clone()),
        TREASURY_ROLE => Some(bindings.treasury.clone()),
        BURN_ROLE => Some(bindings.burn.clone()),
        _ => Some(t.to_string()),
    }
}

/// Apply rules for `outcome` to `amount`. Returns (address, amount) credits.
/// Remainder from integer division is added to the last share.
pub fn apply_rules(
    rules: &EscrowRules,
    outcome: &str,
    amount: u128,
    bindings: &AddressBindings,
) -> Result<Vec<(String, u128)>, RulesError> {
    rules.validate_outcome(outcome)?;
    let shares = rules.outcomes.get(outcome).unwrap();
    let mut out: Vec<(String, u128)> = Vec::with_capacity(shares.len());
    let mut allocated = 0u128;
    for (i, share) in shares.iter().enumerate() {
        let addr = resolve_share_address(&share.address, bindings)
            .filter(|a| !a.is_empty())
            .ok_or_else(|| RulesError::UnknownOutcome(share.address.clone()))?;
        let mut part = amount.saturating_mul(share.percentage as u128) / 100;
        if i + 1 == shares.len() {
            part = amount.saturating_sub(allocated);
        } else {
            allocated = allocated.saturating_add(part);
        }
        if part > 0 {
            out.push((addr, part));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_rules_sum() {
        let rules = EscrowRules::new().with_outcome(
            "accept",
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
        );
        assert!(rules.validate_outcome("accept").is_ok());
        let bindings = AddressBindings {
            sender: "A".into(),
            receiver: "B".into(),
            node: "N".into(),
            treasury: "treasury".into(),
            burn: "burn".into(),
        };
        let parts = apply_rules(&rules, "accept", 1_000_000, &bindings).unwrap();
        let sum: u128 = parts.iter().map(|(_, a)| a).sum();
        assert_eq!(sum, 1_000_000);
        assert_eq!(parts[0], ("B".into(), 700_000));
        assert_eq!(parts[1], ("N".into(), 200_000));
        assert_eq!(parts[2], ("treasury".into(), 100_000));
    }
}
