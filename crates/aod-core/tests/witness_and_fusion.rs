use std::collections::BTreeSet;

use aod_core::bigint::BigInt;
use aod_core::ratio::Ratio;
use aod_core::ternary::{Q4, T1};
use aod_core::witness::{Coupling, DionPolarity, DuonCurrent, DuonVariant, Fusion, SeatState, Trace};

fn recur(t: &Trace) -> Trace {
    t.step(Q4::new(1).unwrap(), T1::POS)
        .unwrap()
        .hinge()
        .unwrap()
        .step(Q4::new(0).unwrap(), T1::NEG)
        .unwrap()
        .close()
        .unwrap()
}

#[test]
fn trace_dual_json_and_v1_closure_stdout_parity() {
    let a = Trace::new("B", "a", Q4::new(0).unwrap()).unwrap();
    let a = recur(&a);
    assert_eq!(a.phases().unwrap(), 1);
    assert_eq!(a.beats().unwrap(), 0);
    // Persistent replay close does not create a new return event
    let a = a.close().unwrap();
    assert_eq!(a.phases().unwrap(), 1);
    let a = recur(&a);
    assert_eq!(a.phases().unwrap(), 2);
    assert_eq!(a.beats().unwrap(), 1);
    assert_eq!(a.rate().unwrap(), Ratio::from_i64(1, 3).unwrap());

    assert_eq!(
        a.closures[0].digest,
        "9f954a87c82157ad4a2844d9dd242df35f3a50867fd6e391658c34068a461d7a"
    );
    assert_eq!(
        a.closures[1].digest,
        "5608a3867bae5f3705175ac68e8f0032ed593539e79a1dec07cab487e5d4f05f"
    );

    let expected_line = r#"{"closures":[{"begin":0,"digest":"9f954a87c82157ad4a2844d9dd242df35f3a50867fd6e391658c34068a461d7a","end":3},{"begin":3,"digest":"5608a3867bae5f3705175ac68e8f0032ed593539e79a1dec07cab487e5d4f05f","end":6}],"events":[{"kind":"step","ordinal":1,"orientation":1,"source":0,"target":1},{"kind":"hinge","ordinal":2,"orientation":0,"source":1,"target":1},{"kind":"step","ordinal":3,"orientation":-1,"source":1,"target":0},{"kind":"step","ordinal":4,"orientation":1,"source":0,"target":1},{"kind":"hinge","ordinal":5,"orientation":0,"source":1,"target":1},{"kind":"step","ordinal":6,"orientation":-1,"source":1,"target":0}],"lineage":"a","parents":[],"proposal":"","scope":"B","start":{"value":0}}"#;
    assert_eq!(a.render_v1(), expected_line);
}

#[test]
fn fusion_v1_stdout_parity_and_hostile_controls() {
    let a = recur(&Trace::new("inner", "a", Q4::new(0).unwrap()).unwrap());
    let b = recur(&Trace::new("inner", "b", Q4::new(0).unwrap()).unwrap());
    let proposal = Fusion::fold(&a, &b, "outer", "ab").unwrap();
    assert_eq!(
        proposal.digest,
        "eb23daa8f032eb72c1f073ef41dffdc28568126806617d09d8f58d3242db2c62"
    );

    let c_open = proposal.outer(Q4::new(0).unwrap()).unwrap();
    // Unclosed outer trace cannot fuse
    assert!(proposal.fuse(&c_open).is_err());

    let c_closed = recur(&c_open);
    let fused = proposal.fuse(&c_closed).unwrap();
    assert_eq!(fused.phases().unwrap(), 1);
    assert_eq!(fused.expose_inner().unwrap().len(), 2);
    assert_eq!(
        fused.closures[0].digest,
        "5a09578bd64dff8777973961dfa6f0b02b30fd6b24e2cbcfae1f2fa7bb830377"
    );

    let expected_line = r#"{"closures":[{"begin":0,"digest":"5a09578bd64dff8777973961dfa6f0b02b30fd6b24e2cbcfae1f2fa7bb830377","end":3}],"events":[{"kind":"step","ordinal":1,"orientation":1,"source":0,"target":1},{"kind":"hinge","ordinal":2,"orientation":0,"source":1,"target":1},{"kind":"step","ordinal":3,"orientation":-1,"source":1,"target":0}],"lineage":"ab","parents":[{"closures":[{"begin":0,"digest":"62495d0527f948d27d6cfa06d6da752f6593f71d48ee3366b39d837fca314fe0","end":3}],"events":[{"kind":"step","ordinal":1,"orientation":1,"source":0,"target":1},{"kind":"hinge","ordinal":2,"orientation":0,"source":1,"target":1},{"kind":"step","ordinal":3,"orientation":-1,"source":1,"target":0}],"lineage":"a","parents":[],"proposal":"","scope":"inner","start":{"value":0}},{"closures":[{"begin":0,"digest":"672971e85ef1d8c1ea5578ca41e2354cf8bc5c90b23096aea4a8a9ac2ea3a3fe","end":3}],"events":[{"kind":"step","ordinal":1,"orientation":1,"source":0,"target":1},{"kind":"hinge","ordinal":2,"orientation":0,"source":1,"target":1},{"kind":"step","ordinal":3,"orientation":-1,"source":1,"target":0}],"lineage":"b","parents":[],"proposal":"","scope":"inner","start":{"value":0}}],"proposal":"eb23daa8f032eb72c1f073ef41dffdc28568126806617d09d8f58d3242db2c62","scope":"outer","start":{"value":0}}"#;
    assert_eq!(fused.render_v1(), expected_line);

    // Hostile controls
    assert!(Fusion::fold(&a, &a, "outer", "aa").is_err());
    assert!(Fusion::fold(&a, &b, "inner", "ab").is_err());
    let base = Trace::new("B", "x", Q4::new(0).unwrap()).unwrap();
    assert!(base.step(Q4::new(3).unwrap(), T1::POS).is_err()); // Hamming-2
    assert!(base.hinge().is_err()); // Hinge before outgoing branch
    let stepped = base.step(Q4::new(1).unwrap(), T1::POS).unwrap();
    assert!(stepped.step(Q4::new(0).unwrap(), T1::NEG).is_err()); // Counterbranch without hinge
    let hinged = stepped.hinge().unwrap();
    assert!(hinged.hinge().is_err()); // Double hinge
}

#[test]
fn coupling_and_duon_same_current_seating() {
    let a = recur(&recur(&Trace::new("B", "a", Q4::new(0).unwrap()).unwrap()))
        .dwell()
        .unwrap();
    let b = recur(&recur(
        &Trace::new("B", "b", Q4::new(0).unwrap()).unwrap().dwell().unwrap(),
    ));
    let pair = Coupling::new(&a, &b, "seven-events").unwrap();
    let pair = pair.contact(3, 4).unwrap();
    // Single contact cannot compute cadence/lag/drift
    assert!(pair.cadence().is_err());
    let pair = pair.contact(6, 7).unwrap();
    assert_eq!(pair.cadence().unwrap(), Ratio::from_i64(1, 1).unwrap());
    assert_eq!(pair.lag().unwrap(), BigInt::from_i64(-1));
    assert_eq!(pair.drift().unwrap(), BigInt::zero());
    // Replayed contact rejected
    assert!(pair.contact(6, 7).is_err());

    // All 12 Duon variants distinct
    let variants = DuonVariant::all_variants();
    let uniq: BTreeSet<DuonVariant> = variants.into_iter().collect();
    assert_eq!(uniq.len(), 12);

    let duon = DuonCurrent::open(
        "duon:001",
        "lin:A",
        "sha256:prefix-001",
        DuonVariant::new(DionPolarity::Alpha, T1::ZERO, DionPolarity::Omega),
        true,
    )
    .unwrap();
    let duad = duon.seat_duad().unwrap();
    assert_eq!(duad.duon_id, "duon:001");
    assert_eq!(duad.seat_state, SeatState::SeatedDuad);
    assert!(duad.hinge_0h);

    // Hostile seating controls (eq:duad-same-current)
    assert!(duon.seat_duad_checked(Some("duon:other"), true).is_err());
    assert!(duon.seat_duad_checked(None, false).is_err());
    assert!(duad.seat_duad().is_err()); // already seated
}
