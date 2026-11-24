use aod_core::bigint::BigInt;
use aod_core::error::CoreError;
use aod_core::ledger::{
    probability, HingeSlide, Kernel, LedgerRow, RouteStatus, SadarPromotion, TunnelBranchInput,
    TunnelClip,
};
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Q4, T1};

#[test]
fn support_form_exact_pi_rd_pressure_overhang_and_yro_firewall() {
    let one = BigInt::one();
    let fixtures = [
        ("1:2:2", 4, 9),
        ("3:2:4", 10, 21),
        ("3:3:6", 30, 61),
        ("3:4:6", 68, 137),
        ("1:2:9_yro", 4, 9),
    ];
    for (spec, exp_pi, exp_rd) in fixtures {
        let sf = SupportForm::parse(spec).unwrap();
        assert!(!sf.is_closure_certificate());
        assert_eq!(sf.pi().unwrap(), BigInt::from_i64(exp_pi));
        assert_eq!(sf.rd(&one).unwrap(), BigInt::from_i64(exp_rd));
    }

    // Periodic sync fixture from Manual: lcm(137, 61) == 8357
    let rd34 = SupportForm::parse("3:4:6").unwrap().rd(&one).unwrap();
    let rd33 = SupportForm::parse("3:3:6").unwrap().rd(&one).unwrap();
    assert_eq!(rd34.lcm(&rd33), BigInt::from_i64(8357));

    // Clipped duration participation, pressure, overhang, and shedding audit
    let sf = SupportForm::parse("1:2:9_yro").unwrap(); // RD(1) = 9
    assert_eq!(sf.rho_window(&one, &BigInt::from_i64(4)).unwrap(), BigInt::from_i64(4));
    assert_eq!(
        sf.pressure(&BigInt::from_i64(3), &one, &BigInt::from_i64(4)).unwrap(),
        BigInt::from_i64(12)
    );
    assert_eq!(
        sf.overhang(&BigInt::from_i64(3), &one, &BigInt::from_i64(4)).unwrap(),
        BigInt::from_i64(15)
    );

    let audit = SupportForm::shedding_audit(&BigInt::from_i64(27), &BigInt::from_i64(24)).unwrap();
    assert_eq!(audit.delta_close, BigInt::from_i64(3));
    assert_eq!(audit.x_shedding, BigInt::from_i64(3));

    let deficit = SupportForm::shedding_audit(&BigInt::from_i64(20), &BigInt::from_i64(24)).unwrap();
    assert_eq!(deficit.delta_close, BigInt::from_i64(-4));
    assert_eq!(deficit.x_shedding, BigInt::zero());

    // Hostile odd/even _yro and domain controls
    assert!(matches!(
        SupportForm::parse("1:2:9"),
        Err(CoreError::OddSupportRequiresYro(_))
    ));
    assert!(matches!(
        SupportForm::parse("3:3:6_yro"),
        Err(CoreError::EvenSupportForbidsYro(_))
    ));
    assert!(SupportForm::from_u64(0, 2, 2, false).is_err());
    assert!(SupportForm::from_u64(1, 0, 2, false).is_err());
    assert!(SupportForm::from_u64(1, 2, 0, false).is_err());
    assert!(sf.rd(&BigInt::zero()).is_err());
    assert!(sf.pressure(&BigInt::from_i64(-1), &one, &one).is_err());
    assert!(SupportForm::shedding_audit(&BigInt::from_i64(-1), &one).is_err());
}

#[test]
fn manual_section4_ledger_kernel_hinge_slide_tunnel_clip_and_sadar() {
    let v0 = Q4::new(0).unwrap();
    let e1 = Q4::new(1).unwrap();
    let e2 = Q4::new(2).unwrap();
    let e3 = Q4::new(4).unwrap();
    let e4 = Q4::new(8).unwrap();

    // Hostile Hamming-2 neighbor rejected
    assert!(matches!(
        LedgerRow::new("B", v0, "e_bad", Q4::new(3).unwrap(), T1::POS, true, BigInt::one(), RouteStatus::Outgoing),
        Err(CoreError::HammingViolation(_))
    ));

    // One-node anisotropic kernel w = (1, 1, 4, 2) -> P = (1/8, 1/8, 1/2, 1/4)
    let rows = vec![
        LedgerRow::new("B", v0, "e_1", e1, T1::POS, true, BigInt::from_i64(1), RouteStatus::Outgoing).unwrap(),
        LedgerRow::new("B", v0, "e_2", e2, T1::NEG, true, BigInt::from_i64(1), RouteStatus::Returned).unwrap(),
        LedgerRow::new("B", v0, "e_3", e3, T1::ZERO, true, BigInt::from_i64(4), RouteStatus::Hinge).unwrap(),
        LedgerRow::new("B", v0, "e_4", e4, T1::POS, true, BigInt::from_i64(2), RouteStatus::Outgoing).unwrap(),
    ];
    let norm = Kernel::normalize(&rows).unwrap();
    assert_eq!(norm[0].probability, Ratio::from_i64(1, 8).unwrap());
    assert_eq!(norm[1].probability, Ratio::from_i64(1, 8).unwrap());
    assert_eq!(norm[2].probability, Ratio::from_i64(1, 2).unwrap());
    assert_eq!(norm[3].probability, Ratio::from_i64(1, 4).unwrap());

    // Blocked dwell (e_3, 0_D) -> post-block slide pool {(e_1,-1),(e_2,-1),(e_3,+1)} with weights (3,3,1)
    let slide_pool = vec![
        LedgerRow::new("B", v0, "e_1", e1, T1::NEG, true, BigInt::from_i64(3), RouteStatus::Slide).unwrap(),
        LedgerRow::new("B", v0, "e_2", e2, T1::NEG, true, BigInt::from_i64(3), RouteStatus::Slide).unwrap(),
        LedgerRow::new("B", v0, "e_3", e3, T1::POS, true, BigInt::from_i64(1), RouteStatus::Slide).unwrap(),
    ];
    let slide = HingeSlide::block_dwell_and_slide(&norm, "e_3", &slide_pool).unwrap();
    assert!(slide.structural_hinge_0h_retained);
    assert!(!slide.blocked_prior_rows[2].admissible);
    assert_eq!(slide.blocked_prior_rows[2].probability, Ratio::zero());
    assert_eq!(slide.slide_rows[0].probability, Ratio::from_i64(3, 7).unwrap());
    assert_eq!(slide.slide_rows[1].probability, Ratio::from_i64(3, 7).unwrap());
    assert_eq!(slide.slide_rows[2].probability, Ratio::from_i64(1, 7).unwrap());
    assert_eq!(slide.p_returned, Ratio::from_i64(6, 7).unwrap());
    assert_eq!(slide.p_outgoing, Ratio::from_i64(1, 7).unwrap());

    // Hostile sigma == 0 inside post-block slide pool rejected
    let bad_slide_pool = vec![
        LedgerRow::new("B", v0, "e_3", e3, T1::ZERO, true, BigInt::from_i64(1), RouteStatus::Slide).unwrap(),
    ];
    assert!(HingeSlide::block_dwell_and_slide(&norm, "e_3", &bad_slide_pool).is_err());

    // Manual §4 Field Tunnelling: single tunnel branch (e_3, +1), RD=5, |omega|=2, C_3=3 -> 6/7
    let single_tunnel = TunnelClip::evaluate(
        &slide.slide_rows,
        &[TunnelBranchInput {
            edge_slot: "e_3".to_string(),
            sigma: T1::POS,
            rd: BigInt::from_i64(5),
            window_len: BigInt::from_i64(2),
            c3: BigInt::from_i64(3),
            attention: None,
        }],
    )
    .unwrap();
    assert_eq!(single_tunnel.t_tunnel, Ratio::from_i64(6, 7).unwrap());

    // Full slide-pool sum when all 3 successors have pressure 6 -> (3/7 + 3/7 + 1/7) * 6 = 6/1
    let full_tunnel = TunnelClip::evaluate(
        &slide.slide_rows,
        &[
            TunnelBranchInput {
                edge_slot: "e_1".to_string(),
                sigma: T1::NEG,
                rd: BigInt::from_i64(5),
                window_len: BigInt::from_i64(2),
                c3: BigInt::from_i64(3),
                attention: Some(Ratio::from_i64(1, 3).unwrap()),
            },
            TunnelBranchInput {
                edge_slot: "e_2".to_string(),
                sigma: T1::NEG,
                rd: BigInt::from_i64(5),
                window_len: BigInt::from_i64(2),
                c3: BigInt::from_i64(3),
                attention: Some(Ratio::from_i64(1, 3).unwrap()),
            },
            TunnelBranchInput {
                edge_slot: "e_3".to_string(),
                sigma: T1::POS,
                rd: BigInt::from_i64(5),
                window_len: BigInt::from_i64(2),
                c3: BigInt::from_i64(3),
                attention: Some(Ratio::from_i64(1, 3).unwrap()),
            },
        ],
    )
    .unwrap();
    assert_eq!(full_tunnel.t_tunnel, Ratio::from_i64(6, 1).unwrap());
    assert_eq!(full_tunnel.phi_tunnel, Some(Ratio::from_i64(2, 1).unwrap()));

    // Manual §4 SADAR promotion: A_12 = 1/3, rho = 4, C_3 = 3 -> p^D = 12, SADAR = 4/1, reverse = -4/1
    let sadar = SadarPromotion::promote(
        &BigInt::from_i64(3),
        &BigInt::from_i64(4),
        Ratio::from_i64(1, 3).unwrap(),
    )
    .unwrap();
    assert_eq!(sadar.p_d, BigInt::from_i64(12));
    assert_eq!(sadar.sadar, Ratio::from_i64(4, 1).unwrap());
    assert_eq!(sadar.reverse_sadar, Ratio::from_i64(-4, 1).unwrap());
    assert_eq!(&sadar.sadar + &sadar.reverse_sadar, Ratio::zero());

    // v1 probability helper parity (weights.dec)
    assert_eq!(
        probability(&BigInt::from_i64(2), &BigInt::from_i64(1), &BigInt::from_i64(3), T1::NEG).unwrap(),
        Ratio::from_i64(1, 3).unwrap()
    );
    assert_eq!(
        probability(&BigInt::from_i64(2), &BigInt::from_i64(1), &BigInt::from_i64(3), T1::ZERO).unwrap(),
        Ratio::from_i64(1, 6).unwrap()
    );
    assert_eq!(
        probability(&BigInt::from_i64(2), &BigInt::from_i64(1), &BigInt::from_i64(3), T1::POS).unwrap(),
        Ratio::from_i64(1, 2).unwrap()
    );
}
