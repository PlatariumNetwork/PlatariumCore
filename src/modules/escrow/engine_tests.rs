//! Unit tests for generic Escrow Engine + contact purpose rules.

#[cfg(test)]
mod escrow_engine_tests {
    use crate::core::asset::Asset;
    use crate::core::state::State;
    use crate::modules::contacteconomy::{
        contact_default_rules, OUTCOME_ACCEPT, OUTCOME_REJECT, OUTCOME_TIMEOUT, PURPOSE_CONTACT,
    };
    use crate::modules::escrow::rules::AddressBindings;
    use crate::modules::escrow::types::EscrowStatus;
    use crate::modules::payment::{Payment, PlpPayment};

    #[test]
    fn create_lock_release_accept() {
        let state = State::new();
        let locker = "locker".to_string();
        let payee = "payee".to_string();
        let node = "node_op".to_string();
        let settler = "settler".to_string();
        state.set_balance(&locker, 2_000_000);
        state.set_uplp_balance(&locker, 10);
        state.set_balance(&settler, 100);
        state.set_uplp_balance(&settler, 10);

        state
            .escrow_lock(
                &locker,
                "eid-1",
                &payee,
                1_000_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lockhash",
                1,
                Some(0),
            )
            .expect("lock");
        let e = state.get_escrow("eid-1").unwrap();
        assert_eq!(e.status, EscrowStatus::Locked);
        assert_eq!(e.purpose, PURPOSE_CONTACT);
        assert!(!e.rules_hash.is_empty());

        let bindings = AddressBindings {
            sender: locker.clone(),
            receiver: payee.clone(),
            node: node.clone(),
            treasury: "treasury".into(),
            burn: "burn".into(),
        };
        state
            .escrow_settle(
                &settler,
                "eid-1",
                1_000_000,
                1,
                OUTCOME_ACCEPT,
                bindings,
                "settlehash",
                Some(0),
            )
            .expect("settle");
        assert_eq!(state.get_balance(&payee), 700_000);
        assert_eq!(state.get_balance(&node), 200_000);
        assert_eq!(state.get_balance(&"treasury".to_string()) >= 100_000, true);
        assert_eq!(state.get_escrow("eid-1").unwrap().status, EscrowStatus::Released);
    }

    #[test]
    fn timeout_refunds_sender() {
        let state = State::new();
        let locker = "locker2".to_string();
        let settler = "settler2".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&settler, 50);
        state.set_uplp_balance(&settler, 5);
        state
            .escrow_lock(
                &locker,
                "eid-2",
                "recv",
                500_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();
        let before = state.get_balance(&locker);
        state
            .escrow_settle(
                &settler,
                "eid-2",
                500_000,
                1,
                OUTCOME_TIMEOUT,
                AddressBindings {
                    sender: locker.clone(),
                    receiver: "recv".into(),
                    node: "node2".into(),
                    treasury: "treasury".into(),
                    burn: "burn".into(),
                },
                "sh",
                None,
            )
            .unwrap();
        assert_eq!(state.get_balance(&locker), before + 450_000);
        assert_eq!(state.get_balance(&"node2".to_string()), 50_000);
        assert_eq!(state.get_escrow("eid-2").unwrap().status, EscrowStatus::Expired);
    }

    #[test]
    fn reject_refunds_majority() {
        let state = State::new();
        let locker = "locker3".to_string();
        let settler = "settler3".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&settler, 50);
        state.set_uplp_balance(&settler, 5);
        state
            .lock_contact_escrow(&locker, "eid-3", 100_000, 1, None)
            .unwrap();
        let before = state.get_balance(&locker);
        state
            .settle_contact_escrow(
                &settler,
                "eid-3",
                100_000,
                1,
                crate::core::contact_escrow::EscrowOutcome::Rejected,
                None,
                "node3",
                None,
            )
            .unwrap();
        assert_eq!(state.get_balance(&locker), before + 80_000);
        assert_eq!(state.get_balance(&"node3".to_string()), 10_000);
    }

    #[test]
    fn invalid_double_settle() {
        let state = State::new();
        let locker = "L".to_string();
        let settler = "S".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 10);
        state.set_balance(&settler, 100);
        state.set_uplp_balance(&settler, 10);
        state
            .lock_contact_escrow(&locker, "dup", 10_000, 1, None)
            .unwrap();
        let bindings = AddressBindings {
            sender: locker.clone(),
            receiver: "R".into(),
            node: "N".into(),
            treasury: "treasury".into(),
            burn: "burn".into(),
        };
        state
            .escrow_settle(&settler, "dup", 10_000, 1, OUTCOME_ACCEPT, bindings.clone(), "a", None)
            .unwrap();
        assert!(state
            .escrow_settle(&settler, "dup", 10_000, 1, OUTCOME_ACCEPT, bindings, "b", None)
            .is_err());
    }

    #[test]
    fn contact_rules_hash_stable() {
        let a = contact_default_rules().hash_hex();
        let b = contact_default_rules().hash_hex();
        assert_eq!(a, b);
        assert!(a.len() == 64);
    }

    #[test]
    fn payment_module_lock() {
        let state = State::new();
        let a = "alice".to_string();
        state.set_balance(&a, 500_000);
        state.set_uplp_balance(&a, 5);
        PlpPayment
            .lock(&state, &a, 100_000, &Asset::PLP, 1, Some(0))
            .unwrap();
        assert!(state.get_balance(&a) < 500_000);
    }

    #[test]
    fn reject_outcome_key() {
        assert_eq!(OUTCOME_REJECT, "reject");
        assert_eq!(OUTCOME_TIMEOUT, "timeout");
        assert_eq!(OUTCOME_ACCEPT, "accept");
    }

    #[test]
    fn invalid_amount_settle() {
        let state = State::new();
        let locker = "Lx".to_string();
        let settler = "Sx".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&settler, 50);
        state.set_uplp_balance(&settler, 5);
        state
            .escrow_lock(
                &locker,
                "bad-amt",
                "R",
                100_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();
        let err = state.escrow_settle(
            &settler,
            "bad-amt",
            99_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker,
                receiver: "R".into(),
                node: "N".into(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
    }

    #[test]
    fn escrow_refund_path() {
        let state = State::new();
        let locker = "Lr".to_string();
        let settler = "Sr".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&settler, 50);
        state.set_uplp_balance(&settler, 5);
        state
            .escrow_lock(
                &locker,
                "ref-1",
                "Rb",
                200_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();
        let before = state.get_balance(&locker);
        state.escrow_refund(&settler, "ref-1", 1, None).unwrap();
        assert_eq!(state.get_balance(&locker), before + 160_000); // 80% reject rule
        assert_eq!(
            state.get_escrow("ref-1").unwrap().status,
            EscrowStatus::Refunded
        );
    }

    #[test]
    fn escrow_cancel_uses_reject_when_no_cancel_rules() {
        let state = State::new();
        let locker = "Lc".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state
            .escrow_lock(
                &locker,
                "can-1",
                "Rb",
                100_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();
        state.escrow_cancel(&locker, "can-1", 1, None).unwrap();
        let st = state.get_escrow("can-1").unwrap().status;
        assert!(matches!(st, EscrowStatus::Refunded | EscrowStatus::Cancelled));
    }
}
