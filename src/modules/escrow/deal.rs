//! Deal purpose: marginal node fee, not a share of principal.

use sha2::{Digest, Sha256};

use super::rules::{EscrowRules, RuleShare};
use super::types::{RECEIVER_ROLE, SENDER_ROLE};

pub const PURPOSE_DEAL: &str = "deal";
pub const PANEL_INSTANCE_FIRST: u8 = 0x01;
pub const PANEL_GENERATION_FIRST: u8 = 0x01;
pub const PANEL_GENERATION_SECOND: u8 = 0x02;

/// SHA-256 of the exact bytes. Does not trim and does not normalize Unicode.
pub fn hash_deal_terms(terms: &[u8]) -> String {
    hex::encode(Sha256::digest(terms))
}

/// 1 PLP = 1_000_000 μPLP.
pub const MICRO_PLP_PER_PLP: u128 = 1_000_000;

struct Bracket {
    /// `None` consumes the remainder.
    width: Option<u128>,
    bps: u32,
}

/// First 1,000 PLP at 180 bps, next 9,000 at 160, next 90,000 at 140,
/// next 900,000 at 120, remainder at 100.
fn peaceful_brackets() -> &'static [Bracket] {
    const W1K: u128 = 1_000 * MICRO_PLP_PER_PLP;
    const W9K: u128 = 9_000 * MICRO_PLP_PER_PLP;
    const W90K: u128 = 90_000 * MICRO_PLP_PER_PLP;
    const W900K: u128 = 900_000 * MICRO_PLP_PER_PLP;
    &[
        Bracket { width: Some(W1K), bps: 180 },
        Bracket { width: Some(W9K), bps: 160 },
        Bracket { width: Some(W90K), bps: 140 },
        Bracket { width: Some(W900K), bps: 120 },
        Bracket { width: None, bps: 100 },
    ]
}

/// Integer marginal peaceful fee. Dispute fee is exactly twice that after rounding.
pub fn deal_fee_uplp(amount: u128, disputed: bool) -> u128 {
    if amount == 0 {
        return 0;
    }
    let mut left = amount;
    let mut fee = 0u128;
    for b in peaceful_brackets() {
        if left == 0 {
            break;
        }
        let slice = match b.width {
            Some(w) => left.min(w),
            None => left,
        };
        fee = fee.saturating_add(slice.saturating_mul(b.bps as u128) / 10_000);
        left -= slice;
    }
    if fee == 0 {
        fee = 1;
    }
    if disputed {
        fee.saturating_mul(2)
    } else {
        fee
    }
}

/// Mutual-confirm and first-instance verdict credits wait this many logical-time units.
pub const DEAL_PEACEFUL_HOLD: u64 = 86_400;

/// Split `premium` among majority keys. Remainder μPLP goes one at a time in sort order.
pub fn split_volunteer_premium(premium: u128, keys: &[String]) -> Vec<(String, u128)> {
    let mut ks: Vec<String> = keys
        .iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    ks.sort();
    ks.dedup();
    if ks.is_empty() || premium == 0 {
        return Vec::new();
    }
    let n = ks.len() as u128;
    let share = premium / n;
    let rem = premium % n;
    ks.into_iter()
        .enumerate()
        .map(|(i, k)| {
            let extra = if (i as u128) < rem { 1 } else { 0 };
            (k, share + extra)
        })
        .collect()
}

/// Settlement marker. Accept credits are applied from `peaceful_fee`, not these percents.
pub fn deal_rules() -> EscrowRules {
    EscrowRules::new()
        .with_outcome(
            "accept",
            vec![RuleShare {
                address: RECEIVER_ROLE.into(),
                percentage: 100,
            }],
        )
        .with_outcome(
            "timeout",
            vec![RuleShare {
                address: SENDER_ROLE.into(),
                percentage: 100,
            }],
        )
        .with_outcome(
            "cancel",
            vec![RuleShare {
                address: SENDER_ROLE.into(),
                percentage: 100,
            }],
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::contacteconomy::contact_default_rules;

    #[test]
    fn purpose_deal_is_not_contact() {
        assert_eq!(PURPOSE_DEAL, "deal");
        assert_ne!(PURPOSE_DEAL, crate::modules::contacteconomy::PURPOSE_CONTACT);
    }

    #[test]
    fn dispute_fee_is_twice_rounded_peaceful() {
        let amount = 2_500 * MICRO_PLP_PER_PLP;
        let peaceful = deal_fee_uplp(amount, false);
        assert_eq!(peaceful, 42 * MICRO_PLP_PER_PLP);
        assert_eq!(deal_fee_uplp(amount, true), peaceful * 2);
        assert_eq!(deal_fee_uplp(amount, true), 84 * MICRO_PLP_PER_PLP);
        let million = 1_000_000 * MICRO_PLP_PER_PLP;
        let just_over = million + MICRO_PLP_PER_PLP;
        let fee_m = deal_fee_uplp(million, false);
        let fee_over = deal_fee_uplp(just_over, false);
        assert_eq!(fee_over - fee_m, MICRO_PLP_PER_PLP * 100 / 10_000);
        let one_k = 1_000 * MICRO_PLP_PER_PLP;
        let ten_k = 10_000 * MICRO_PLP_PER_PLP;
        assert!(deal_fee_uplp(ten_k, false) < deal_fee_uplp(one_k, false) * 10);
    }

    #[test]
    fn zero_amount_is_free() {
        assert_eq!(deal_fee_uplp(0, false), 0);
        assert_eq!(deal_fee_uplp(0, true), 0);
    }

    #[test]
    fn peaceful_and_dispute_fee_for_2500_plp() {
        let amount = 2_500_000_000u128;
        assert_eq!(deal_fee_uplp(amount, false), 42_000_000);
        assert_eq!(deal_fee_uplp(amount, true), 84_000_000);
    }

    #[test]
    fn one_micro_floors_to_one() {
        assert_eq!(deal_fee_uplp(1, false), 1);
        assert_eq!(deal_fee_uplp(1, true), 2);
    }

    #[test]
    fn bracket_does_not_reprice_the_whole_amount() {
        let at = 1_000 * MICRO_PLP_PER_PLP;
        let fee_at = deal_fee_uplp(at, false);
        let fee_next = deal_fee_uplp(at + 1, false);
        let step = 1u128 * 160 / 10_000;
        assert_eq!(fee_at, at * 180 / 10_000);
        assert_eq!(fee_next, fee_at + step);
        assert!(fee_next - fee_at <= step);
    }

    #[test]
    fn deal_rules_hash_differs_from_contact() {
        assert_ne!(deal_rules().hash_hex(), contact_default_rules().hash_hex());
    }

    #[test]
    fn leftover_micro_goes_lexicographic() {
        let keys = vec!["c".into(), "a".into(), "b".into()];
        let parts = split_volunteer_premium(32, &keys);
        assert_eq!(
            parts,
            vec![
                ("a".into(), 11u128),
                ("b".into(), 11u128),
                ("c".into(), 10u128),
            ]
        );
        let five = vec!["e".into(), "d".into(), "c".into(), "b".into(), "a".into()];
        let parts5 = split_volunteer_premium(32, &five);
        assert_eq!(parts5.iter().map(|(_, n)| *n).sum::<u128>(), 32);
        assert_eq!(parts5[0], ("a".into(), 7));
        assert_eq!(parts5[4], ("e".into(), 6));
    }
}
