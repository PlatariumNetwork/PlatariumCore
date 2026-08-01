//! Compatibility shim: contact escrow now uses the generic Escrow Engine.
//! Prefer `crate::modules::escrow` and `crate::modules::contacteconomy`.

pub use crate::modules::contacteconomy::{
    contact_default_rules, OUTCOME_ACCEPT, OUTCOME_REJECT, OUTCOME_TIMEOUT, PURPOSE_CONTACT,
};
pub use crate::modules::escrow::types::{
    Escrow, EscrowError, EscrowStatus, TX_KIND_ESCROW_CANCEL, TX_KIND_ESCROW_LOCK,
    TX_KIND_ESCROW_REFUND, TX_KIND_ESCROW_SETTLE, BURN_ROLE as BURN_ADDRESS,
};

/// Legacy outcome codes (u8) mapped to rules keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EscrowOutcome {
    Accepted = 0,
    Timeout = 1,
    Rejected = 2,
}

impl EscrowOutcome {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Accepted),
            1 => Some(Self::Timeout),
            2 => Some(Self::Rejected),
            _ => None,
        }
    }

    pub fn as_rules_key(self) -> &'static str {
        match self {
            Self::Accepted => OUTCOME_ACCEPT,
            Self::Timeout => OUTCOME_TIMEOUT,
            Self::Rejected => OUTCOME_REJECT,
        }
    }
}

/// Legacy aliases for tx kinds (accepted in from_gateway_json).
pub const TX_KIND_CONTACT_ESCROW_LOCK: &str = TX_KIND_ESCROW_LOCK;
pub const TX_KIND_CONTACT_ESCROW_SETTLE: &str = TX_KIND_ESCROW_SETTLE;

/// Deprecated thin view of escrow for old callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactEscrowEntry {
    pub locker: String,
    pub amount_uplp: u128,
    pub status: EscrowStatus,
}

impl From<&Escrow> for ContactEscrowEntry {
    fn from(e: &Escrow) -> Self {
        Self {
            locker: e.creator.clone(),
            amount_uplp: e.amount,
            status: e.status,
        }
    }
}
