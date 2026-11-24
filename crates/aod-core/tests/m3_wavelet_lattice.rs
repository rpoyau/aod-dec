use std::collections::{BTreeMap, BTreeSet};

use aod_core::bigint::BigInt;
use aod_core::error::CoreError;
use aod_core::ratio::Ratio;
use aod_core::wavelet::{
    cancel_retain, closure_cycle_count, declare_synthetic_lineage_registry,
    deduplicated_return_count, eligible_return_ids, native_cadence_ratio,
    project_admitted_lineage_occurrences, project_admitted_wave_occurrences, register_outer_returns,
    validate_complete_windows, AdmittedClosureRecord, CoordinateRelation, M3ClosureTrace, M3Wave,
    OccurrenceRegistry, ParentDescriptor, RecipientContext, RecipientLattice,
    RecipientOutcomeStage, RecipientStage, ReturnWitness, ScopeMap, ShedRoute, ShedWitness,
    SingleContactStageAssociation, StageEventWitness, Wavelet, WaveletRole, WaveletSpec,
};
use aod_core::witness::SeatState;

#[test]
fn f01_f02_f04_f05_f07_wavelet_registry_parent_and_scope_map_parity() {
    // 1. Declared synthetic overlap (C0@0, C1@2, C2@6, C3@8)
    let (known, canon_reg) = declare_synthetic_lineage_registry(
        "W_B_1",
        "B:main",
        &[0, 2, 6, 8],
        0,
        "native_cut",
        Some(&["C0", "C1", "C2", "C3"]),
    )
    .unwrap();
    let (c0, c1, c2, c3) = (&known[0], &known[1], &known[2], &known[3]);
    assert_eq!(c2.display_label, "C2");
    assert_ne!(c2.return_id, "C2");

    // Idempotent duplicate packaging
    assert_eq!(
        OccurrenceRegistry::build(&[c1.clone(), c1.clone()]).unwrap().len(),
        1
    );

    // Out-of-window retained support -> 0 eligible
    let w_out = Wavelet::new(WaveletSpec {
        wavelet_id: "w_out",
        parent_lineage: "W_B_1",
        explicit_parent: Some(&c0.parent_descriptor),
        ancestry: vec!["lin:1".into(), "out".into()],
        window: (3, 5),
        sigma_seat: SeatState::SeatedDuad,
        sigma_role: WaveletRole::CoupledSegment,
        records: vec!["rec:C0@0".into(), "rec:C1@2".into()],
        return_witnesses: vec![c0.clone(), c1.clone()],
        signed_current: BigInt::zero(),
        is_admitted_wave: false,
        scope_id: "B:main",
        witnesses_complete: true,
        scale: None,
        cut_coordinate: "native_cut",
        scope_map: None,
        registry: Some(&canon_reg),
    })
    .unwrap();
    assert!(eligible_return_ids(&w_out, None, None, None, Some(&canon_reg))
        .unwrap()
        .is_empty());

    // Overlapping complete windows: (0,6] -> {C1, C2} (2), (4,8] -> {C2, C3} (2), union == 3
    let w1 = Wavelet::new(WaveletSpec {
        wavelet_id: "w1",
        parent_lineage: "W_B_1",
        explicit_parent: Some(&c0.parent_descriptor),
        ancestry: vec!["lin:1".into(), "w1".into()],
        window: (0, 6),
        sigma_seat: SeatState::SeatedDuad,
        sigma_role: WaveletRole::CoupledSegment,
        records: vec!["rec:C0@0".into(), "rec:C1@2".into(), "rec:C2@6".into()],
        return_witnesses: vec![c0.clone(), c1.clone(), c2.clone()],
        signed_current: BigInt::from_i64(1),
        is_admitted_wave: true,
        scope_id: "B:main",
        witnesses_complete: true,
        scale: None,
        cut_coordinate: "native_cut",
        scope_map: None,
        registry: Some(&canon_reg),
    })
    .unwrap();
    let w2 = Wavelet::new(WaveletSpec {
        wavelet_id: "w2",
        parent_lineage: "W_B_1",
        explicit_parent: Some(&c0.parent_descriptor),
        ancestry: vec!["lin:1".into(), "w2".into()],
        window: (4, 8),
        sigma_seat: SeatState::SeatedDuad,
        sigma_role: WaveletRole::CoupledSegment,
        records: vec!["rec:C1@2".into(), "rec:C2@6".into(), "rec:C3@8".into()],
        return_witnesses: vec![c1.clone(), c2.clone(), c3.clone()],
        signed_current: BigInt::from_i64(-1),
        is_admitted_wave: true,
        scope_id: "B:main",
        witnesses_complete: true,
        scale: None,
        cut_coordinate: "native_cut",
        scope_map: None,
        registry: Some(&canon_reg),
    })
    .unwrap();
    validate_complete_windows(&[w1.clone(), w2.clone()], &canon_reg, None).unwrap();
    let union = deduplicated_return_count(
        &[w1.clone(), w2.clone()],
        Some(&c0.parent_descriptor),
        None,
        "B:main",
        None,
        Some(&canon_reg),
    )
    .unwrap();
    assert_eq!(union, 3);

    // Cancellation retention (eq:wavelet-cancellation-retention)
    let cancel = cancel_retain(&[w1, w2]);
    assert_eq!(cancel.net_current, BigInt::zero());
    assert_eq!(cancel.retained_histories.len(), 2);
    assert_ne!(cancel.retained_histories[0], cancel.retained_histories[1]);

    // F04: scale-0 vs scale-1 distinct parent root keys + AdmittedOriginClosure non-promotion
    let w_s0 = M3Wave::recurrent("W", 3, 10, "B:main", 0).unwrap();
    let w_s1 = M3Wave::recurrent("W", 3, 10, "B:main", 1).unwrap();
    let occ_s0 = project_admitted_wave_occurrences(&w_s0, "native_cut", "C").unwrap();
    let occ_s1 = project_admitted_wave_occurrences(&w_s1, "native_cut", "C").unwrap();
    let mut both_vec = occ_s0.clone();
    both_vec.extend(occ_s1.clone());
    let both_canon = OccurrenceRegistry::build(&both_vec).unwrap();

    let pdesc_s0 = ParentDescriptor::describe_admitted_parent(&w_s0).unwrap();
    let pdesc_s1 = ParentDescriptor::describe_admitted_parent(&w_s1).unwrap();
    assert_ne!(pdesc_s0.root_key().unwrap(), pdesc_s1.root_key().unwrap());

    let sw0 = Wavelet::restrict_admitted(
        &w_s0,
        "sw0",
        (2, 12),
        SeatState::SeatedDuad,
        WaveletRole::LocalSegment,
        "native_cut",
        None,
        BigInt::zero(),
        false,
        Some(&both_canon),
    )
    .unwrap();
    let sw1 = Wavelet::restrict_admitted(
        &w_s1,
        "sw1",
        (2, 12),
        SeatState::SeatedDuad,
        WaveletRole::LocalSegment,
        "native_cut",
        None,
        BigInt::zero(),
        false,
        Some(&both_canon),
    )
    .unwrap();
    validate_complete_windows(&[sw0.clone(), sw1.clone()], &both_canon, None).unwrap();
    assert_eq!(
        deduplicated_return_count(&[sw0.clone()], Some(&pdesc_s0), None, "B:main", None, Some(&both_canon))
            .unwrap(),
        1
    );
    assert_eq!(
        deduplicated_return_count(&[sw1.clone()], Some(&pdesc_s1), None, "B:main", None, Some(&both_canon))
            .unwrap(),
        1
    );
    // Cannot combine scale-0 and scale-1 wavelets
    assert!(deduplicated_return_count(&[sw0.clone(), sw1.clone()], None, Some("W"), "B:main", None, Some(&both_canon)).is_err());

    // Origin-only closure cannot be promoted to is_admitted_wave = true
    let orig_only = project_admitted_lineage_occurrences(
        &[w_s0.records[0].clone()],
        "native_cut",
        "C",
        None,
    )
    .unwrap();
    let orig_pdesc = &orig_only[0].parent_descriptor;
    assert!(matches!(
        Wavelet::new(WaveletSpec {
            wavelet_id: "bad_orig_promo",
            parent_lineage: "W",
            explicit_parent: Some(orig_pdesc),
            ancestry: vec!["W:ancestor".into()],
            window: (-1, 5),
            sigma_seat: SeatState::SeatedDuad,
            sigma_role: WaveletRole::LocalSegment,
            records: vec![orig_only[0].certificate_digest.clone().unwrap()],
            return_witnesses: orig_only.clone(),
            signed_current: BigInt::zero(),
            is_admitted_wave: true,
            scope_id: "B:main",
            witnesses_complete: true,
            scale: Some(0),
            cut_coordinate: "native_cut",
            scope_map: None,
            registry: None,
        }),
        Err(CoreError::PromotionForbidden(_))
    ));

    // F05: supplied canonical registry remains authoritative & non-mutating
    let origin_only_reg = OccurrenceRegistry::build(&[occ_s0[0].clone()]).unwrap();
    let keys_before = origin_only_reg.keys();
    assert!(eligible_return_ids(&sw0, None, None, None, Some(&origin_only_reg)).is_err());
    assert!(validate_complete_windows(&[sw0.clone()], &origin_only_reg, None).is_err());
    assert_eq!(origin_only_reg.keys(), keys_before);
    let extended_reg = origin_only_reg
        .extend_occurrence_registry(&[occ_s0[1].clone()])
        .unwrap();
    assert_eq!(origin_only_reg.keys(), keys_before);
    assert_eq!(
        deduplicated_return_count(&[sw0], Some(&pdesc_s0), None, "B:main", None, Some(&extended_reg))
            .unwrap(),
        1
    );

    // F01: collision & aliasing rejections
    let col_a = ReturnWitness::synthetic(
        "collision", 1, 2, "W", "B:main", 0, "declared_synthetic", None, None, None, "native_cut", true, None,
    ).unwrap();
    let col_b = ReturnWitness::synthetic(
        "collision", 2, 6, "W", "B:main", 0, "declared_synthetic", None, None, None, "native_cut", true, None,
    ).unwrap();
    assert!(matches!(
        OccurrenceRegistry::build(&[col_a, col_b]),
        Err(CoreError::CollisionError(_))
    ));

    let al_a = ReturnWitness::synthetic(
        "C1", 1, 2, "W", "B:main", 0, "declared_synthetic", None, None, None, "native_cut", true, None,
    ).unwrap();
    let al_b = ReturnWitness::synthetic(
        "C1-repackaged", 1, 2, "W", "B:main", 0, "declared_synthetic", None, None, None, "native_cut", true, None,
    ).unwrap();
    assert!(matches!(
        OccurrenceRegistry::build(&[al_a, al_b]),
        Err(CoreError::AliasingError(_))
    ));

    // Omitted return C3@8 in complete window (0, 8] rejected
    let w_inc = Wavelet::new(WaveletSpec {
        wavelet_id: "w_inc",
        parent_lineage: "W_B_1",
        explicit_parent: Some(&c0.parent_descriptor),
        ancestry: vec!["lin:1".into()],
        window: (0, 8),
        sigma_seat: SeatState::SeatedDuad,
        sigma_role: WaveletRole::CoupledSegment,
        records: vec!["rec:C1@2".into(), "rec:C2@6".into()],
        return_witnesses: vec![c1.clone(), c2.clone()],
        signed_current: BigInt::zero(),
        is_admitted_wave: false,
        scope_id: "B:main",
        witnesses_complete: true,
        scale: None,
        cut_coordinate: "native_cut",
        scope_map: None,
        registry: Some(&canon_reg),
    })
    .unwrap();
    assert!(validate_complete_windows(&[w_inc], &canon_reg, None).is_err());

    // F02 & F07: multi-parent canonical registry sharing coarse (B:source, W, scale=0)
    let parent_a = M3Wave::recurrent("W", 3, 10, "B:source", 0).unwrap();
    let b_recs: Vec<AdmittedClosureRecord> = (0..3)
        .map(|k| {
            let t = M3ClosureTrace::minimal_closed("W", 100 + k, "B:source", 0, Some(1 + 10 * k))
                .unwrap();
            AdmittedClosureRecord::admit_monon(t).unwrap()
        })
        .collect();
    let parent_b = M3Wave::new(b_recs, None).unwrap();
    let occ_a = project_admitted_wave_occurrences(&parent_a, "native_cut", "C").unwrap();
    let occ_b = project_admitted_wave_occurrences(&parent_b, "native_cut", "C").unwrap();
    let mut ab_all = occ_a.clone();
    ab_all.extend(occ_b);
    let combined_ab = OccurrenceRegistry::build(&ab_all).unwrap();
    assert_eq!(combined_ab.len(), 6);
    let all_native_recs: Vec<AdmittedClosureRecord> = combined_ab
        .values()
        .map(|r| r.admitted_record.clone().unwrap())
        .collect();
    assert_eq!(closure_cycle_count(&all_native_recs, None).unwrap(), 6);

    let pdesc_a = ParentDescriptor::describe_admitted_parent(&parent_a).unwrap();
    // Unmapped cross-scope rejected
    assert!(matches!(
        Wavelet::new(WaveletSpec {
            wavelet_id: "unmapped",
            parent_lineage: "W",
            explicit_parent: Some(&pdesc_a),
            ancestry: vec!["W:ancestor".into()],
            window: (2, 12),
            sigma_seat: SeatState::SeatedDuad,
            sigma_role: WaveletRole::LocalSegment,
            records: vec![occ_a[0].trace_digest.clone().unwrap()],
            return_witnesses: occ_a[..2].to_vec(),
            signed_current: BigInt::zero(),
            is_admitted_wave: false,
            scope_id: "B:main",
            witnesses_complete: true,
            scale: Some(0),
            cut_coordinate: "native_cut",
            scope_map: None,
            registry: Some(&combined_ab),
        }),
        Err(CoreError::ScopeMapRequired(_))
    ));

    let map_a = ScopeMap::declare_conditional("map:W-parentA", "B:source", "B:main", "W", 0, &occ_a)
        .unwrap();

    // Attaching a ScopeMap when scopes already match is rejected
    assert!(matches!(
        Wavelet::new(WaveletSpec {
            wavelet_id: "same_scope_with_map",
            parent_lineage: "W",
            explicit_parent: Some(&pdesc_a),
            ancestry: vec!["W:ancestor".into()],
            window: (2, 12),
            sigma_seat: SeatState::SeatedDuad,
            sigma_role: WaveletRole::LocalSegment,
            records: vec![occ_a[0].trace_digest.clone().unwrap()],
            return_witnesses: occ_a[..2].to_vec(),
            signed_current: BigInt::zero(),
            is_admitted_wave: false,
            scope_id: "B:source",
            witnesses_complete: true,
            scale: Some(0),
            cut_coordinate: "native_cut",
            scope_map: Some(&map_a),
            registry: Some(&combined_ab),
        }),
        Err(CoreError::ScopeMapForbidden(_))
    ));

    // F07 positive: valid mapped window (2, 12] passes against combined_ab despite parent_b
    let w_mapped_a = Wavelet::new(WaveletSpec {
        wavelet_id: "w_mapped_a",
        parent_lineage: "W",
        explicit_parent: Some(&pdesc_a),
        ancestry: vec!["W:ancestor".into()],
        window: (2, 12),
        sigma_seat: SeatState::SeatedDuad,
        sigma_role: WaveletRole::LocalSegment,
        records: occ_a[..2].iter().map(|r| r.trace_digest.clone().unwrap()).collect(),
        return_witnesses: occ_a[..2].to_vec(),
        signed_current: BigInt::zero(),
        is_admitted_wave: false,
        scope_id: "B:main",
        witnesses_complete: true,
        scale: Some(0),
        cut_coordinate: "native_cut",
        scope_map: Some(&map_a),
        registry: Some(&combined_ab),
    })
    .unwrap();
    assert_eq!(
        validate_complete_windows(&[w_mapped_a.clone()], &combined_ab, Some(&map_a))
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        deduplicated_return_count(
            &[w_mapped_a],
            Some(&pdesc_a),
            None,
            "B:main",
            Some(&map_a),
            Some(&combined_ab)
        )
        .unwrap(),
        1
    );

    // F07 hostile: omitted own-parent return in (2, 22] rejected, and partial_map_a unbound return rejected
    let w_mapped_omit = Wavelet::new(WaveletSpec {
        wavelet_id: "w_mapped_omit",
        parent_lineage: "W",
        explicit_parent: Some(&pdesc_a),
        ancestry: vec!["W:ancestor".into()],
        window: (2, 22),
        sigma_seat: SeatState::SeatedDuad,
        sigma_role: WaveletRole::LocalSegment,
        records: occ_a[..2].iter().map(|r| r.trace_digest.clone().unwrap()).collect(),
        return_witnesses: occ_a[..2].to_vec(),
        signed_current: BigInt::zero(),
        is_admitted_wave: false,
        scope_id: "B:main",
        witnesses_complete: true,
        scale: Some(0),
        cut_coordinate: "native_cut",
        scope_map: Some(&map_a),
        registry: Some(&combined_ab),
    })
    .unwrap();
    assert!(validate_complete_windows(&[w_mapped_omit.clone()], &combined_ab, Some(&map_a)).is_err());
    let partial_map_a =
        ScopeMap::declare_conditional("map:W-parentA", "B:source", "B:main", "W", 0, &occ_a[..2])
            .unwrap();
    assert!(validate_complete_windows(&[w_mapped_omit], &combined_ab, Some(&partial_map_a)).is_err());

    // Inner/outer disjointness
    assert_eq!(register_outer_returns(&["in_01", "in_02"], &["out_01"]).unwrap(), 1);
    assert!(register_outer_returns(&["in_01", "in_02"], &["in_02", "out_01"]).is_err());
}

#[test]
fn f03_f06_recipient_lattice_chronology_coordinate_relation_and_mapped_replay() {
    let witness = ShedWitness::new(
        "W_B_1",
        "dT_R1",
        "shared_committed_k",
        12,
        ShedRoute::Exo,
        Ratio::from_i64(1, 2).unwrap(),
        "S_W_post_12",
    )
    .unwrap();
    // Hostile X_route <= 0 rejected
    assert!(ShedWitness::new(
        "W_B_1",
        "dT_R1",
        "shared_committed_k",
        12,
        ShedRoute::Exo,
        Ratio::zero(),
        "S_W_post_12",
    )
    .is_err());

    let ctx_fresh = RecipientContext {
        recipient_lineage: "R_1".into(),
        recipient_scope: "B_rec:1".into(),
        cut_coordinate: "shared_committed_k".into(),
        closure_cut_coordinate: None,
        window: (10, 25),
        coordinate_relation: None,
        prior_certified_returns: Vec::new(),
        seen_return_ids: BTreeSet::new(),
    };

    let mut ev_seat = BTreeMap::new();
    ev_seat.insert(
        RecipientStage::Delivered,
        StageEventWitness::new("ev:d2", RecipientStage::Delivered, "B_rec:1", "shared_committed_k", 12).unwrap(),
    );
    ev_seat.insert(
        RecipientStage::Accepted,
        StageEventWitness::new("ev:a2", RecipientStage::Accepted, "B_rec:1", "shared_committed_k", 13).unwrap(),
    );
    ev_seat.insert(
        RecipientStage::Seated,
        StageEventWitness::new("ev:s2", RecipientStage::Seated, "B_rec:1", "shared_committed_k", 14).unwrap(),
    );

    // Non-closing stages return (0, 0, blocks_later_seating = false)
    let r_seat = RecipientLattice::evaluate(
        &witness,
        &ctx_fresh,
        &[
            RecipientStage::Emitted,
            RecipientStage::Delivered,
            RecipientStage::Accepted,
            RecipientStage::Seated,
        ],
        None,
        Some(&ev_seat),
        None,
    )
    .unwrap();
    assert_eq!(r_seat.closure_phase_delta, 0);
    assert_eq!(r_seat.reclosure_bip_delta, 0);
    assert!(!r_seat.blocks_later_seating);

    // ClosedOrigin -> (1, 0, j = 0)
    let rec0 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R_1", 0, "B_rec:1", 0, Some(13)).unwrap(),
    )
    .unwrap();
    let r_orig = RecipientLattice::evaluate(
        &witness,
        &ctx_fresh,
        &[
            RecipientStage::Emitted,
            RecipientStage::Delivered,
            RecipientStage::Accepted,
            RecipientStage::Seated,
            RecipientStage::ClosedOrigin,
        ],
        Some(&rec0),
        Some(&ev_seat),
        None,
    )
    .unwrap();
    assert_eq!(r_orig.stage, RecipientOutcomeStage::ClosedOrigin);
    assert_eq!(r_orig.closure_phase_delta, 1);
    assert_eq!(r_orig.reclosure_bip_delta, 0);
    assert_eq!(r_orig.occurrence_j, Some(0));

    // F06: cross-scope mapped fresh j=2 + replay of j=2 and j=1
    let src_s0 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R_1", 0, "B_src:1", 0, Some(1)).unwrap(),
    )
    .unwrap();
    let src_s1 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R_1", 1, "B_src:1", 0, Some(13)).unwrap(),
    )
    .unwrap();
    let src_s2 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R_1", 2, "B_src:1", 0, Some(16)).unwrap(),
    )
    .unwrap();
    let src_proj = project_admitted_lineage_occurrences(
        &[src_s0.clone(), src_s1.clone(), src_s2.clone()],
        "shared_committed_k",
        "C",
        None,
    )
    .unwrap();
    let rec_smap =
        ScopeMap::declare_conditional("map:src_to_rec", "B_src:1", "B_rec:1", "R_1", 0, &src_proj)
            .unwrap();

    let ctx_mapped_fresh = RecipientContext {
        prior_certified_returns: vec![src_s0.clone(), src_s1.clone()],
        ..ctx_fresh.clone()
    };
    let r_mapped_fresh = RecipientLattice::evaluate(
        &witness,
        &ctx_mapped_fresh,
        &[
            RecipientStage::Emitted,
            RecipientStage::Delivered,
            RecipientStage::Accepted,
            RecipientStage::Seated,
            RecipientStage::Reclosed,
        ],
        Some(&src_s2),
        Some(&ev_seat),
        Some(&rec_smap),
    )
    .unwrap();
    assert_eq!(r_mapped_fresh.stage, RecipientOutcomeStage::Reclosed);
    assert_eq!(r_mapped_fresh.closure_phase_delta, 1);
    assert_eq!(r_mapped_fresh.reclosure_bip_delta, 1);
    assert_eq!(r_mapped_fresh.occurrence_j, Some(2));

    let ctx_mapped_replay = RecipientContext {
        prior_certified_returns: vec![src_s0, src_s1.clone(), src_s2.clone()],
        ..ctx_fresh.clone()
    };
    let r_rep2 = RecipientLattice::evaluate(
        &witness,
        &ctx_mapped_replay,
        &[
            RecipientStage::Emitted,
            RecipientStage::Delivered,
            RecipientStage::Accepted,
            RecipientStage::Seated,
            RecipientStage::Reclosed,
        ],
        Some(&src_s2),
        Some(&ev_seat),
        Some(&rec_smap),
    )
    .unwrap();
    assert_eq!(r_rep2.stage, RecipientOutcomeStage::ReclosedReplay);
    assert_eq!(r_rep2.closure_phase_delta, 0);
    assert_eq!(r_rep2.reclosure_bip_delta, 0);
    assert_eq!(r_rep2.occurrence_j, Some(2));

    let r_rep1 = RecipientLattice::evaluate(
        &witness,
        &ctx_mapped_replay,
        &[
            RecipientStage::Emitted,
            RecipientStage::Delivered,
            RecipientStage::Accepted,
            RecipientStage::Seated,
            RecipientStage::Reclosed,
        ],
        Some(&src_s1),
        Some(&ev_seat),
        Some(&rec_smap),
    )
    .unwrap();
    assert_eq!(r_rep1.stage, RecipientOutcomeStage::ReclosedReplay);
    assert_eq!(r_rep1.closure_phase_delta, 0);
    assert_eq!(r_rep1.reclosure_bip_delta, 0);
    assert_eq!(r_rep1.occurrence_j, Some(1));

    // F03 hostile: pre-emission completion (emit_cut 19 vs completion 18) rejected
    let shed_19 = ShedWitness::new(
        "P",
        "B:shared",
        "shared_committed_k",
        19,
        ShedRoute::Exo,
        Ratio::from_i64(1, 2).unwrap(),
        "Ppost19",
    )
    .unwrap();
    let sh0 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R", 0, "B:shared", 0, Some(1)).unwrap(),
    )
    .unwrap();
    let sh1 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R", 1, "B:shared", 0, Some(11)).unwrap(),
    )
    .unwrap();
    let sh2_18 = AdmittedClosureRecord::admit_monon(
        M3ClosureTrace::minimal_closed("R", 2, "B:shared", 0, Some(16)).unwrap(),
    )
    .unwrap(); // ends 18 < 19
    let ctx_shared = RecipientContext {
        recipient_lineage: "R".into(),
        recipient_scope: "B:shared".into(),
        cut_coordinate: "shared_committed_k".into(),
        closure_cut_coordinate: None,
        window: (10, 20),
        coordinate_relation: None,
        prior_certified_returns: vec![sh0, sh1],
        seen_return_ids: BTreeSet::new(),
    };
    let mut ev_19 = BTreeMap::new();
    ev_19.insert(
        RecipientStage::Delivered,
        StageEventWitness::new("e1", RecipientStage::Delivered, "B:shared", "shared_committed_k", 19).unwrap(),
    );
    ev_19.insert(
        RecipientStage::Accepted,
        StageEventWitness::new("e2", RecipientStage::Accepted, "B:shared", "shared_committed_k", 19).unwrap(),
    );
    ev_19.insert(
        RecipientStage::Seated,
        StageEventWitness::new("e3", RecipientStage::Seated, "B:shared", "shared_committed_k", 19).unwrap(),
    );
    assert!(matches!(
        RecipientLattice::evaluate(
            &shed_19,
            &ctx_shared,
            &[
                RecipientStage::Emitted,
                RecipientStage::Delivered,
                RecipientStage::Accepted,
                RecipientStage::Seated,
                RecipientStage::Reclosed,
            ],
            Some(&sh2_18),
            Some(&ev_19),
            None,
        ),
        Err(CoreError::ChronologyError(_))
    ));

    // F03 hostile: CoordinateRelation required across distinct coords & forbidden on shared coords
    let shed_other_coord = ShedWitness {
        cut_coordinate: "source_local_k".into(),
        ..witness.clone()
    };
    assert!(matches!(
        RecipientLattice::evaluate(
            &shed_other_coord,
            &ctx_fresh,
            &[RecipientStage::Emitted, RecipientStage::Delivered],
            None,
            Some(&ev_seat),
            None,
        ),
        Err(CoreError::CoordinateRelationRequired(_))
    ));
    let rel_ok = CoordinateRelation::declare(
        "rel:src_to_rec",
        "source_local_k",
        "shared_committed_k",
        "W_B_1",
        12,
        12,
    )
    .unwrap();
    let ctx_with_rel = RecipientContext {
        coordinate_relation: Some(rel_ok),
        ..ctx_fresh.clone()
    };
    // Attaching CoordinateRelation when coordinates already match -> forbidden!
    assert!(matches!(
        RecipientLattice::evaluate(
            &witness,
            &ctx_with_rel,
            &[RecipientStage::Emitted, RecipientStage::Delivered],
            None,
            Some(&ev_seat),
            None,
        ),
        Err(CoreError::CoordinateRelationForbidden(_))
    ));
    // Valid cross-coordinate relation succeeds
    let ok_rel_out = RecipientLattice::evaluate(
        &shed_other_coord,
        &ctx_with_rel,
        &[RecipientStage::Emitted, RecipientStage::Delivered],
        None,
        Some(&ev_seat),
        None,
    )
    .unwrap();
    assert_eq!(ok_rel_out.stage, RecipientOutcomeStage::Delivered);

    // Cadence ratio vs SingleContactStageAssociation
    assert_eq!(
        native_cadence_ratio("Wa", &BigInt::from_i64(3), "Wr", &BigInt::from_i64(5), true, 2)
            .unwrap(),
        Ratio::from_i64(3, 5).unwrap()
    );
    assert_eq!(
        native_cadence_ratio("Wshed", &BigInt::zero(), "Wr", &BigInt::from_i64(10), true, 2)
            .unwrap(),
        Ratio::zero()
    );
    assert!(native_cadence_ratio("Wshed", &BigInt::zero(), "Wr", &BigInt::from_i64(10), true, 1).is_err());
    assert!(native_cadence_ratio("Wa", &BigInt::from_i64(3), "Wr", &BigInt::zero(), true, 2).is_err());
    assert!(native_cadence_ratio("Wa", &BigInt::from_i64(3), "Wr", &BigInt::from_i64(5), false, 2).is_err());
    assert!(native_cadence_ratio("Wa", &BigInt::from_i64(3), "Wa", &BigInt::from_i64(5), true, 2).is_err());

    let assoc = SingleContactStageAssociation::new("Wshed", "W_B_1", "Wr", (10, 20), 1).unwrap();
    assert!(!assoc.is_cadence_ratio);
    assert!(assoc.biz_tf.is_none());
}
