//! Generic Escrow Engine — reusable financial primitive for Platarium.
//!
//! Applications (contact messaging, P2P deals, ads) set `purpose` and rules.
//! No social-graph semantics live here.

pub mod rules;
pub mod types;

#[cfg(test)]
mod engine_tests;

pub use rules::{apply_rules, resolve_share_address, AddressBindings, EscrowRules, RuleShare, RulesError};
pub use types::{
    Escrow, EscrowError, EscrowStatus, TX_KIND_ESCROW_CANCEL, TX_KIND_ESCROW_LOCK,
    TX_KIND_ESCROW_REFUND, TX_KIND_ESCROW_SETTLE, TREASURY_ROLE, NODE_ROLE, RECEIVER_ROLE,
    SENDER_ROLE, BURN_ROLE,
};
