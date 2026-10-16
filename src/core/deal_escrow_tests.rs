use crate::core::asset::Asset;
use crate::core::execution::ExecutionLogic;
use crate::core::state::{State, TREASURY_ADDRESS};
use crate::core::state_file::{save_state_file, state_query_json};
use crate::core::transaction::Transaction;
use crate::modules::contacteconomy::{contact_default_rules, PURPOSE_CONTACT, PURPOSE_DEAL};
use crate::modules::escrow::rules::AddressBindings;
use crate::modules::escrow::types::EscrowStatus;
use crate::modules::escrow::{
    deal_rules, hash_deal_terms, DEAL_PEACEFUL_HOLD, PANEL_GENERATION_SECOND, PANEL_INSTANCE_FIRST,
    Escrow,
};
use std::collections::HashSet;
use std::path::PathBuf;

const AMOUNT: u128 = 2_500_000_000;
const PEACEFUL: u128 = 42_000_000;

fn funded() -> (State, String, String, String) {
    let state = State::new();
    let creator = "creator".to_string();
    let beneficiary = "beneficiary".to_string();
    let node = "node".to_string();
    state.set_balance(&creator, 10_000_000_000);
    (state, creator, beneficiary, node)
}

fn lock_deal(state: &State, creator: &str, beneficiary: &str, node: &str, amount: u128, id: &str) {
    state
        .escrow_lock(
            &creator.to_string(),
            id,
            beneficiary,
            node,
            amount,
            &Asset::PLP,
            PURPOSE_DEAL,
            1_000,
            0,
            "lock",
            0,
            None,
        )
        .unwrap();
    state.deal_store_terms(id, b"plain terms").unwrap();
}

fn bindings(payee: &str, node: &str) -> AddressBindings {
    AddressBindings {
        sender: String::new(),
        receiver: payee.to_string(),
        node: node.to_string(),
        treasury: TREASURY_ADDRESS.to_string(),
        burn: "burn".into(),
        volunteer_majority: Vec::new(),
                terms: Vec::new(),
        }
}

fn majority_bindings(payee: &str, node: &str, keys: &[&str]) -> AddressBindings {
    let mut b = bindings(payee, node);
    b.volunteer_majority = keys.iter().map(|k| (*k).to_string()).collect();
    b
}

fn terms_bindings(payee: &str, node: &str, terms: &[u8]) -> AddressBindings {
    let mut b = bindings(payee, node);
    b.terms = terms.to_vec();
    b
}

fn confirm_accept(
    state: &State,
    who: &str,
    id: &str,
    amount: u128,
    beneficiary: &str,
    node: &str,
    hash: &str,
) -> Result<(), crate::error::PlatariumError> {
    state.escrow_settle(
        &who.to_string(),
        id,
        amount,
        0,
        "accept",
        bindings(beneficiary, node),
        hash,
        None,
    )
}

#[test]
fn deal_lock_stores_fees_and_not_contact_rules() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d1");
    let e = state.get_escrow("d1").unwrap();
    assert_eq!(e.peaceful_fee, PEACEFUL);
    assert_eq!(e.reserved_fee, 84_000_000);
    assert_ne!(e.rules_hash, contact_default_rules().hash_hex());
    assert_eq!(e.rules_hash, deal_rules().hash_hex());
    assert_eq!(state.get_balance(&creator), before - AMOUNT);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(state.get_balance(&beneficiary), 0);
    assert_eq!(state.deal_locked_uplp(&creator), AMOUNT);
    assert_eq!(state.deal_receivable_uplp(&beneficiary), AMOUNT);
    assert_eq!(state.get_balance(&beneficiary), 0);
}

#[test]
fn contact_lock_leaves_deal_fees_at_zero() {
    let state = State::new();
    let creator = "c".to_string();
    state.set_balance(&creator, 1_000);
    state
        .escrow_lock(
            &creator,
            "c1",
            "ben",
            "node",
            100,
            &Asset::PLP,
            PURPOSE_CONTACT,
            0,
            0,
            "",
            0,
            None,
        )
        .unwrap();
    let e = state.get_escrow("c1").unwrap();
    assert_eq!(e.peaceful_fee, 0);
    assert_eq!(e.reserved_fee, 0);
    assert_eq!(e.rules_hash, contact_default_rules().hash_hex());
}

#[test]
fn empty_node_and_non_plp_and_thin_amount_do_not_debit() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    assert!(state
        .escrow_lock(
            &creator,
            "bad-node",
            &beneficiary,
            "",
            AMOUNT,
            &Asset::PLP,
            PURPOSE_DEAL,
            1_000,
            0,
            "",
            0,
            None,
        )
        .is_err());
    assert!(state
        .escrow_lock(
            &creator,
            "bad-asset",
            &beneficiary,
            &node,
            AMOUNT,
            &Asset::Token("USDT".into()),
            PURPOSE_DEAL,
            1_000,
            0,
            "",
            0,
            None,
        )
        .is_err());
    assert!(state
        .escrow_lock(
            &creator,
            "thin",
            &beneficiary,
            &node,
            1,
            &Asset::PLP,
            PURPOSE_DEAL,
            1_000,
            0,
            "",
            0,
            None,
        )
        .is_err());
    assert_eq!(state.get_balance(&creator), before);
    assert!(state.get_escrow("bad-node").is_none());
}

#[test]
fn one_cancel_does_not_pay() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-cancel");
    state
        .escrow_settle(
            &creator,
            "d-cancel",
            AMOUNT,
            0,
            "cancel",
            bindings(&beneficiary, &node),
            "ok",
            None,
        )
        .unwrap();
    assert_eq!(state.get_balance(&creator), before - AMOUNT);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(state.get_escrow("d-cancel").unwrap().status, EscrowStatus::Locked);
    assert!(state.get_escrow("d-cancel").unwrap().creator_cancelled);
    assert_eq!(state.deal_locked_uplp(&creator), AMOUNT);
}

#[test]
fn one_confirmation_does_not_close_or_pay() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-acc");
    let node_before = state.get_balance(&node);
    let ben_before = state.get_balance(&beneficiary);
    let creator_after_lock = state.get_balance(&creator);
    confirm_accept(&state, &creator, "d-acc", AMOUNT, &beneficiary, &node, "creator").unwrap();
    let e = state.get_escrow("d-acc").unwrap();
    assert!(e.creator_confirmed);
    assert!(!e.beneficiary_confirmed);
    assert_eq!(e.status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&creator), creator_after_lock);
    assert_eq!(state.get_balance(&node), node_before);
    assert_eq!(state.get_balance(&beneficiary), ben_before);
    assert!(confirm_accept(&state, &node, "d-acc", AMOUNT, &beneficiary, &node, "node").is_err());
    assert!(confirm_accept(
        &state,
        TREASURY_ADDRESS,
        "d-acc",
        AMOUNT,
        &beneficiary,
        &node,
        "treasury",
    )
    .is_err());
    confirm_accept(&state, &beneficiary, "d-acc", AMOUNT, &beneficiary, &node, "ben").unwrap();
    let e = state.get_escrow("d-acc").unwrap();
    assert!(e.beneficiary_confirmed);
    assert_eq!(e.status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), node_before);
    assert_eq!(state.get_balance(&beneficiary), ben_before);
}

#[test]
fn mutual_confirm_pays_after_hold_and_not_before() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-hold");
    confirm_accept(&state, &creator, "d-hold", AMOUNT, &beneficiary, &node, "c").unwrap();
    state.set_logical_time(10);
    confirm_accept(&state, &beneficiary, "d-hold", AMOUNT, &beneficiary, &node, "b").unwrap();
    assert_eq!(state.get_escrow("d-hold").unwrap().pending_close_at, 10);
    assert_eq!(state.get_balance(&node), 0);
    state.set_logical_time(10 + DEAL_PEACEFUL_HOLD - 1);
    assert!(confirm_accept(&state, &beneficiary, "d-hold", AMOUNT, &beneficiary, &node, "early").is_err());
    assert_eq!(state.get_escrow("d-hold").unwrap().status, EscrowStatus::Locked);
    state.set_logical_time(10 + DEAL_PEACEFUL_HOLD);
    confirm_accept(&state, &beneficiary, "d-hold", AMOUNT, &beneficiary, &node, "pay").unwrap();
    assert_eq!(state.get_balance(&node), PEACEFUL);
    assert_eq!(state.get_balance(&beneficiary), AMOUNT - PEACEFUL);
    assert_eq!(state.get_escrow("d-hold").unwrap().status, EscrowStatus::Released);
    assert_eq!(state.deal_locked_uplp(&creator), 0);
    assert!(confirm_accept(&state, &beneficiary, "d-hold", AMOUNT, &beneficiary, &node, "again").is_err());
}

#[test]
fn confirmation_after_expiry_still_does_not_close() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-exp");
    state.set_logical_time(2_000);
    confirm_accept(&state, &beneficiary, "d-exp", AMOUNT, &beneficiary, &node, "late").unwrap();
    let e = state.get_escrow("d-exp").unwrap();
    assert!(e.beneficiary_confirmed);
    assert_eq!(e.status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(state.get_balance(&beneficiary), 0);
}

#[test]
fn redirect_and_timeout_rules() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-to");
    let before_c = state.get_balance(&creator);
    assert!(state
        .escrow_settle(
            &beneficiary,
            "d-to",
            AMOUNT,
            0,
            "accept",
            bindings("someone-else", &node),
            "redir",
            None,
        )
        .is_err());
    assert!(state
        .escrow_settle(
            &beneficiary,
            "d-to",
            AMOUNT,
            0,
            "accept",
            bindings(&beneficiary, "other-node"),
            "redir-node",
            None,
        )
        .is_err());
    assert_eq!(state.get_escrow("d-to").unwrap().status, EscrowStatus::Locked);
    state.set_logical_time(1_000);
    assert!(state
        .escrow_settle(
            &node,
            "d-to",
            AMOUNT,
            0,
            "timeout",
            bindings(&beneficiary, &node),
            "early",
            None,
        )
        .is_err());
    assert_eq!(state.get_balance(&creator), before_c);
    state.set_logical_time(1_001);
    assert!(state
        .escrow_settle(
            &node,
            "d-to",
            AMOUNT,
            0,
            "timeout",
            bindings(&beneficiary, &node),
            "late",
            None,
        )
        .is_err());
    assert_eq!(state.get_balance(&creator), before_c);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(state.get_escrow("d-to").unwrap().status, EscrowStatus::Locked);
    confirm_accept(&state, &creator, "d-to", AMOUNT, &beneficiary, &node, "one").unwrap();
    assert_eq!(state.get_escrow("d-to").unwrap().status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), 0);
}

#[test]
fn old_escrow_json_defaults_fees_to_zero() {
    let raw = r#"{"escrow_id":"e","creator":"c","beneficiary":"b","amount":1,"asset":"PLP","purpose":"contact","status":"Locked","rules":{"outcomes":{}},"rules_hash":"","created_at":0,"expires_at":0,"lock_tx_hash":"","settle_tx_hash":""}"#;
    let e: Escrow = serde_json::from_str(raw).unwrap();
    assert_eq!(e.peaceful_fee, 0);
    assert_eq!(e.reserved_fee, 0);
}

#[test]
fn query_json_reports_locked_and_receivable_beside_spendable() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-q");
    let path: PathBuf = std::env::temp_dir().join("platarium-deal-query-test.json");
    save_state_file(&path, &state).unwrap();
    let body = state_query_json(&path, &beneficiary, "PLP").unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["balance"], "0");
    assert_eq!(v["receivable_uplp"], AMOUNT.to_string());
    assert_eq!(v["locked_uplp"], "0");
    let creator_body = state_query_json(&path, &creator, "PLP").unwrap();
    let c: serde_json::Value = serde_json::from_str(&creator_body).unwrap();
    assert_eq!(c["locked_uplp"], AMOUNT.to_string());
    assert_eq!(c["balance"], (10_000_000_000u128 - AMOUNT).to_string());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn execution_routes_deal_lock_and_leaves_transfer_alone() {
    let (state, creator, beneficiary, node) = funded();
    let mut tx = Transaction::new(
        creator.clone(),
        beneficiary.clone(),
        Asset::PLP,
        AMOUNT,
        0,
        0,
        HashSet::new(),
        HashSet::new(),
        "sig".into(),
        "sig2".into(),
    )
    .unwrap();
    tx.tx_kind = Some("escrow_lock".into());
    tx.escrow_id = Some("exec-1".into());
    tx.purpose = Some(PURPOSE_DEAL.into());
    tx.settle_payee = Some(beneficiary.clone());
    tx.settle_node = Some(node.clone());
    tx.expires_at = Some(1_000);
    tx.hash = tx.compute_hash().unwrap();
    ExecutionLogic::apply_transaction_effects(&state, &tx).unwrap();
    assert!(state.get_escrow("exec-1").is_some());
    assert_eq!(state.get_balance(&beneficiary), 0);

    let mut plain = Transaction::new(
        creator.clone(),
        beneficiary.clone(),
        Asset::PLP,
        10,
        0,
        1,
        HashSet::new(),
        HashSet::new(),
        "sig".into(),
        "sig2".into(),
    )
    .unwrap();
    plain.hash = plain.compute_hash().unwrap();
    state.set_balance(&creator, 100);
    ExecutionLogic::apply_transaction_effects(&state, &plain).unwrap();
    assert_eq!(state.get_balance(&beneficiary), 10);
}

#[test]
fn treasury_can_still_settle_contact() {
    let state = State::new();
    let creator = "c".to_string();
    let ben = "b".to_string();
    state.set_balance(&creator, 1_000);
    state
        .escrow_lock(
            &creator,
            "contact-1",
            &ben,
            "n",
            1_000,
            &Asset::PLP,
            PURPOSE_CONTACT,
            0,
            0,
            "",
            0,
            None,
        )
        .unwrap();
    state
        .escrow_settle(
            &TREASURY_ADDRESS.to_string(),
            "contact-1",
            1_000,
            0,
            "accept",
            bindings(&ben, "n"),
            "t",
            None,
        )
        .unwrap();
    assert_eq!(state.get_escrow("contact-1").unwrap().status, EscrowStatus::Released);
}

const RESERVED: u128 = 84_000_000;
const PREMIUM: u128 = 42_000_000;

fn dispute(
    state: &State,
    who: &str,
    id: &str,
    key: &str,
    keys: &[&str],
    beneficiary: &str,
    node: &str,
    hash: &str,
) -> Result<(), crate::error::PlatariumError> {
    state.escrow_settle(
        &who.to_string(),
        id,
        AMOUNT,
        0,
        key,
        majority_bindings(beneficiary, node, keys),
        hash,
        None,
    )
}

#[test]
fn dispute_fulfilled_pays_peaceful_fee_and_splits_premium() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-ful");
    let keys = ["v2", "v1", "v3"];
    assert!(dispute(
        &state, &node, "d-ful", "dispute-fulfilled", &keys, &beneficiary, &node, "node",
    )
    .is_err());
    dispute(
        &state, &beneficiary, "d-ful", "dispute-fulfilled", &keys, &beneficiary, &node, "open",
    )
    .unwrap();
    assert_eq!(state.get_escrow("d-ful").unwrap().status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), 0);
    state.set_logical_time(DEAL_PEACEFUL_HOLD - 1);
    assert!(dispute(
        &state, &creator, "d-ful", "dispute-fulfilled", &keys, &beneficiary, &node, "early",
    )
    .is_err());
    state.set_logical_time(DEAL_PEACEFUL_HOLD);
    dispute(
        &state, &creator, "d-ful", "dispute-fulfilled", &keys, &beneficiary, &node, "pay",
    )
    .unwrap();
    assert_eq!(state.get_balance(&node), PEACEFUL);
    assert_eq!(state.get_balance(&beneficiary), AMOUNT - RESERVED);
    assert_eq!(state.get_balance(&"v1".to_string()) + state.get_balance(&"v2".to_string()) + state.get_balance(&"v3".to_string()), PREMIUM);
    assert_eq!(state.get_balance(&"v1".to_string()), PREMIUM / 3);
    assert_eq!(state.get_balance(&"v3".to_string()), PREMIUM / 3);
    assert_eq!(state.get_escrow("d-ful").unwrap().status, EscrowStatus::Released);
    assert_eq!(state.deal_receivable_uplp(&beneficiary), 0);
}

#[test]
fn five_matching_volunteers_share_the_same_premium() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-five");
    let keys = ["a", "b", "c", "d", "e"];
    dispute(
        &state, &creator, "d-five", "dispute-fulfilled", &keys, &beneficiary, &node, "open",
    )
    .unwrap();
    state.set_logical_time(DEAL_PEACEFUL_HOLD);
    dispute(
        &state, &creator, "d-five", "dispute-fulfilled", &keys, &beneficiary, &node, "pay",
    )
    .unwrap();
    let mut sum = 0u128;
    for k in keys {
        sum += state.get_balance(&k.to_string());
    }
    assert_eq!(sum, PREMIUM);
    assert_eq!(state.get_balance(&node), PEACEFUL);
}

#[test]
fn dispute_not_fulfilled_refunds_creator_minus_reserved() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-nf");
    let keys = ["m1", "m2", "m3"];
    assert!(dispute(
        &state, &creator, "d-nf", "dispute-not-fulfilled", &keys, &beneficiary, &node, "self",
    )
    .is_ok());
    assert_eq!(state.get_balance(&creator), before - AMOUNT);
    state.set_logical_time(DEAL_PEACEFUL_HOLD);
    dispute(
        &state, &beneficiary, "d-nf", "dispute-not-fulfilled", &keys, &beneficiary, &node, "pay",
    )
    .unwrap();
    assert_eq!(state.get_balance(&node), PEACEFUL);
    assert_eq!(state.get_balance(&creator), before - RESERVED);
    assert_eq!(state.get_balance(&beneficiary), 0);
    assert_eq!(state.get_escrow("d-nf").unwrap().status, EscrowStatus::Refunded);
    assert_eq!(state.deal_receivable_uplp(&beneficiary), 0);
}

#[test]
fn expiry_block_leaves_the_deal_locked() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-exp2");
    state.set_logical_time(1_000);
    assert_eq!(state.get_escrow("d-exp2").unwrap().status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), 0);
    state.set_logical_time(1_001);
    assert_eq!(state.get_escrow("d-exp2").unwrap().status, EscrowStatus::Locked);
    assert_eq!(state.deal_locked_uplp(&creator), AMOUNT);
    assert_eq!(state.get_balance(&node), 0);
}

#[test]
fn second_settle_after_release_is_rejected() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-2");
    confirm_accept(&state, &creator, "d-2", AMOUNT, &beneficiary, &node, "c").unwrap();
    confirm_accept(&state, &beneficiary, "d-2", AMOUNT, &beneficiary, &node, "b").unwrap();
    state.set_logical_time(DEAL_PEACEFUL_HOLD);
    confirm_accept(&state, &beneficiary, "d-2", AMOUNT, &beneficiary, &node, "pay").unwrap();
    let node_paid = state.get_balance(&node);
    assert!(confirm_accept(&state, &creator, "d-2", AMOUNT, &beneficiary, &node, "again").is_err());
    assert_eq!(state.get_balance(&node), node_paid);
    assert_eq!(state.get_escrow("d-2").unwrap().status, EscrowStatus::Released);
}

#[test]
fn complaint_blocks_a_peaceful_release() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-cmp");
    let lock_hash = hash_deal_terms(b"plain terms");
    assert_eq!(state.get_escrow("d-cmp").unwrap().terms_hash, lock_hash);
    state
        .escrow_settle(
            &creator,
            "d-cmp",
            AMOUNT,
            0,
            "complaint",
            terms_bindings(&beneficiary, &node, b"plain terms"),
            "c1",
            None,
        )
        .unwrap();
    confirm_accept(&state, &creator, "d-cmp", AMOUNT, &beneficiary, &node, "c").unwrap();
    confirm_accept(&state, &beneficiary, "d-cmp", AMOUNT, &beneficiary, &node, "b").unwrap();
    state.set_logical_time(DEAL_PEACEFUL_HOLD);
    let _ = confirm_accept(&state, &beneficiary, "d-cmp", AMOUNT, &beneficiary, &node, "pay");
    assert_eq!(state.get_escrow("d-cmp").unwrap().status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(state.get_balance(&beneficiary), 0);
    state.set_logical_time(2_000);
    assert_eq!(state.get_escrow("d-cmp").unwrap().status, EscrowStatus::Locked);
    let keys = ["v1", "v2", "v3"];
    dispute(
        &state, &beneficiary, "d-cmp", "dispute-fulfilled", &keys, &beneficiary, &node, "open",
    )
    .unwrap();
    state.set_logical_time(2_000 + DEAL_PEACEFUL_HOLD);
    dispute(
        &state, &beneficiary, "d-cmp", "dispute-fulfilled", &keys, &beneficiary, &node, "pay",
    )
    .unwrap();
    assert_eq!(state.get_escrow("d-cmp").unwrap().status, EscrowStatus::Released);
}

#[test]
fn mismatch_terms_complaint_is_rejected() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-hash");
    assert!(state
        .escrow_settle(
            &creator,
            "d-hash",
            AMOUNT,
            0,
            "complaint",
            terms_bindings(&beneficiary, &node, b"plain terms "),
            "trim",
            None,
        )
        .is_err());
    assert!(state
        .escrow_settle(
            &creator,
            "d-hash",
            AMOUNT,
            0,
            "complaint",
            terms_bindings(&beneficiary, &node, "plain terms\u{0301}".as_bytes()),
            "norm",
            None,
        )
        .is_err());
    assert_eq!(state.get_escrow("d-hash").unwrap().status, EscrowStatus::Locked);
    assert_eq!(state.get_balance(&node), 0);
}

#[test]
fn either_party_files_one_appeal() {
    let (state, creator, beneficiary, node) = funded();
    state.set_balance(&beneficiary, PREMIUM);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-ap");
    confirm_accept(&state, &creator, "d-ap", AMOUNT, &beneficiary, &node, "c").unwrap();
    confirm_accept(&state, &beneficiary, "d-ap", AMOUNT, &beneficiary, &node, "b").unwrap();
    let creator_after = state.get_balance(&creator);
    state
        .escrow_settle(
            &creator,
            "d-ap",
            AMOUNT,
            0,
            "appeal",
            terms_bindings(&beneficiary, &node, b"plain terms"),
            "a1",
            None,
        )
        .unwrap();
    assert_eq!(state.get_balance(&creator), creator_after - PREMIUM);
    assert_eq!(state.get_escrow("d-ap").unwrap().appeal_held, PREMIUM);
    assert_eq!(state.deal_locked_uplp(&creator), AMOUNT);
    assert!(state
        .escrow_settle(
            &beneficiary,
            "d-ap",
            AMOUNT,
            0,
            "appeal",
            terms_bindings(&beneficiary, &node, b"plain terms"),
            "a2",
            None,
        )
        .is_err());
    assert!(state
        .escrow_settle(
            &node,
            "d-ap",
            AMOUNT,
            0,
            "appeal",
            terms_bindings(&beneficiary, &node, b"plain terms"),
            "n",
            None,
        )
        .is_err());
    state.set_logical_time(DEAL_PEACEFUL_HOLD);
    assert!(confirm_accept(&state, &beneficiary, "d-ap", AMOUNT, &beneficiary, &node, "pay").is_err());
}

#[test]
fn mutual_cancel_stores_deadline_without_paying() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-mc");
    state.set_logical_time(50);
    state
        .escrow_settle(
            &creator,
            "d-mc",
            AMOUNT,
            0,
            "cancel",
            bindings(&beneficiary, &node),
            "c1",
            None,
        )
        .unwrap();
    assert_eq!(state.get_balance(&creator), before - AMOUNT);
    state
        .escrow_settle(
            &beneficiary,
            "d-mc",
            AMOUNT,
            0,
            "cancel",
            bindings(&beneficiary, &node),
            "c2",
            None,
        )
        .unwrap();
    let e = state.get_escrow("d-mc").unwrap();
    assert_eq!(e.pending_outcome, "cancel");
    assert_eq!(e.pending_close_at, 50);
    assert_eq!(e.pending_close_at + DEAL_PEACEFUL_HOLD, 50 + DEAL_PEACEFUL_HOLD);
    assert_eq!(state.get_balance(&creator), before - AMOUNT);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(e.status, EscrowStatus::Locked);
}

#[test]
fn second_panel_after_no_result_shares_no_key() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-p2");
    let first = ["p1", "p2", "p3"];
    dispute(
        &state, &creator, "d-p2", "dispute-fulfilled", &first, &beneficiary, &node, "open",
    )
    .unwrap();
    let locked = state.get_balance(&creator);
    state
        .escrow_settle(
            &creator,
            "d-p2",
            AMOUNT,
            0,
            "panel-no-result",
            bindings(&beneficiary, &node),
            "nr",
            None,
        )
        .unwrap();
    assert_eq!(state.get_balance(&creator), locked);
    assert_eq!(state.get_escrow("d-p2").unwrap().status, EscrowStatus::Locked);
    let second = ["q1", "q2", "q3", "q4", "q5"];
    state
        .escrow_settle(
            &beneficiary,
            "d-p2",
            AMOUNT,
            0,
            "seat-second-panel",
            majority_bindings(&beneficiary, &node, &second),
            "seat",
            None,
        )
        .unwrap();
    let e = state.get_escrow("d-p2").unwrap();
    assert_eq!(e.panel_instance, PANEL_INSTANCE_FIRST);
    assert_eq!(e.panel_generation, PANEL_GENERATION_SECOND);
    assert_eq!(e.second_panel, ["q1", "q2", "q3", "q4", "q5"]);
    for k in first {
        assert!(!e.second_panel.iter().any(|s| s == k));
    }
    assert_eq!(state.get_balance(&creator), locked);
    let overlap = majority_bindings(&beneficiary, &node, &["p1", "r2", "r3", "r4", "r5"]);
    assert!(state
        .escrow_settle(
            &creator,
            "d-p2",
            AMOUNT,
            0,
            "seat-second-panel",
            overlap,
            "again",
            None,
        )
        .is_err());
}
