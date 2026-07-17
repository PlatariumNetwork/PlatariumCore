//! Protocol constants for gas-triggered block assembly (consensus; not env-configurable).

/// Maximum sum of `fee_uplp` (μPLP) per block.
pub const BLOCK_GAS_CAP_UPLP: u64 = 5000;

/// Propose a block when mempool has at least this many transactions.
pub const BLOCK_MIN_TX_COUNT: usize = 1;

/// Propose when mempool aggregate fee reaches this (μPLP).
pub const BLOCK_MIN_GAS_UPLP: u64 = 1;

/// Propose after the oldest mempool tx waited this many seconds.
pub const BLOCK_MAX_WAIT_SEC: i64 = 5;

/// Hard cap on transactions per block.
pub const BLOCK_MAX_TX_COUNT: usize = 500;

/// Gateway faucet pseudo-address (matches PlatariumGatewayGO).
pub const FAUCET_ADDRESS: &str = "faucet";
