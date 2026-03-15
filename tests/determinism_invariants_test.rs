//! **Step 10 — Integration and Invariant Testing (Determinism Tests)**
//!
//! Property-based style invariants for the Consensus Engine:
//! - Same inputs → same validator selection result
//! - Same seed → same committee
//! - Same TX and state → same state_root
//! - No float, RNG, or system time in the consensus path (documented and asserted by double-run equality)

use platarium_core::*;
use std::collections::HashSet;

// ========== Invariant 1: Same inputs → same validator selection result ==========

#[test]
fn test_invariant_same_inputs_same_validator_selection() {
    let registry = NodeRegistry::new();
    for i in 0..15 {
        registry
            .register(
                format!("n{}", i),
                format!("pk{}", i),
                1000 + i as u128 * 100,
                10,
            )
            .unwrap();
    }

    let block_number = 42u64;
    let global_entropy = b"prev_finalized_block_hash_bytes";
    let current_tps = 50u64;
    let capacity = 100u64;

    let (l1_a, l2_a) =
        select_l1_l2_validators(&registry, block_number, global_entropy, current_tps, capacity)
            .unwrap();
    let (l1_b, l2_b) =
        select_l1_l2_validators(&registry, block_number, global_entropy, current_tps, capacity)
            .unwrap();

    assert_eq!(l1_a, l1_b, "same inputs must yield same L1 selection");
    assert_eq!(l2_a, l2_b, "same inputs must yield same L2 selection");
}

#[test]
fn test_invariant_same_inputs_same_select_validators() {
    let registry = NodeRegistry::new();
    for i in 0..10 {
        registry
            .register(format!("n{}", i), format!("pk{}", i), 2000, 10)
            .unwrap();
    }

    let l1_a = select_validators(&registry, 20, 100, 1, b"entropy").unwrap();
    let l1_b = select_validators(&registry, 20, 100, 1, b"entropy").unwrap();

    assert_eq!(l1_a, l1_b, "same inputs must yield same validator list");
}

// ========== Invariant 2: Same seed → same committee ==========

#[test]
fn test_invariant_same_seed_same_committee() {
    let registry = NodeRegistry::new();
    for i in 0..20 {
        registry
            .register(format!("node_{}", i), format!("pk{}", i), 1500, 10)
            .unwrap();
    }

    let seed = committee_selection_seed(7, b"global_entropy_here");
    let percent = 20u64;

    let committee_a = select_validators_with_percent(&registry, &seed, percent).unwrap();
    let committee_b = select_validators_with_percent(&registry, &seed, percent).unwrap();

    assert_eq!(committee_a, committee_b, "same seed and percent must yield same committee");
    assert!(!committee_a.is_empty());
}

#[test]
fn test_invariant_same_seed_same_committee_multiple_runs() {
    let registry = NodeRegistry::new();
    registry.register("a".into(), "pka".into(), 5000, 10).unwrap();
    registry.register("b".into(), "pkb".into(), 5000, 10).unwrap();
    registry.register("c".into(), "pkc".into(), 5000, 10).unwrap();

    let seed = compute_seed(100, b"prev_block_hash");
    let pct = selection_percent_from_load(10, 100).unwrap();

    let c1 = select_validators_with_percent(&registry, &seed, pct).unwrap();
    let c2 = select_validators_with_percent(&registry, &seed, pct).unwrap();
    let c3 = select_validators_with_percent(&registry, &seed, pct).unwrap();

    assert_eq!(c1, c2);
    assert_eq!(c2, c3);
}

// ========== Invariant 3: Same TX and state → same state_root ==========

#[test]
fn test_invariant_same_state_same_state_root() {
    let state1 = State::new();
    state1.set_balance(&"alice".to_string(), 1000);
    state1.set_uplp_balance(&"alice".to_string(), 10);
    state1.set_nonce(&"alice".to_string(), 0);
    state1.set_balance(&"bob".to_string(), 2000);
    state1.set_nonce(&"bob".to_string(), 5);

    let snap1 = state1.snapshot();
    let root1 = snap1.compute_state_root();
    let root2 = snap1.compute_state_root();

    assert_eq!(root1, root2, "same snapshot must yield same state_root");
}

#[test]
fn test_invariant_same_state_contents_same_state_root_two_states() {
    let state1 = State::new();
    state1.set_balance(&"addr_z".to_string(), 100);
    state1.set_uplp_balance(&"addr_z".to_string(), 1);
    state1.set_nonce(&"addr_z".to_string(), 0);
    state1.set_balance(&"addr_a".to_string(), 200);
    state1.set_nonce(&"addr_a".to_string(), 1);

    let state2 = State::new();
    state2.set_balance(&"addr_a".to_string(), 200);
    state2.set_nonce(&"addr_a".to_string(), 1);
    state2.set_balance(&"addr_z".to_string(), 100);
    state2.set_uplp_balance(&"addr_z".to_string(), 1);
    state2.set_nonce(&"addr_z".to_string(), 0);

    let root1 = state1.snapshot().compute_state_root();
    let root2 = state2.snapshot().compute_state_root();

    assert_eq!(
        root1, root2,
        "same state contents (different insertion order) must yield same state_root"
    );
}

#[test]
fn test_invariant_same_tx_hash_deterministic() {
    let reads = HashSet::from(["r1".to_string(), "r2".to_string()]);
    let writes = HashSet::from(["w1".to_string()]);

    let tx1 = Transaction::new(
        "from".to_string(),
        "to".to_string(),
        Asset::PLP,
        100,
        1,
        0,
        reads.clone(),
        writes.clone(),
        "sig_m".to_string(),
        "sig_d".to_string(),
    )
    .unwrap();
    let tx2 = Transaction::new(
        "from".to_string(),
        "to".to_string(),
        Asset::PLP,
        100,
        1,
        0,
        reads,
        writes,
        "sig_m".to_string(),
        "sig_d".to_string(),
    )
    .unwrap();

    assert_eq!(tx1.hash, tx2.hash, "same TX data must yield same hash");
}

// ========== Invariant 4: Consensus path determinism (no float, RNG, system time) ==========

#[test]
fn test_invariant_consensus_path_double_run_identical() {
    // Run a mini consensus path twice: seed → selection → state snapshot → state_root.
    // If any float/RNG/time leaked in, results could differ.

    let registry = NodeRegistry::new();
    for i in 0..12 {
        registry
            .register(format!("v{}", i), format!("pk{}", i), 3000, 10)
            .unwrap();
    }

    let block_number = 5u64;
    let global_entropy = b"deterministic_entropy_for_test";
    let seed = compute_seed(block_number, global_entropy);
    let percent = selection_percent_from_load(30, 100).unwrap();

    let committee_run1 = select_validators_with_percent(&registry, &seed, percent).unwrap();
    let committee_run2 = select_validators_with_percent(&registry, &seed, percent).unwrap();

    assert_eq!(committee_run1, committee_run2);

    let state = State::new();
    state.set_balance(&"x".to_string(), 5000);
    state.set_uplp_balance(&"x".to_string(), 100);
    state.set_nonce(&"x".to_string(), 0);

    let root_run1 = state.snapshot().compute_state_root();
    let root_run2 = state.snapshot().compute_state_root();

    assert_eq!(root_run1, root_run2);
}

#[test]
fn test_invariant_fee_calculation_integer_only() {
    // Fee path must be integer-only (no float); same input → same output
    let fees: Vec<u64> = (0..=100)
        .map(|pct| calculate_fee_from_load(pct))
        .collect();
    let fees_again: Vec<u64> = (0..=100)
        .map(|pct| calculate_fee_from_load(pct))
        .collect();
    assert_eq!(fees, fees_again);
    assert!(fees.iter().all(|&f| f >= 1 && f <= 5));
}

#[test]
fn test_invariant_merkle_root_deterministic() {
    let hashes = vec![
        "aa".to_string(),
        "bb".to_string(),
        "cc".to_string(),
    ];
    let root1 = compute_merkle_root(&hashes);
    let root2 = compute_merkle_root(&hashes);
    assert_eq!(root1, root2);

    let hashes_reordered = vec![
        "cc".to_string(),
        "aa".to_string(),
        "bb".to_string(),
    ];
    let root3 = compute_merkle_root(&hashes_reordered);
    assert_eq!(root1, root3, "Merkle root must be order-independent (sorted internally)");
}

#[test]
fn test_invariant_block_leader_deterministic() {
    let l2 = vec!["L2_0".into(), "L2_1".into(), "L2_2".into()];
    let leader_0 = block_leader_for_height(0, &l2);
    let leader_0_again = block_leader_for_height(0, &l2);
    assert_eq!(leader_0, leader_0_again);
    assert_eq!(block_leader_for_height(3, &l2), block_leader_for_height(3, &l2));
}

#[test]
fn test_invariant_l1_l2_confirmation_deterministic() {
    let votes: Vec<(String, Vote)> = vec![
        ("n1".into(), Vote::Confirm),
        ("n2".into(), Vote::Confirm),
        ("n3".into(), Vote::Reject),
    ];
    let (res1, pen1) = process_l1_confirmation(&votes).unwrap();
    let (res2, pen2) = process_l1_confirmation(&votes).unwrap();
    assert_eq!(res1, res2);
    assert_eq!(pen1, pen2);

    let l2_votes: Vec<(String, Vote)> = (0..10)
        .map(|i| (format!("n{}", i), if i < 7 { Vote::Confirm } else { Vote::Reject }))
        .collect();
    let (b1, p1) = process_l2_block_votes(&l2_votes).unwrap();
    let (b2, p2) = process_l2_block_votes(&l2_votes).unwrap();
    assert_eq!(b1, b2);
    assert_eq!(p1, p2);
}
