use crate::core::asset::Asset;
use crate::core::execution::ExecutionLogic;
use crate::core::state::{State, TREASURY_ADDRESS};
use crate::core::state_file::{save_state_file, state_query_json};
use crate::core::transaction::Transaction;
use crate::modules::contacteconomy::{contact_default_rules, PURPOSE_CONTACT, PURPOSE_DEAL};
use crate::modules::escrow::rules::AddressBindings;
use crate::modules::escrow::types::EscrowStatus;
use crate::modules::escrow::{deal_rules, Escrow};
use std::collections::HashSet;
use std::path::PathBuf;

const AMOUNT: u128 = 2_500_000_000;
const PEACEFUL: u128 = 32_000_000;

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
}

fn bindings(payee: &str, node: &str) -> AddressBindings {
    AddressBindings {
        sender: String::new(),
        receiver: payee.to_string(),
        node: node.to_string(),
        treasury: TREASURY_ADDRESS.to_string(),
        burn: "burn".into(),
    }
}

#[test]
fn deal_lock_stores_fees_and_not_contact_rules() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d1");
    let e = state.get_escrow("d1").unwrap();
    assert_eq!(e.peaceful_fee, PEACEFUL);
    assert_eq!(e.reserved_fee, 64_000_000);
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
fn cancel_restores_creator_and_pays_node_nothing() {
    let (state, creator, beneficiary, node) = funded();
    let before = state.get_balance(&creator);
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-cancel");
    assert!(state
        .escrow_settle(
            &beneficiary,
            "d-cancel",
            AMOUNT,
            0,
            "cancel",
            bindings(&beneficiary, &node),
            "nope",
            None,
        )
        .is_err());
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
    assert_eq!(state.get_balance(&creator), before);
    assert_eq!(state.get_balance(&node), 0);
    assert_eq!(state.get_escrow("d-cancel").unwrap().status, EscrowStatus::Cancelled);
    assert_eq!(state.deal_locked_uplp(&creator), 0);
    assert_eq!(state.deal_receivable_uplp(&beneficiary), 0);
}

#[test]
fn accept_pays_peaceful_fee_and_drops_views() {
    let (state, creator, beneficiary, node) = funded();
    lock_deal(&state, &creator, &beneficiary, &node, AMOUNT, "d-acc");
    let node_before = state.get_balance(&node);
    let ben_before = state.get_balance(&beneficiary);
    assert!(state
        .escrow_settle(
            &creator,
            "d-acc",
            AMOUNT,
            0,
            "accept",
            bindings(&beneficiary, &node),
            "creator",
            None,
        )
        .is_err());
    assert_eq!(state.get_escrow("d-acc").unwrap().status, EscrowStatus::Locked);
    assert!(state
        .escrow_settle(
            &node,
            "d-acc",
            AMOUNT,
            0,
            "accept",
            bindings(&beneficiary, &node),
            "node",
            None,
        )
        .is_err());
    assert_eq!(state.get_balance(&node), node_before);
    assert!(state
        .escrow_settle(
            &TREASURY_ADDRESS.to_string(),
            "d-acc",
            AMOUNT,
            0,
            "accept",
            bindings(&beneficiary, &node),
            "treasury",
            None,
        )
        .is_err());
    state
        .escrow_settle(
            &beneficiary,
            "d-acc",
            AMOUNT,
            0,
            "accept",
            bindings(&beneficiary, &node),
            "yes",
            None,
        )
        .unwrap();
    assert_eq!(state.get_balance(&node), node_before + PEACEFUL);
    assert_eq!(state.get_balance(&beneficiary), ben_before + (AMOUNT - PEACEFUL));
    assert_eq!(
        state.get_balance(&node) + state.get_balance(&beneficiary),
        AMOUNT
    );
    let e = state.get_escrow("d-acc").unwrap();
    assert_eq!(e.status, EscrowStatus::Released);
    assert_eq!(state.deal_locked_uplp(&creator), 0);
    assert_eq!(state.deal_receivable_uplp(&beneficiary), 0);
    assert!(state
        .escrow_settle(
            &beneficiary,
            "d-acc",
            AMOUNT,
            0,
            "accept",
            bindings(&beneficiary, &node),
            "again",
            None,
        )
        .is_err());
    assert_eq!(state.get_balance(&node), node_before + PEACEFUL);
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
    state
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
        .unwrap();
    assert_eq!(state.get_balance(&creator), before_c + (AMOUNT - PEACEFUL));
    assert_eq!(state.get_balance(&node), PEACEFUL);
    assert_eq!(state.get_escrow("d-to").unwrap().status, EscrowStatus::Expired);
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
