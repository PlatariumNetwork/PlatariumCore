// Asset module
// Currency / token model for multi-asset transactions
//
// INVARIANTS:
// - PLP = base network currency
// - Fee is ALWAYS in μPLP and is NOT an Asset; Asset does not affect fee
// - No system time or randomness

use serde::{Deserialize, Serialize};
use std::fmt;

/// Asset identifier for transaction amount.
/// Fee is always μPLP and is separate from Asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Asset {
    /// Base network currency (Platarium).
    /// Amount in minimal units (μPLP); 1 PLP = 1_000_000 μPLP.
    PLP,

    /// Other token, e.g. "USDT", "NFT:123".
    /// Amount in token's minimal units (decimals defined off-chain).
    Token(String),
}

impl Asset {
    /// Canonical string for hashing/ordering (deterministic).
    pub fn as_canonical(&self) -> String {
        match self {
            Asset::PLP => "PLP".to_string(),
            Asset::Token(s) => format!("Token:{}", s),
        }
    }
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_canonical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_plp_canonical() {
        assert_eq!(Asset::PLP.as_canonical(), "PLP");
    }

    #[test]
    fn test_asset_token_canonical() {
        assert_eq!(Asset::Token("USDT".to_string()).as_canonical(), "Token:USDT");
    }

    #[test]
    fn test_asset_equality() {
        assert_eq!(Asset::PLP, Asset::PLP);
        assert_eq!(Asset::Token("A".to_string()), Asset::Token("A".to_string()));
        assert_ne!(Asset::PLP, Asset::Token("PLP".to_string()));
    }
}
