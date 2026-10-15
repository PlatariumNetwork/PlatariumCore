//! Deal purpose: marginal node fee, not a share of principal.

use super::rules::{EscrowRules, RuleShare};
use super::types::{RECEIVER_ROLE, SENDER_ROLE};

pub const PURPOSE_DEAL: &str = "deal";

/// 1 PLP = 1_000_000 μPLP.
pub const MICRO_PLP_PER_PLP: u128 = 1_000_000;

struct Bracket {
    /// `None` consumes the remainder.
    width: Option<u128>,
    bps: u32,
}

/// First 100 PLP at 50 bps, next 900 at 100, next 9000 at 150, rest at 200.
fn peaceful_brackets() -> &'static [Bracket] {
    const W100: u128 = 100 * MICRO_PLP_PER_PLP;
    const W900: u128 = 900 * MICRO_PLP_PER_PLP;
    const W9000: u128 = 9000 * MICRO_PLP_PER_PLP;
    &[
        Bracket { width: Some(W100), bps: 50 },
        Bracket { width: Some(W900), bps: 100 },
        Bracket { width: Some(W9000), bps: 150 },
        Bracket { width: None, bps: 200 },
    ]
}

fn dispute_brackets() -> &'static [Bracket] {
    const W100: u128 = 100 * MICRO_PLP_PER_PLP;
    const W900: u128 = 900 * MICRO_PLP_PER_PLP;
    const W9000: u128 = 9000 * MICRO_PLP_PER_PLP;
    &[
        Bracket { width: Some(W100), bps: 100 },
        Bracket { width: Some(W900), bps: 200 },
        Bracket { width: Some(W9000), bps: 300 },
        Bracket { width: None, bps: 400 },
    ]
}

/// Integer marginal fee. A positive amount that rounds to 0 still costs 1 μPLP.
pub fn deal_fee_uplp(amount: u128, disputed: bool) -> u128 {
    if amount == 0 {
        return 0;
    }
    let table = if disputed {
        dispute_brackets()
    } else {
        peaceful_brackets()
    };
    let mut left = amount;
    let mut fee = 0u128;
    for b in table {
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
        1
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
    fn dispute_rates_are_double_and_capped() {
        let p = peaceful_brackets();
        let d = dispute_brackets();
        assert_eq!(p.len(), d.len());
        for (a, b) in p.iter().zip(d.iter()) {
            assert_eq!(a.width, b.width);
            assert_eq!(b.bps, a.bps * 2);
        }
        assert_eq!(d.last().unwrap().bps, 400);
        let width_sum: u128 = p.iter().filter_map(|b| b.width).sum();
        assert_eq!(width_sum, 10_000 * MICRO_PLP_PER_PLP);
    }

    #[test]
    fn zero_amount_is_free() {
        assert_eq!(deal_fee_uplp(0, false), 0);
        assert_eq!(deal_fee_uplp(0, true), 0);
    }

    #[test]
    fn peaceful_and_dispute_fee_for_2500_plp() {
        let amount = 2_500_000_000u128;
        assert_eq!(deal_fee_uplp(amount, false), 32_000_000);
        assert_eq!(deal_fee_uplp(amount, true), 64_000_000);
    }

    #[test]
    fn one_micro_floors_to_one() {
        assert_eq!(deal_fee_uplp(1, false), 1);
        assert_eq!(deal_fee_uplp(1, true), 1);
    }

    #[test]
    fn bracket_does_not_reprice_the_whole_amount() {
        let at = 100 * MICRO_PLP_PER_PLP;
        let fee_at = deal_fee_uplp(at, false);
        let fee_next = deal_fee_uplp(at + 1, false);
        let step = 1u128 * 100 / 10_000;
        assert_eq!(fee_at, at * 50 / 10_000);
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
