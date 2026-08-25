//! Asset and currency model for multi-asset transactions. PLP is the base network currency. Fee is always in μPLP and is separate from the transaction asset.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Asset identifier for the transaction amount. Fee is always μPLP and is not represented as an `Asset`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Asset {
    /// Base network currency (Platarium). Amount in minimal units; 1 PLP = 1_000_000 μPLP.
    PLP,
    /// Other token (e.g. "USDT", "NFT:123"). Amount in the token’s minimal units (decimals defined off-chain).
    Token(String),
}

impl Asset {
    /// Contributor reputation token. Accumulate-only (no transfers, purchases later).
    pub const XP_SYMBOL: &'static str = "XP";

    /// `Asset::Token("XP")` — canonical `Token:XP`.
    pub fn xp() -> Self {
        Asset::Token(Self::XP_SYMBOL.to_string())
    }

    /// Returns a canonical string for hashing and ordering (deterministic).
    pub fn as_canonical(&self) -> String {
        match self {
            Asset::PLP => "PLP".to_string(),
            Asset::Token(s) => format!("Token:{}", s),
        }
    }

    /// Soulbound / accumulate-only: cannot move via transfer, escrow, or send.
    pub fn is_non_transferable(&self) -> bool {
        matches!(self, Asset::Token(s) if s.eq_ignore_ascii_case(Self::XP_SYMBOL))
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

    #[test]
    fn token_xp_is_non_transferable() {
        assert_eq!(Asset::xp().as_canonical(), "Token:XP");
        assert!(Asset::xp().is_non_transferable());
        assert!(Asset::Token("xp".into()).is_non_transferable());
        assert!(!Asset::Token("USDT".into()).is_non_transferable());
        assert!(!Asset::PLP.is_non_transferable());
    }
}
