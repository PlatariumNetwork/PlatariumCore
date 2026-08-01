//! Escrow types and status machine.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::rules::EscrowRules;

pub const TX_KIND_ESCROW_LOCK: &str = "escrow_lock";
pub const TX_KIND_ESCROW_SETTLE: &str = "escrow_settle";
pub const TX_KIND_ESCROW_REFUND: &str = "escrow_refund";
pub const TX_KIND_ESCROW_CANCEL: &str = "escrow_cancel";

/// Role placeholders resolved at settle time via address bindings.
pub const SENDER_ROLE: &str = "sender";
pub const RECEIVER_ROLE: &str = "receiver";
pub const NODE_ROLE: &str = "node";
pub const TREASURY_ROLE: &str = "treasury";
pub const BURN_ROLE: &str = "burn";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EscrowStatus {
    Created = 0,
    Locked = 1,
    Released = 2,
    Refunded = 3,
    Expired = 4,
    Cancelled = 5,
}

impl EscrowStatus {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Created),
            1 => Some(Self::Locked),
            2 => Some(Self::Released),
            3 => Some(Self::Refunded),
            4 => Some(Self::Expired),
            5 => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Released | Self::Refunded | Self::Expired | Self::Cancelled
        )
    }
}

/// Universal escrow record (financial only — no social graph).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Escrow {
    pub escrow_id: String,
    pub creator: String,
    pub beneficiary: String,
    pub amount: u128,
    /// Asset canonical string (e.g. "PLP").
    pub asset: String,
    /// Application purpose tag, e.g. "contact".
    pub purpose: String,
    pub status: EscrowStatus,
    pub rules: EscrowRules,
    /// Deterministic hash of rules (hex).
    pub rules_hash: String,
    /// Logical creation time (seconds); set by lock tx / protocol, not wall clock in Core tests.
    pub created_at: u64,
    pub expires_at: u64,
    pub lock_tx_hash: String,
    pub settle_tx_hash: String,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum EscrowError {
    #[error("escrow already exists")]
    AlreadyExists,
    #[error("escrow not found")]
    NotFound,
    #[error("invalid escrow status: {0:?}")]
    InvalidStatus(EscrowStatus),
    #[error("amount mismatch: locked {locked}, got {got}")]
    AmountMismatch { locked: u128, got: u128 },
    #[error("escrow expired")]
    Expired,
    #[error("invalid rules: {0}")]
    InvalidRules(String),
    #[error("missing address binding for role {0}")]
    MissingBinding(String),
    #[error("creator mismatch")]
    CreatorMismatch,
    #[error("replay: settle_tx already set")]
    Replay,
}
