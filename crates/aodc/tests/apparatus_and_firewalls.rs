use std::path::PathBuf;

use aodc::diagnostics::{CompatMode, DiagnosticCode};
use aodc::sim::MODEL_STATUS;
use aodc::{check_source, run_file, run_source};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn ac16_sandbox_chamber_model_status_and_zero_observer_back_reaction() {
    let fixture = repo_root().join("tests/fixtures/v2_smoke/opposing_336_chamber.dec");
    let run1 = run_file(&fixture, CompatMode::V2).unwrap();
    let run2 = run_file(&fixture, CompatMode::V2).unwrap();
    assert_eq!(run1.emits, run2.emits, "sandbox must be 100% deterministic");
    assert_eq!(run1.emits[0], MODEL_STATUS);

    // Explicitly verify bit-for-bit identical final_state_hash across 0, 1, and 4 observers!
    let multi_obs = r#"
chamber BoxCh:
    space = box([0..64, 0..64, 0..64])
    walls = closed

prepare Opposing in BoxCh:
    clock ref_clk = 2
    inject 3:3:6 at (26, 32, 32) dir +X orient 1 shells 2
    inject 3:3:6 at (38, 32, 32) dir -X orient -1 shells 2

observe Obs1:
    tracer t1:
        region = box([20..44, 24..40, 24..40])
        capture = [support, returns, hinges, shedding]

observe Obs2:
    sensor s2:
        region = box([0..64, 0..64, 0..64])
        capture = [crossings, cadence]

observe Obs3:
    tracer p3(view):
        mut acc = 0
        for cid in view.carriers:
            acc = acc + cid

observe Obs4:
    sensor p4(view) -> int:
        return view.tick + view.carrier_count

Dec main() -> int:
    let r0 = simulate Opposing for 20 ticks
    let r1 = simulate Opposing for 20 ticks with Obs1
    let r4 = simulate Opposing for 20 ticks with Obs1, Obs2, Obs3, Obs4
    assert r0.model_status == "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"
    assert r0.final_state_hash == r1.final_state_hash
    assert r0.final_state_hash == r4.final_state_hash
    return 0
"#;
    let out = run_source(multi_obs, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "0\n");
}

#[test]
fn ac17_readonly_observer_firewall_direct_and_transitive_mutation_rejected() {
    // 1. Direct field mutation on `view` inside observe
    let direct_field = r#"
observe BadObs:
    tracer t(view):
        view.tick = 99

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(direct_field, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 2. Direct index mutation on `view.carriers`
    let direct_index = r#"
observe BadObs:
    tracer t(view):
        view.carriers[0] = 42

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(direct_index, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 3. Calling `simulate` inside `observe`
    let obs_sim = r#"
chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare P in C:
    inject 3:3:6 at (2, 2, 2) dir +X orient 1 shells 1

observe BadObs:
    tracer t(view):
        let r = simulate P for 1 ticks

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(obs_sim, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 4. P1-3 TRANSITIVE helper mutation: observe -> helper_a(view) -> helper_b(v) mutating v.tick!
    let transitive_field_mut = r#"
Dec helper_b(v: ChamberView) -> int:
    v.tick = 123
    return 0

Dec helper_a(v: ChamberView) -> int:
    return helper_b(v)

observe BadTransitive:
    sensor s(view) -> int:
        return helper_a(view)

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(transitive_field_mut, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 5. P1-3 TRANSITIVE helper calling `simulate` inside `observe`
    let transitive_sim = r#"
chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare P in C:
    inject 3:3:6 at (2, 2, 2) dir +X orient 1 shells 1

Dec run_hidden() -> int:
    let r = simulate P for 2 ticks
    return r.ticks

Dec wrapper() -> int:
    return run_hidden()

observe BadTransitiveSim:
    sensor s(view) -> int:
        return wrapper()

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(transitive_sim, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 6. Positive control: pure read-only helper + local scratch `mut` accumulator allowed!
    let pure_helper_ok = r#"
Dec count_plus(v: ChamberView, offset: int) -> int:
    mut acc = offset
    for cid in v.carriers:
        acc = acc + cid
    return acc

observe GoodObs:
    sensor s(view) -> int:
        return count_plus(view, 1)

Dec main() -> int:
    return 0
"#;
    assert!(check_source(pure_helper_ok, CompatMode::V2).is_ok());

    // 7. Validator R03 Probe 1: `main() -> mut_list(run.final_snapshot.carriers)` (`ReadOnly<[int]>`)
    let ro_carriers_to_multiarm = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    inject 3:3:6 at (4, 4, 4) dir +X orient +1t shells 1

Dec mut_list(xs) -> out:
    ([int] xs) -> int out:
        xs[0] = 999
        out = 0

Dec main() -> int:
    let run = simulate P for 2 ticks
    return mut_list(run.final_snapshot.carriers)
"#;
    assert_eq!(
        check_source(ro_carriers_to_multiarm, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 8. Validator R03 Probe 2: `Dec take_ro(ro: ReadOnly<S>) -> int: return mutator(ro)`
    let ro_struct_to_multiarm = r#"
struct S { x: int }

Dec mutator(s) -> out:
    (S s) -> int out:
        s.x = 42
        out = s.x

Dec take_ro(ro: ReadOnly<S>) -> int:
    return mutator(ro)

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ro_struct_to_multiarm, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 9. Validator R03 Probe 3: `ReadOnly<map[str, int]>` passed to multi-arm map mutator
    let ro_map_to_multiarm = r#"
Dec mut_map(m) -> out:
    (map[str, int] m) -> int out:
        m["k"] = 7
        out = 0

Dec take_ro_map(ro: ReadOnly<map[str, int]>) -> int:
    return mut_map(ro)

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ro_map_to_multiarm, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 10. Validator R03 Probe 4: `observe -> take_ro(s)` where `take_ro(ro: ReadOnly<S>)` calls multi-arm `mutator(ro)`
    let obs_take_ro_to_mutator = r#"
struct S { x: int }

Dec mutator(s) -> out:
    (S s) -> int out:
        s.x = 9
        out = s.x

Dec take_ro(ro: ReadOnly<S>) -> int:
    return mutator(ro)

observe SneakyReadOnly:
    sensor s(view) -> int:
        let local_s = S { x: 1 }
        return take_ro(local_s)

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(obs_take_ro_to_mutator, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 11. AC-03 PR 1 Polish: positional `Struct(ro)` cannot strip `ReadOnly<T>`
    let pos_struct_strip_ro = r#"
struct Inner { x: int }
struct Wrap { inner: Inner }

Dec take_ro(ro: ReadOnly<Inner>) -> int:
    let w = Wrap(ro)
    return w.inner.x

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(pos_struct_strip_ro, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 12. AC-03 PR 1 Polish: positional `Enum::Var(ro)` cannot strip `ReadOnly<T>`
    let pos_enum_strip_ro = r#"
struct Inner { x: int }
enum Boxed { Hold(Inner) }

Dec take_ro(ro: ReadOnly<Inner>) -> int:
    let b = Boxed::Hold(ro)
    match b:
        Boxed::Hold(v):
            return v.x
    return 0

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(pos_enum_strip_ro, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );
}

#[test]
fn ac18_one_way_quarantine_qext_firewall_positive_and_all_hostile_cases() {
    // Positive control: tests/fixtures/v2_smoke/qext_post_freeze.dec
    let fixture = repo_root().join("tests/fixtures/v2_smoke/qext_post_freeze.dec");
    let out = run_file(&fixture, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "Q_ext(1/1)\n4/5\n0\n");

    // (i) Hostile: outer native `mut` write inside `quarantine Q_ext:`
    let outer_mut_write = r#"
use aod::qext::ext_decimal

Dec main() -> int:
    mut native_flag = 0
    quarantine Q_ext:
        let q = ext_decimal("1.25")
        native_flag = 1
    return native_flag
"#;
    assert_eq!(
        check_source(outer_mut_write, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (i-b) Hostile: outer native `mut` read inside `quarantine Q_ext:` (only immutable let/frozen allowed)
    let outer_mut_read = r#"
use aod::qext::{ext_decimal, qext_from_ratio}

Dec main() -> int:
    mut r = 1/2r
    quarantine Q_ext:
        let q = qext_from_ratio(r) + ext_decimal("0.5")
        emit q
    return 0
"#;
    assert_eq!(
        check_source(outer_mut_read, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ii) Hostile: `return` escaping `quarantine Q_ext:` in `main()`
    let escape_return = r#"
use aod::qext::ext_decimal

Dec main() -> int:
    quarantine Q_ext:
        let q = ext_decimal("1.25")
        if q == ext_decimal("1.25"):
            return 1
    return 0
"#;
    assert_eq!(
        check_source(escape_return, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ii-b) Hostile: `break` escaping `quarantine Q_ext:` into enclosing native loop
    let escape_break = r#"
use aod::qext::ext_decimal

Dec main() -> int:
    for i in 0..3:
        quarantine Q_ext:
            let q = ext_decimal("1.25")
            if q == ext_decimal("1.25"):
                break
    return 0
"#;
    assert_eq!(
        check_source(escape_break, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iii-a) Hostile: `ext_decimal("1.25")` in native helper `Dec`
    let ext_in_native_dec = r#"
use aod::qext::ext_decimal

Dec helper() -> int:
    let q = ext_decimal("1.25")
    return 0

Dec main() -> int:
    return helper()
"#;
    assert_eq!(
        check_source(ext_in_native_dec, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iii-b) Hostile: `ext_decimal("1.25")` in `chamber`
    let ext_in_chamber = r#"
use aod::qext::ext_decimal

chamber BadCh:
    space = ext_decimal("1.25")
    walls = open

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ext_in_chamber, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iii-c) Hostile: `ext_decimal("1.25")` in `prepare`
    let ext_in_prepare = r#"
use aod::qext::ext_decimal

chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare BadPrep in C:
    let bad = ext_decimal("1.25")
    inject 3:3:6 at (2, 2, 2) dir +X orient 1 shells 1

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ext_in_prepare, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iii-d) Hostile: `ext_decimal("1.25")` in `observe`
    let ext_in_observe = r#"
use aod::qext::ext_decimal

observe BadObs:
    tracer t(view):
        let bad = ext_decimal("1.25")

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ext_in_observe, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iv) Hostile: `quarantine Q_ext:` statement inside native helper `Dec`
    let qext_block_in_helper = r#"
use aod::qext::ext_decimal

Dec native_helper() -> int:
    quarantine Q_ext:
        let q = ext_decimal("1.25")
        emit q
    return 0

Dec main() -> int:
    return native_helper()
"#;
    assert_eq!(
        check_source(qext_block_in_helper, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (v) Hostile: referencing quarantine Q_ext const/fn from native code
    let qext_ref_from_native = r#"
use aod::qext::ext_decimal

quarantine Q_ext Cal:
    const K = ext_decimal("1.25")

Dec main() -> int:
    let x = Cal::K
    return 0
"#;
    assert_eq!(
        check_source(qext_ref_from_native, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (vi) Hostile: native function returning QExt<ratio> (`Dec forge() -> QExt<ratio>`)
    let native_fn_returning_qext = r#"
use aod::qext::ext_decimal

quarantine Q_ext Cal:
    const K = ext_decimal("1.25")

Dec forge() -> QExt<ratio>:
    return Cal::K

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(native_fn_returning_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (vii) Hostile: native function accepting QExt<ratio> (direct or wrapped in List/Tuple)
    let native_fn_taking_qext = r#"
Dec side(x: QExt<ratio>) -> int:
    return 0

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(native_fn_taking_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );
    let native_fn_taking_nested_qext = r#"
Dec side_list(xs: [QExt<ratio>]) -> int:
    return 0

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(native_fn_taking_nested_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (viii) Hostile: transitive `simulate` invoked via native helper from inside `quarantine Q_ext:`
    let transitive_sim_in_qext = r#"
chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare P in C:
    inject 3:3:6 at (2, 2, 2) dir +X orient +1t shells 1

Dec run_sim() -> int:
    let r = simulate P for 2 ticks
    return r.ticks

Dec wrapper() -> int:
    return run_sim()

Dec main() -> int:
    quarantine Q_ext:
        let t = wrapper()
        emit t
    return 0
"#;
    assert_eq!(
        check_source(transitive_sim_in_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ix) P0-A Hostile: `observe SneakyObs -> sensor s(view) -> main() -> quarantine Q_ext:`
    let sneaky_obs_to_main_qext = r#"
use aod::qext::ext_decimal

observe SneakyObs:
    sensor s(view) -> int:
        return main()

Dec main() -> int:
    quarantine Q_ext:
        let q = ext_decimal("1.25")
        emit q
    return 0
"#;
    assert_eq!(
        check_source(sneaky_obs_to_main_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (x) P2-2 Nested `>>` generics: `ReadOnly<QExt<ratio>>` parses cleanly & fails with QExtLeakageForbidden (NOT ParseError)
    let nested_generic_qext = r#"
Dec leak(x: ReadOnly<QExt<ratio>>) -> int:
    return 0

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(nested_generic_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (xi) P2-3 Positive: sibling `quarantine Q_ext ModA` called from `quarantine Q_ext ModB`
    let sibling_qext_mods = r#"
use aod::qext::{ext_decimal, qext_from_ratio, qext_render}

quarantine Q_ext ModA:
    const BASE = ext_decimal("2.0")
    Dec half(x: QExt<ratio>) -> QExt<ratio>:
        return x / BASE

quarantine Q_ext ModB:
    const QUARTER = ModA::BASE * ext_decimal("2.0")
    Dec scale_via_a(r: ratio) -> QExt<ratio>:
        let lifted = qext_from_ratio(r)
        return ModA::half(lifted) + lifted / QUARTER

Dec main() -> int:
    let native_r = 4/1r
    quarantine Q_ext:
        let out = ModB::scale_via_a(native_r)
        assert out == ext_decimal("3.0")
        emit qext_render(out)
    return 0
"#;
    let sib_out = run_source(sibling_qext_mods, CompatMode::V2).unwrap();
    assert_eq!(sib_out.to_stdout_string(), "Q_ext(3/1)\n0\n");
}

#[test]
fn p2_1_zero_silent_clamping_for_clock_shells_and_orient() {
    // 1. Clock period <= 0 rejected explicitly
    let bad_clock = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    clock bad_clk = 0
    inject 3:3:6 at (4, 4, 4) dir +X orient +1t shells 1

Dec main() -> int:
    let r = simulate P for 2 ticks
    return r.ticks
"#;
    assert_eq!(
        run_source(bad_clock, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::RuntimeError
    );

    // 2. Shells <= 0 rejected explicitly
    let bad_shells = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    inject 3:3:6 at (4, 4, 4) dir +X orient +1t shells 0

Dec main() -> int:
    let r = simulate P for 2 ticks
    return r.ticks
"#;
    assert_eq!(
        run_source(bad_shells, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::RuntimeError
    );

    // 3. Zero orientation `0t` rejected explicitly (no silent coercion to +1t!)
    let bad_orient = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    inject 3:3:6 at (4, 4, 4) dir +X orient 0t shells 1

Dec main() -> int:
    let r = simulate P for 2 ticks
    return r.ticks
"#;
    assert_eq!(
        run_source(bad_orient, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::RuntimeError
    );
}
