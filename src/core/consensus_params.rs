//! Protocol constants for gas-triggered block assembly (consensus; not env-configurable).

/// Maximum sum of `fee_uplp` (μPLP) per block.
/// 500 000 μPLP ≈ 50 000 txs at FEE_UPLP=10 — effectively unlimited for testnet.
pub const BLOCK_GAS_CAP_UPLP: u64 = 500_000;

/// Propose a block when mempool has at least this many transactions.
pub const BLOCK_MIN_TX_COUNT: usize = 1;

/// Propose when mempool aggregate fee reaches this (μPLP).
pub const BLOCK_MIN_GAS_UPLP: u64 = 1;

/// Propose after the oldest mempool tx waited this many seconds.
pub const BLOCK_MAX_WAIT_SEC: i64 = 5;

/// Hard cap on transactions per block.
pub const BLOCK_MAX_TX_COUNT: usize = 10_000;

/// Gateway faucet pseudo-address (matches PlatariumGatewayGO).
pub const FAUCET_ADDRESS: &str = "faucet";

/// Max how far ahead of the contiguous tip a mempool tx nonce may be.
/// Allows parallel HTTP submits after Gateway `/api/nonce/allocate` without
/// requiring in-order arrival. Packing still requires consecutive nonces.
pub const MEMPOOL_MAX_NONCE_GAP: u64 = 64;

/// Maximum allowed positive drift of a consensus timestamp ahead of the
/// caller's local wall clock (seconds). Issue #72.
///
/// Consensus-critical paths reject when
/// `timestamp > local_wall_clock + MAX_DRIFT`. This is **not** env-configurable.
/// See [`crate::core::consensus_timestamp::validate_consensus_timestamp`].
pub const MAX_DRIFT: i64 = 30;
