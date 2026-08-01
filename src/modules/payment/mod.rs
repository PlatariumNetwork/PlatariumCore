//! Payment module — asset-agnostic lock/release/refund/transfer over Core balances.

use crate::core::asset::Asset;
use crate::core::state::{State, Address, TREASURY_ADDRESS};
use crate::error::Result;
use crate::modules::escrow::types::BURN_ROLE;

/// Payment interface for escrow and future payment systems.
pub trait Payment {
    fn lock(
        &self,
        state: &State,
        from: &Address,
        amount: u128,
        asset: &Asset,
        fee_uplp: u128,
        expected_nonce: Option<u64>,
    ) -> Result<()>;

    /// Credit funds out of escrow hold to a recipient (release path).
    fn release(
        &self,
        state: &State,
        to: &Address,
        amount: u128,
        asset: &Asset,
    ) -> Result<()>;

    /// Credit funds back to the original locker (refund path).
    fn refund(
        &self,
        state: &State,
        to: &Address,
        amount: u128,
        asset: &Asset,
    ) -> Result<()>;

    fn credit(&self, state: &State, to: &Address, amount: u128, asset: &Asset) -> Result<()>;

    fn transfer(
        &self,
        state: &State,
        from: &Address,
        to: &Address,
        amount: u128,
        asset: &Asset,
        fee_uplp: u128,
        expected_nonce: Option<u64>,
    ) -> Result<()>;
}

/// PLP payment backend (primary asset today).
pub struct PlpPayment;

impl Payment for PlpPayment {
    fn lock(
        &self,
        state: &State,
        from: &Address,
        amount: u128,
        asset: &Asset,
        fee_uplp: u128,
        expected_nonce: Option<u64>,
    ) -> Result<()> {
        // Debit creator via hold; escrow map tracks the locked amount.
        // Fee → treasury; principal is removed from spendable balance (held in escrow record).
        state.debit_for_escrow_lock(from, asset, amount, fee_uplp, expected_nonce)
    }

    fn release(
        &self,
        state: &State,
        to: &Address,
        amount: u128,
        asset: &Asset,
    ) -> Result<()> {
        self.credit(state, to, amount, asset)
    }

    fn refund(
        &self,
        state: &State,
        to: &Address,
        amount: u128,
        asset: &Asset,
    ) -> Result<()> {
        self.credit(state, to, amount, asset)
    }

    fn credit(&self, state: &State, to: &Address, amount: u128, asset: &Asset) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        state.credit_asset(to, asset, amount);
        Ok(())
    }

    fn transfer(
        &self,
        state: &State,
        from: &Address,
        to: &Address,
        amount: u128,
        asset: &Asset,
        fee_uplp: u128,
        expected_nonce: Option<u64>,
    ) -> Result<()> {
        state.apply_transfer(from, to, asset, amount, fee_uplp, expected_nonce)
    }
}

/// Resolve special sinks used by escrow rules.
pub fn sink_address(role: &str) -> &'static str {
    match role {
        "treasury" => TREASURY_ADDRESS,
        "burn" | BURN_ROLE => "burn",
        _ => TREASURY_ADDRESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::State;

    #[test]
    fn plp_lock_and_credit() {
        let state = State::new();
        let a = "alice".to_string();
        state.set_balance(&a, 1_000_000);
        state.set_uplp_balance(&a, 10);
        let pay = PlpPayment;
        pay.lock(&state, &a, 100_000, &Asset::PLP, 1, Some(0))
            .unwrap();
        assert!(state.get_balance(&a) < 1_000_000);
        pay.credit(&state, &"bob".to_string(), 50_000, &Asset::PLP)
            .unwrap();
        assert_eq!(state.get_balance(&"bob".to_string()), 50_000);
        pay.release(&state, &"carol".to_string(), 10_000, &Asset::PLP)
            .unwrap();
        pay.refund(&state, &a, 5_000, &Asset::PLP).unwrap();
        assert_eq!(state.get_balance(&"carol".to_string()), 10_000);
    }
}
