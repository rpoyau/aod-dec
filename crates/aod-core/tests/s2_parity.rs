use std::collections::BTreeMap;
use std::path::PathBuf;

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::qext::{parse_decimal_exact, QExtRatio};
use aod_core::ratio::Ratio;
use aod_core::s2_kernel::{
    awareness_current_update, awareness_mindfulness_step, brownian_3d_delta3_msb_x,
    closure_wave_rd_rcd, cycle_shedding_step, cyclotomic_z12_overtone,
    dec_field_tunnelling_window_clip, dec_hinge_slide_normalize, dec_normalize_one_node,
    finite_stokes_cut_balance, format_rational_fixed, higgs_lhc_rounded_display,
    higgs_trace_score_and_freeze, lensing_swells_delta3_and_tau_discrepancy,
    map21_nonlinear_finite_law_variance, parse_canonical_rational_pair, parse_exact_int_str,
    parse_printed_rational, printed_token_half_ulp_enclosure, q4_hamming_and_neighbors,
    relational_cadence_and_lag, round_rational_half_even, run_tumble_3d_delta3_lsb_x,
    sadar_afc_flux_antisymmetry, sparc_exact_bin_eval, temporal_metrology_hellwig_mean,
    temporal_metrology_phase_si, temporal_metrology_signed_advance, z4_dft_gaussian,
    DecOneNodeRow, GaussianRatio, HiggsCandidateInput, HingeSlideBranch, Map21SupportPoint,
    OrientedChannel, PhaseInterval, MAX_ROUNDING_DECIMALS,
};
use aod_core::s2_vectors::{
    validate_vector_record, verify_reject_operation, verify_s2_vectors_dir, RATIONAL_CORE_REL,
    SYMBOLIC_DISPLAY_REL,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn r(n: i64, d: i64) -> Ratio {
    Ratio::from_i64(n, d).unwrap()
}

fn bi(n: i64) -> BigInt {
    BigInt::from_i64(n)
}

#[test]
fn ac02_30_of_30_golden_vectors_and_exact_status_rollup() {
    let root = repo_root();
    let report = verify_s2_vectors_dir(
        &root.join("bindings/aod/s2_vectors"),
        &root.join("SOURCE_BINDING.json"),
    );
    assert!(
        report.is_ok(),
        "expected 30/30 S2 golden vectors to PASS, failures: {:?}",
        report.failures
    );
    assert_eq!(report.status, "PASS");
    assert_eq!(report.binding_id, "S2@3122c94");
    assert_eq!(report.binding_status, "DRAFT_CANDIDATE_UNDER_S2_REVIEW");
    assert_eq!(
        report.upstream_commit,
        "3122c9444596a9f479e1444c9d497e510a87f825"
    );
    assert_eq!(
        report.upstream_tree,
        "d58b43ccf82b428be8f831d11203604e12d66388"
    );
    assert_eq!(
        report.base_commit_binding,
        "cea71a76f26c08b603866e746867b3982da6c88e"
    );
    assert_eq!(report.rational_core_count, 24);
    assert_eq!(report.symbolic_and_display_count, 6);
    assert_eq!(report.total_vectors, 30);
    assert_eq!(report.passed_vectors, 30);
    assert_eq!(report.failed_vectors, 0);
    assert!(report.failures.is_empty());

    assert_eq!(
        report.file_digests.get(RATIONAL_CORE_REL).map(String::as_str),
        Some("a852094458a884ec456fd712a1fbfdb8cff46f2e90095b74bafe7644ad72cc99")
    );
    assert_eq!(
        report
            .file_digests
            .get(SYMBOLIC_DISPLAY_REL)
            .map(String::as_str),
        Some("d2ea7c41ff7fb7b62258cb137576f12f667c38377406d695557fa37b3541d19f")
    );

    assert_eq!(
        report.scientific_status_summary.get("AUTHENTICATED_EXACT"),
        Some(&18)
    );
    assert_eq!(
        report
            .scientific_status_summary
            .get("CONFORMANT_DECLARED_FIXTURE"),
        Some(&5)
    );
    assert_eq!(
        report
            .scientific_status_summary
            .get("RETAINED_DATA_CONFORMANT"),
        Some(&6)
    );
    assert_eq!(
        report
            .scientific_status_summary
            .get("UNRESOLVED_PROVENANCE_QUARANTINED"),
        Some(&1)
    );
}

#[test]
fn ac03_anti_memorization_all_24_positive_operations_and_3_reject_controls() {
    // 1. dec_normalize_one_node (fresh weights [2, 3, 5, 10])
    let n1 = dec_normalize_one_node(&[
        DecOneNodeRow {
            admissible: bi(1),
            weight: bi(2),
            state: 1,
            neighbor: [1, 0, 0, 0],
        },
        DecOneNodeRow {
            admissible: bi(1),
            weight: bi(3),
            state: -1,
            neighbor: [0, 1, 0, 0],
        },
        DecOneNodeRow {
            admissible: bi(1),
            weight: bi(5),
            state: 0,
            neighbor: [0, 0, 1, 0],
        },
        DecOneNodeRow {
            admissible: bi(1),
            weight: bi(10),
            state: -1,
            neighbor: [0, 0, 0, 1],
        },
    ])
    .unwrap();
    assert_eq!(n1.z_total, bi(20));
    assert_eq!(n1.probabilities, vec![r(1, 10), r(3, 20), r(1, 4), r(1, 2)]);
    assert_eq!(n1.sum, r(1, 1));
    assert_eq!(n1.routes, vec!["outgoing", "returned", "hinge", "returned"]);

    // 2. dec_hinge_slide_normalize (fresh fractional weights)
    let hs = dec_hinge_slide_normalize(&[
        HingeSlideBranch {
            weight: r(5, 2),
            route: "returned".to_string(),
        },
        HingeSlideBranch {
            weight: r(1, 2),
            route: "returned".to_string(),
        },
        HingeSlideBranch {
            weight: r(2, 1),
            route: "outgoing".to_string(),
        },
    ])
    .unwrap();
    assert_eq!(hs.z_total, r(5, 1));
    assert_eq!(hs.sum_returned, r(3, 5));
    assert_eq!(hs.sum_outgoing, r(2, 5));

    // 3. dec_field_tunnelling_window_clip
    let tun = dec_field_tunnelling_window_clip(&bi(0), &r(3, 2), &r(9, 1), &bi(11), &bi(4), &bi(5))
        .unwrap();
    assert_eq!(tun.p_val, r(1, 6));
    assert_eq!(tun.rho_d_omega, bi(4));
    assert_eq!(tun.p_d, bi(20));
    assert_eq!(tun.tunnel_contribution, r(10, 3));

    // 4. sadar_afc_flux_antisymmetry (3x3 fresh stochastic matrix)
    let flux = sadar_afc_flux_antisymmetry(
        &[r(1, 6), r(1, 3), r(1, 2)],
        &[
            vec![r(1, 2), r(1, 4), r(1, 4)],
            vec![r(1, 3), r(1, 3), r(1, 3)],
            vec![r(1, 5), r(2, 5), r(2, 5)],
        ],
    )
    .unwrap();
    assert!(flux.antisymmetric);
    assert_eq!(flux.flux[0][1], r(1, 24) - r(1, 9));
    assert_eq!(flux.flux[1][0], -flux.flux[0][1].clone());

    // 5. q4_hamming_and_neighbors (fresh non-adjacent pair)
    let q4 = q4_hamming_and_neighbors(&bi(3), &bi(12), &bi(4)).unwrap();
    assert_eq!(q4.hamming_distance, 4);
    assert!(!q4.is_h1_edge);
    assert_eq!(q4.neighbors, vec![bi(2), bi(1), bi(7), bi(11)]);

    // 6. closure_wave_rd_rcd (p=4, q=2, hinges=2)
    let cw = closure_wave_rd_rcd(&bi(4), &bi(2), &bi(2), &[bi(3), bi(1)], &bi(35)).unwrap();
    assert_eq!(cw.pi_val, bi(18));
    assert_eq!(cw.rd_val, bi(38));
    assert_eq!(cw.rcd_duration, bi(42));
    assert_eq!(cw.participation, bi(35));

    // 7. relational_cadence_and_lag
    let rc = relational_cadence_and_lag(&bi(7), &bi(5), &r(3, 4), &r(1, 2), &r(1, 1)).unwrap();
    assert_eq!(rc.forward.ratio, r(7, 5));
    assert_eq!(rc.forward.bias_diff, r(1, 4));
    assert_eq!(rc.forward.lag, r(9, 4));
    assert_eq!(rc.forward.drift, r(5, 4));
    assert_eq!(rc.reverse.ratio, r(5, 7));
    assert_eq!(rc.reverse.lag, r(-9, 4));

    // 8. finite_stokes_cut_balance (fresh 4-node region)
    let st = finite_stokes_cut_balance(
        &[
            OrientedChannel {
                i: bi(0),
                j: bi(1),
                p: r(1, 2),
                a: r(6, 1),
                lineage: "x".to_string(),
            },
            OrientedChannel {
                i: bi(1),
                j: bi(2),
                p: r(3, 4),
                a: r(4, 1),
                lineage: "x".to_string(),
            },
            OrientedChannel {
                i: bi(2),
                j: bi(3),
                p: r(2, 5),
                a: r(10, 1),
                lineage: "y".to_string(),
            },
            OrientedChannel {
                i: bi(0),
                j: bi(3),
                p: r(1, 3),
                a: r(-9, 1),
                lineage: "z".to_string(),
            },
        ],
        &[bi(0), bi(1), bi(2)],
    )
    .unwrap();
    assert_eq!(st.internal, r(0, 1));
    assert_eq!(st.boundary, r(1, 1));
    assert_eq!(st.divergence, r(1, 1));
    assert!(st.divergence_equals_boundary);

    // 9. awareness_current_update (selecting slot 1 when next_current <= threshold)
    let aw = awareness_current_update(
        &r(1, 4),
        &r(3, 1),
        &r(3, 4),
        &r(-1, 2),
        &r(1, 2),
        &[r(2, 1), r(-1, 2)],
        &r(1, 1),
    )
    .unwrap();
    assert_eq!(aw.carried, r(3, 4));
    assert_eq!(aw.native_current, r(1, 2));
    assert_eq!(aw.next_current, r(7, 12));
    assert_eq!(aw.selected_slot, 1);
    assert!(aw.awareness_effect);

    // 10. awareness_mindfulness_step (pressure=2 halves distance to 1 each step)
    let mf = awareness_mindfulness_step(&r(2, 1), &r(5, 1), &r(1, 1), &r(1, 1), 3).unwrap();
    assert_eq!(mf.no_delay_currents, vec![r(3, 1), r(2, 1), r(3, 2)]);
    assert!(!mf.no_delay_stable_at_one);
    assert_eq!(mf.extra_delay_currents, vec![r(5, 1), r(3, 1), r(1, 1)]);

    // 11. higgs_trace_score_and_freeze (winner moved to index 1!)
    let hg = higgs_trace_score_and_freeze(&[
        HiggsCandidateInput {
            p: bi(3),
            q: bi(2),
            win_l: bi(7),
            c3: bi(3),
            ca: bi(18),
            cb: bi(24),
        },
        HiggsCandidateInput {
            p: bi(3),
            q: bi(3),
            win_l: bi(7),
            c3: bi(3),
            ca: bi(18),
            cb: bi(24),
        },
    ])
    .unwrap();
    assert_eq!(hg.winner_index, 1);
    assert_eq!(hg.scores, vec![r(79, 50), r(0, 1)]);
    assert_eq!(hg.e_h_0_mev, bi(124928));
    assert_eq!(hg.e_h_0_gev, r(15616, 125));

    // 12. brownian_3d_delta3_msb_x (fresh displacements, beta_a = 3)
    let br = brownian_3d_delta3_msb_x(
        &[
            [bi(-2), bi(0), bi(1)],
            [bi(1), bi(-1), bi(0)],
            [bi(0), bi(2), bi(-1)],
        ],
        3,
    )
    .unwrap();
    assert_eq!(br.q_vals, vec![bi(5), bi(2), bi(5)]);
    assert_eq!(br.octants_msb_x, vec![4, 2, 1]);
    assert_eq!(br.r_q, r(4, 1));
    assert_eq!(br.r_x, r(5, 12));
    assert_eq!(br.r_y, r(5, 12));
    assert_eq!(br.r_z, r(1, 6));

    // 13. run_tumble_3d_delta3_lsb_x (explicit MSB vs LSB divergence on [-1, 0, 0] and [0, 0, -1])
    let rt = run_tumble_3d_delta3_lsb_x(&[[bi(-1), bi(0), bi(0)], [bi(0), bi(0), bi(-1)]], &bi(4))
        .unwrap();
    assert_eq!(rt.octants_lsb_x, vec![1, 4]);
    assert_eq!(rt.octants_msb_x_contrast, vec![4, 1]);
    assert_eq!(rt.q3, vec![bi(1), bi(1)]);
    assert_eq!(rt.theta, vec![bi(1), bi(1)]);

    // 14. cycle_shedding_step (non-clipping zero excess + clipping positive excess)
    let cs_zero = cycle_shedding_step(
        &r(1, 4),
        &r(1, 2),
        &r(1, 4),
        &r(1, 1),
        &r(1, 2),
        &r(1, 2),
        &r(1, 3),
    );
    assert_eq!(cs_zero.shed_excess, r(0, 1));
    assert_eq!(cs_zero.padar_burden, r(1, 4));
    assert_eq!(cs_zero.remainder_sum, r(0, 1));

    // 15. sparc_exact_bin_eval (fresh inputs with negative Vgas AND nonzero Vbul)
    let sp = sparc_exact_bin_eval("2.0", "50.0", "5.0", "-1.5", "40.0", "10.0").unwrap();
    assert_eq!(sp.u_obs, r(2500, 1));
    assert_eq!(sp.sigma_u, r(500, 1));
    assert_eq!(sp.u_gas, r(9, 4));
    assert_eq!(sp.u_disk, r(1600, 1));
    assert_eq!(sp.u_bulge, r(100, 1));
    assert_eq!(sp.u_bar, r(3489, 4)); // 9/4 + 800 + 70
    assert_eq!(sp.c_cap, r(103, 1)); // 2 * (1.5 + 40 + 10)
    assert_eq!(sp.u_ao, r(6785, 4)); // 3489/4 + 824

    // 16. temporal_metrology_phase_si (fresh intervals + non-additive split detection)
    let tm = temporal_metrology_phase_si(
        &PhaseInterval {
            winding: bi(3),
            phase_start: r(1, 5),
            phase_end: r(4, 5),
        },
        &PhaseInterval {
            winding: bi(2),
            phase_start: r(0, 1),
            phase_end: r(1, 2),
        },
        &PhaseInterval {
            winding: bi(1),
            phase_start: r(1, 5),
            phase_end: r(2, 5),
        },
        &PhaseInterval {
            winding: bi(1),
            phase_start: r(2, 5),
            phase_end: r(4, 5),
        },
        &bi(1000),
    )
    .unwrap();
    assert_eq!(tm.compared_advance, r(18, 5));
    assert_eq!(tm.reference_advance, r(5, 2));
    assert_eq!(tm.ratio, r(36, 25));
    assert!(!tm.split_additivity_exact); // 6/5 + 7/5 = 13/5 != 18/5

    // 17. temporal_metrology_signed_advance (negative winding)
    let tms = temporal_metrology_signed_advance(
        &PhaseInterval {
            winding: bi(-2),
            phase_start: r(1, 2),
            phase_end: r(1, 4),
        },
        &PhaseInterval {
            winding: bi(3),
            phase_start: r(0, 1),
            phase_end: r(0, 1),
        },
        &bi(1200),
    )
    .unwrap();
    assert_eq!(tms.compared_advance, r(-9, 4));
    assert_eq!(tms.signed_mean_frequency_hz, r(-900, 1));

    // 18. temporal_metrology_hellwig_mean
    let hm = temporal_metrology_hellwig_mean(&r(2001, 10), &r(1999, 10)).unwrap();
    assert_eq!(hm.published_mean_hz, r(200, 1));
    assert_eq!(hm.difference_hz, r(1, 5));

    // 19. lensing_swells_delta3_and_tau_discrepancy
    let ln = lensing_swells_delta3_and_tau_discrepancy(
        &[bi(5), bi(5), bi(10)],
        &[bi(7), bi(5), bi(6)],
        &r(3, 10),
        &r(7, 20),
    )
    .unwrap();
    assert_eq!(ln.delta_z, vec![bi(-2), bi(0), bi(4)]);
    assert_eq!(ln.s3, vec![-1, 0, 1]);
    assert_eq!(ln.sum_delta_z, bi(2));
    assert_eq!(ln.sum_m, bi(6));
    assert_eq!(ln.tau_difference, r(1, 20));
    assert!(ln.tau_distinct);

    // 20. map21_nonlinear_finite_law_variance (AC-05: > i128::MAX exact BigInt denominator!)
    // Use y = +-1/10^10 so output_variance denominator is (10^20 - 1)^2 * 10^20 > 10^59 >> i128::MAX!
    let ten10 = BigInt::from_u64(10).pow(10);
    let y_neg = Ratio::new(-BigInt::one(), ten10.clone()).unwrap();
    let y_pos = Ratio::new(BigInt::one(), ten10.clone()).unwrap();
    let m21_big = map21_nonlinear_finite_law_variance(&[
        Map21SupportPoint {
            p: r(1, 2),
            y: y_neg.clone(),
            d: y_neg,
        },
        Map21SupportPoint {
            p: r(1, 2),
            y: y_pos.clone(),
            d: y_pos,
        },
    ])
    .unwrap();
    // Exact closed form: mean_shift = 1/(10^20 - 1), variance = 1/(10^20 * (10^20 - 1)^2)
    let ten20 = BigInt::from_u64(10).pow(20);
    let ten20_m1 = &ten20 - &BigInt::one();
    assert_eq!(
        m21_big.mean_shift,
        Ratio::new(BigInt::one(), ten20_m1.clone()).unwrap()
    );
    let expected_var_den = &(&ten20_m1 * &ten20_m1) * &ten20;
    assert!(expected_var_den.to_i128().is_none(), "denominator must exceed i128::MAX");
    assert_eq!(
        m21_big.output_variance,
        Ratio::new(BigInt::one(), expected_var_den).unwrap()
    );
    assert_eq!(m21_big.linearized_variance_diagnostic, r(0, 1));

    // 21. sympy_z4_dft (fresh input with mixed Re + Im*I Gaussian coefficients)
    let z4 = z4_dft_gaussian(&[
        "1".to_string(),
        "2".to_string(),
        "0".to_string(),
        "0".to_string(),
    ])
    .unwrap();
    assert_eq!(z4.coefficients, vec!["3/4", "1/4 - I/2", "-1/4", "1/4 + I/2"]);
    assert_eq!(z4.reconstruction, vec!["1", "2", "0", "0"]);
    assert_eq!(z4.parseval_time, r(5, 4));
    assert_eq!(z4.parseval_freq, r(5, 4));

    // Exercise all GaussianRatio::format_sympy formatting branches (P1-3)
    assert_eq!(GaussianRatio::new(r(0, 1), r(0, 1)).format_sympy(), "0");
    assert_eq!(GaussianRatio::new(r(0, 1), r(1, 1)).format_sympy(), "I");
    assert_eq!(GaussianRatio::new(r(0, 1), r(-1, 1)).format_sympy(), "-I");
    assert_eq!(GaussianRatio::new(r(0, 1), r(3, 1)).format_sympy(), "3*I");
    assert_eq!(GaussianRatio::new(r(0, 1), r(-3, 2)).format_sympy(), "-3*I/2");
    assert_eq!(GaussianRatio::new(r(1, 2), r(1, 1)).format_sympy(), "1/2 + I");
    assert_eq!(GaussianRatio::new(r(-1, 2), r(-1, 3)).format_sympy(), "-1/2 - I/3");

    // 22. cyclotomic_z12_overtone (fresh pulse {0, 6} on Z_12)
    let cyc = cyclotomic_z12_overtone(&bi(12), &[bi(0), bi(6)], &bi(3), &bi(2)).unwrap();
    assert_eq!(cyc.v_hat_3, r(1, 6));
    assert_eq!(cyc.nonzero_modes, vec![0, 2, 4, 6, 8, 10]);
    assert_eq!(cyc.parseval_exact, r(1, 6));
    assert!(!cyc.overtone_exceeds_nyquist_bound); // 2 <= 12/(2*3) = 2

    // 23. higgs_lhc_rounded_display (fresh inputs)
    let disp = higgs_lhc_rounded_display("100.00", "0.25", "99.75", 4).unwrap();
    assert_eq!(disp.delta_gev, r(-1, 4));
    assert_eq!(disp.z_score_exact, r(-1, 1));
    assert_eq!(disp.z_score_rounded_6dp, "-1.0000");
    assert_eq!(disp.z_score_rounding_residual, r(0, 1));
    assert_eq!(disp.abs_percent_error_rounded_6dp, "0.2500");
    assert_eq!(disp.abs_percent_error_rounding_residual, r(0, 1));

    // 24. printed_token_half_ulp_enclosure (fresh negative decimal)
    let enc = printed_token_half_ulp_enclosure("-12.345").unwrap();
    assert_eq!(enc.decimal_places, 3);
    assert_eq!(enc.value, r(-2469, 200));
    assert_eq!(enc.half_ulp_bound, r(1, 2000));
    assert_eq!(enc.enclosure_lower, r(-24691, 2000));
    assert_eq!(enc.enclosure_upper, r(-24689, 2000));

    // 25..27: Fresh invalid inputs for all 3 typed-reject operations (RC-021..RC-023)
    let mut r21 = BTreeMap::new();
    r21.insert("json_float_text".to_string(), JsonValue::from_str_val("{\"v\": 1e-5}"));
    r21.insert("decimal_token_for_runtime_float".to_string(), JsonValue::from_str_val("2.5"));
    verify_reject_operation("typed_reject_float_operand", &r21, "TypeError", "REJECTED_FLOAT_OPERAND").unwrap();

    let mut r22 = BTreeMap::new();
    r22.insert("bool_operand".to_string(), JsonValue::Bool(false));
    verify_reject_operation("typed_reject_bool_as_int", &r22, "TypeError", "REJECTED_BOOL_AS_INT").unwrap();

    let mut r23 = BTreeMap::new();
    r23.insert(
        "bad_tokens".to_string(),
        JsonValue::Array(
            ["+NaN", "-Infinity", "3/0", "1/2 ", "abc", ".5", "5.", "1e9999"]
                .into_iter()
                .map(JsonValue::from_str_val)
                .collect(),
        ),
    );
    verify_reject_operation(
        "typed_reject_nonfinite_and_malformed_tokens",
        &r23,
        "ValueError",
        "REJECTED_MALFORMED_TOKEN",
    )
    .unwrap();
}

#[test]
fn ac03_ac04_rounding_tie_to_even_negative_zero_and_strict_parsers() {
    // Banker's rounding (half-to-even) exact ties
    assert_eq!(round_rational_half_even(&r(1, 2), 0).unwrap(), r(0, 1));
    assert_eq!(round_rational_half_even(&r(3, 2), 0).unwrap(), r(2, 1));
    assert_eq!(round_rational_half_even(&r(5, 2), 0).unwrap(), r(2, 1));
    assert_eq!(round_rational_half_even(&r(-1, 2), 0).unwrap(), r(0, 1));
    assert_eq!(round_rational_half_even(&r(-3, 2), 0).unwrap(), r(-2, 1));
    assert_eq!(round_rational_half_even(&r(-5, 2), 0).unwrap(), r(-2, 1));
    assert_eq!(format_rational_fixed(&r(201, 200), 2).unwrap(), "1.00"); // 1.005 -> even 1.00
    assert_eq!(format_rational_fixed(&r(203, 200), 2).unwrap(), "1.02"); // 1.015 -> even 1.02

    // Negative-zero guard (P1-2): tiny negative rational rounding to 0 NEVER prints "-0.000000"
    let tiny_neg = r(-1, 10_000_000);
    assert_eq!(format_rational_fixed(&tiny_neg, 6).unwrap(), "0.000000");
    assert_eq!(format_rational_fixed(&tiny_neg, 0).unwrap(), "0");
    let q_tiny = QExtRatio::from_native_frozen(tiny_neg);
    assert_eq!(q_tiny.format_fixed(6).unwrap(), "0.000000");
    assert_eq!(q_tiny.round_half_even(6).unwrap(), parse_decimal_exact("0").unwrap());

    // Bounds guards on decimals & scientific exponent (P1-2)
    assert!(round_rational_half_even(&r(1, 3), MAX_ROUNDING_DECIMALS).is_ok());
    assert!(round_rational_half_even(&r(1, 3), MAX_ROUNDING_DECIMALS + 1).is_err());
    assert!(format_rational_fixed(&r(1, 3), MAX_ROUNDING_DECIMALS + 1).is_err());
    assert!(q_tiny.round_half_even(MAX_ROUNDING_DECIMALS + 1).is_err());
    assert!(q_tiny.format_fixed(MAX_ROUNDING_DECIMALS + 1).is_err());
    assert!(parse_printed_rational("1e1024", false, true).is_ok());
    assert!(parse_printed_rational("1e1025", false, true).is_err());
    assert!(parse_printed_rational("1e-1025", false, true).is_err());

    // Strict canonical integer parser (^(?:0|-?[1-9][0-9]*)$)
    assert_eq!(parse_exact_int_str("0").unwrap(), bi(0));
    assert_eq!(parse_exact_int_str("-42").unwrap(), bi(-42));
    for bad in ["", "+5", "-0", "01", "-01", "00", " 1", "1 ", "1.0"] {
        assert!(parse_exact_int_str(bad).is_err(), "expected rejection for {bad:?}");
    }

    // Strict canonical rational pair parser (enforces den > 0 and reducedness)
    assert_eq!(parse_canonical_rational_pair("0", "1").unwrap(), r(0, 1));
    assert_eq!(parse_canonical_rational_pair("-3", "7").unwrap(), r(-3, 7));
    for (n, d) in [
        ("2", "4"),
        ("0", "2"),
        ("-0", "1"),
        ("+1", "2"),
        ("01", "2"),
        ("1", "-2"),
        ("1", "0"),
    ] {
        assert!(
            parse_canonical_rational_pair(n, d).is_err(),
            "expected canonical pair rejection for ({n:?}, {d:?})"
        );
    }
}

#[test]
fn ac04_ac05_schema_11_keys_domain_guards_and_rc020_lock() {
    let valid_line = r#"{"schema_version":"aod.s4.vector.v1","case_id":"RC-020","source_binding":"s","operation":"lensing_swells_delta3_and_tau_discrepancy","domain":"d","inputs":{},"expected_outcome":{"status":"OK","exact_value":{}},"evidence_role":"r","scientific_status":"UNRESOLVED_PROVENANCE_QUARANTINED","oracle_basis":"closed_form_algebra","integer_encoding":{"format":"canonical_decimal_string_pair","rust_min_int_type":"i64","max_abs_integer_digits":3,"overflow_note":"ok"}}"#;
    let base = parse_strict_json(valid_line).unwrap();
    validate_vector_record(&base).unwrap();

    // 1. Tampered RC-020 scientific_status upgrade MUST be rejected (AC-05)
    let mut bad_rc20 = base.as_object().unwrap().clone();
    bad_rc20.insert(
        "scientific_status".to_string(),
        JsonValue::from_str_val("AUTHENTICATED_EXACT"),
    );
    assert!(validate_vector_record(&JsonValue::Object(bad_rc20)).is_err());

    // 2. Circular oracle_basis == candidate_snapshot MUST be rejected
    let mut bad_oracle = base.as_object().unwrap().clone();
    bad_oracle.insert(
        "oracle_basis".to_string(),
        JsonValue::from_str_val("candidate_snapshot"),
    );
    assert!(validate_vector_record(&JsonValue::Object(bad_oracle)).is_err());

    // 3. Missing key & extra key MUST be rejected
    let mut missing_k = base.as_object().unwrap().clone();
    missing_k.remove("domain");
    assert!(validate_vector_record(&JsonValue::Object(missing_k)).is_err());

    let mut extra_k = base.as_object().unwrap().clone();
    extra_k.insert("unexpected_key".to_string(), JsonValue::from_i64(1));
    assert!(validate_vector_record(&JsonValue::Object(extra_k)).is_err());

    // 4. max_abs_integer_digits: true (bool) and 0 MUST be rejected (P1-3)
    for bad_val in [JsonValue::Bool(true), JsonValue::from_i64(0)] {
        let mut obj = base.as_object().unwrap().clone();
        let mut enc = obj.get("integer_encoding").unwrap().as_object().unwrap().clone();
        enc.insert("max_abs_integer_digits".to_string(), bad_val);
        obj.insert("integer_encoding".to_string(), JsonValue::Object(enc));
        assert!(validate_vector_record(&JsonValue::Object(obj)).is_err());
    }

    // 5. Per-operation domain guards (P1-3)
    // Non-square matrix & non-unit-sum memory in sadar_afc_flux_antisymmetry
    assert!(sadar_afc_flux_antisymmetry(&[], &[]).is_err());
    assert!(sadar_afc_flux_antisymmetry(&[r(1, 2), r(1, 2)], &[vec![r(1, 1)]]).is_err());
    assert!(sadar_afc_flux_antisymmetry(
        &[r(1, 3), r(1, 3)],
        &[vec![r(1, 2), r(1, 2)], vec![r(1, 2), r(1, 2)]],
    )
    .is_err());

    // Phase out of [0, 1) in temporal_metrology_*
    let bad_phase = PhaseInterval {
        winding: bi(1),
        phase_start: r(0, 1),
        phase_end: r(1, 1), // == 1 forbidden!
    };
    let good_phase = PhaseInterval {
        winding: bi(1),
        phase_start: r(0, 1),
        phase_end: r(0, 1),
    };
    assert!(temporal_metrology_signed_advance(&bad_phase, &good_phase, &bi(100)).is_err());
    assert!(temporal_metrology_phase_si(&bad_phase, &good_phase, &good_phase, &good_phase, &bi(100)).is_err());

    // group_order != 12 in cyclotomic_z12_overtone
    assert!(cyclotomic_z12_overtone(&bi(8), &[bi(0), bi(4)], &bi(2), &bi(1)).is_err());
}
