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
        let settler = payee.clone(); // accept must be beneficiary (C5)
        state.set_balance(&locker, 2_000_000);
        state.set_uplp_balance(&locker, 10);
        state.set_uplp_balance(&settler, 10);

        state
            .escrow_lock(
                &locker,
                "eid-1",
                &payee,
                &node, // bind node at lock (C5)
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
        assert_eq!(e.node, node);

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
        assert_eq!(state.get_balance(&payee), 700_000); // fee paid from settler μPLP, not PLP credit
        assert_eq!(state.get_balance(&node), 200_000);
        assert_eq!(state.get_balance(&"treasury".to_string()) >= 100_000, true);
        assert_eq!(state.get_escrow("eid-1").unwrap().status, EscrowStatus::Released);
    }

    #[test]
    fn timeout_refunds_sender() {
        let state = State::new();
        let locker = "locker2".to_string();
        let settler = locker.clone(); // timeout authorized for creator (C5)
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state
            .escrow_lock(
                &locker,
                "eid-2",
                "recv",
                "node2",
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
        let settler = locker.clone(); // legacy empty beneficiary → creator settles reject
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state
            .lock_contact_escrow(&locker, "eid-3", &locker, "node3", 100_000, 1, None)
            .unwrap();
        let before = state.get_balance(&locker);
        state
            .settle_contact_escrow(
                &settler,
                "eid-3",
                100_000,
                1,
                crate::core::contact_escrow::EscrowOutcome::Rejected,
                Some(&locker),
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
        let settler = "R".to_string(); // beneficiary accepts
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 10);
        state.set_balance(&settler, 100);
        state.set_uplp_balance(&settler, 10);
        state
            .escrow_lock(
                &locker,
                "dup",
                &settler,
                "N",
                10_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();
        let bindings = AddressBindings {
            sender: locker.clone(),
            receiver: settler.clone(),
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
        let settler = "R".to_string(); // beneficiary
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&settler, 50);
        state.set_uplp_balance(&settler, 5);
        state
            .escrow_lock(
                &locker,
                "bad-amt",
                &settler,
                "N",
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
                receiver: settler.clone(),
                node: "N".into(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        // C6: failed settle must not burn settler fee
        assert_eq!(state.get_uplp_balance(&settler), 5);
    }

    #[test]
    fn escrow_refund_path() {
        let state = State::new();
        let locker = "Lr".to_string();
        let settler = "Rb".to_string(); // beneficiary rejects/refunds
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&settler, 50);
        state.set_uplp_balance(&settler, 5);
        state
            .escrow_lock(
                &locker,
                "ref-1",
                &settler,
                "node_ref",
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
        assert_eq!(state.get_balance(&"node_ref".to_string()), 20_000);
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
                "node_can",
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

    #[test]
    fn unauthorized_settler_rejected_without_fee_burn() {
        let state = State::new();
        let locker = "Lu".to_string();
        let beneficiary = "Bu".to_string();
        let stranger = "Su".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_balance(&stranger, 100);
        state.set_uplp_balance(&stranger, 10);
        state
            .escrow_lock(
                &locker,
                "unauth-1",
                &beneficiary,
                "N",
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
            &stranger,
            "unauth-1",
            100_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker,
                receiver: beneficiary,
                node: "N".into(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_uplp_balance(&stranger), 10);
        assert_eq!(
            state.get_escrow("unauth-1").unwrap().status,
            EscrowStatus::Locked
        );
    }

    #[test]
    fn settle_payee_redirect_rejected_without_fee_burn() {
        // C5: locked beneficiary cannot be overridden via settle bindings.
        let state = State::new();
        let locker = "Lp".to_string();
        let beneficiary = "Bp".to_string();
        let attacker = "Ap".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_uplp_balance(&beneficiary, 10);
        state
            .escrow_lock(
                &locker,
                "redir-1",
                &beneficiary,
                "N",
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
            &beneficiary,
            "redir-1",
            100_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker,
                receiver: attacker,
                node: "N".into(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_uplp_balance(&beneficiary), 10);
        assert_eq!(
            state.get_escrow("redir-1").unwrap().status,
            EscrowStatus::Locked
        );
    }

    #[test]
    fn settle_node_redirect_rejected_without_fee_burn() {
        // R2-H5: locked node cannot be overridden via settle bindings.
        let state = State::new();
        let locker = "Ln".to_string();
        let beneficiary = "Bn".to_string();
        let locked_node = "NodeLock".to_string();
        let attacker_node = "NodeAttack".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_uplp_balance(&beneficiary, 10);
        state
            .escrow_lock(
                &locker,
                "redir-n",
                &beneficiary,
                &locked_node,
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
            &beneficiary,
            "redir-n",
            100_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker,
                receiver: beneficiary.clone(),
                node: attacker_node,
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_uplp_balance(&beneficiary), 10);
        assert_eq!(
            state.get_escrow("redir-n").unwrap().status,
            EscrowStatus::Locked
        );
    }

    #[test]
    fn unauthorized_cancel_and_refund_rejected() {
        let state = State::new();
        let locker = "Lc2".to_string();
        let beneficiary = "Bc2".to_string();
        let stranger = "Sc2".to_string();
        state.set_balance(&locker, 1_000_000);
        state.set_uplp_balance(&locker, 5);
        state.set_uplp_balance(&stranger, 10);
        state.set_uplp_balance(&beneficiary, 10);
        state
            .escrow_lock(
                &locker,
                "auth-cr",
                &beneficiary,
                "node_cr",
                50_000,
                &Asset::PLP,
                PURPOSE_CONTACT,
                0,
                0,
                "lh",
                1,
                None,
            )
            .unwrap();
        assert!(state.escrow_cancel(&stranger, "auth-cr", 1, None).is_err());
        assert!(state.escrow_cancel(&beneficiary, "auth-cr", 1, None).is_err());
        assert_eq!(state.get_uplp_balance(&stranger), 10);
        assert_eq!(state.get_uplp_balance(&beneficiary), 10);
        assert_eq!(
            state.get_escrow("auth-cr").unwrap().status,
            EscrowStatus::Locked
        );

        // Stranger cannot refund; beneficiary can.
        assert!(state.escrow_refund(&stranger, "auth-cr", 1, None).is_err());
        assert_eq!(state.get_uplp_balance(&stranger), 10);
        state.escrow_refund(&beneficiary, "auth-cr", 1, None).unwrap();
        assert_eq!(
            state.get_escrow("auth-cr").unwrap().status,
            EscrowStatus::Refunded
        );
    }

    #[test]
    fn empty_lock_node_rejected_and_settler_cannot_choose_node() {
        // R2-H5 / C5: contact rules credit node ⇒ node required at lock;
        // settler cannot supply settle_node when lock left it unbound.
        let state = State::new();
        let locker = "LoQa".to_string();
        let beneficiary = "BbQa".to_string();
        state.set_balance(&locker, 2_000_000);
        state.set_uplp_balance(&locker, 20);
        let err = state.escrow_lock(
            &locker,
            "empty-n",
            &beneficiary,
            "",
            100_000,
            &Asset::PLP,
            PURPOSE_CONTACT,
            0,
            0,
            "lh",
            1,
            None,
        );
        assert!(err.is_err(), "contact lock without node must fail");
        // Legacy lock_contact_escrow likewise refuses empty node.
        assert!(state
            .lock_contact_escrow(&locker, "empty-n-legacy", &beneficiary, "", 100_000, 1, None)
            .is_err());

        // Lock correctly, then reject settler-supplied alternate node even if matching payee.
        state
            .escrow_lock(
                &locker,
                "bound-n",
                &beneficiary,
                "HonestNode",
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
        state.set_uplp_balance(&beneficiary, 10);
        let attacker = "AttackerNodeQa".to_string();
        let err = state.escrow_settle(
            &beneficiary,
            "bound-n",
            100_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker.clone(),
                receiver: beneficiary.clone(),
                node: attacker.clone(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_balance(&attacker), 0);
        assert_eq!(
            state.get_escrow("bound-n").unwrap().status,
            EscrowStatus::Locked
        );
    }

    #[test]
    fn empty_lock_beneficiary_rejected_and_settler_cannot_choose_payee() {
        // R2-H5 / C5: contact rules credit receiver ⇒ beneficiary required at lock;
        // settler cannot supply settle_payee when lock left payee unbound (issues #104 / #107).
        let state = State::new();
        let locker = "LoPay".to_string();
        let node = "NodePay".to_string();
        state.set_balance(&locker, 2_000_000);
        state.set_uplp_balance(&locker, 20);
        let err = state.escrow_lock(
            &locker,
            "empty-b",
            "",
            &node,
            100_000,
            &Asset::PLP,
            PURPOSE_CONTACT,
            0,
            0,
            "lh",
            1,
            None,
        );
        assert!(err.is_err(), "contact lock without beneficiary must fail");
        assert!(state
            .lock_contact_escrow(&locker, "empty-b-legacy", "", &node, 100_000, 1, None)
            .is_err());

        // Bound lock: settler-supplied alternate payee rejected.
        let beneficiary = "BenePay".to_string();
        state
            .escrow_lock(
                &locker,
                "bound-b",
                &beneficiary,
                &node,
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
        state.set_uplp_balance(&beneficiary, 10);
        let attacker = "AttackerPayee".to_string();
        let err = state.escrow_settle(
            &beneficiary,
            "bound-b",
            100_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker.clone(),
                receiver: attacker.clone(),
                node: node.clone(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_balance(&attacker), 0);
        assert_eq!(
            state.get_escrow("bound-b").unwrap().status,
            EscrowStatus::Locked
        );
    }

    #[test]
    fn settle_payee_redirect_to_attacker_rejected() {
        // C5 QA: beneficiary accept must not redirect principal via settle bindings.
        let state = State::new();
        let locker = "Lb".to_string();
        let beneficiary = "Bb".to_string();
        let attacker = "Attacker".to_string();
        state.set_balance(&locker, 2_000_000);
        state.set_uplp_balance(&locker, 20);
        state.set_uplp_balance(&beneficiary, 10);
        state
            .escrow_lock(
                &locker,
                "c5-redir",
                &beneficiary,
                "NodeOk",
                1_000_000,
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
            &beneficiary,
            "c5-redir",
            1_000_000,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: locker,
                receiver: attacker.clone(),
                node: attacker.clone(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_balance(&attacker), 0);
        assert_eq!(state.get_balance(&beneficiary), 0);
        assert_eq!(
            state.get_escrow("c5-redir").unwrap().status,
            EscrowStatus::Locked
        );
    }

    #[test]
    fn settle_missing_escrow_does_not_debit_fee() {
        let state = State::new();
        let settler = "Sm".to_string();
        state.set_balance(&settler, 100);
        state.set_uplp_balance(&settler, 10);
        let err = state.escrow_settle(
            &settler,
            "no-such",
            1,
            1,
            OUTCOME_ACCEPT,
            AddressBindings {
                sender: "a".into(),
                receiver: "b".into(),
                node: "n".into(),
                treasury: "treasury".into(),
                burn: "burn".into(),
            },
            "sh",
            None,
        );
        assert!(err.is_err());
        assert_eq!(state.get_uplp_balance(&settler), 10);
        assert_eq!(state.get_balance(&settler), 100);
    }
}
