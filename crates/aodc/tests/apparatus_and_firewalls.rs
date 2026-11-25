use std::path::PathBuf;

use aodc::diagnostics::{CompatMode, DiagnosticCode};
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
    assert_eq!(run1.emits[0], "ModelStatus::DeclaredSandboxNotS3Authenticated");

    // Explicitly verify bit-for-bit identical final_state_hash across 0, 1, and 4 observers!
    let multi_obs = r#"
use aod::core:
    t1, sum

use aod::sim::ModelStatus

chamber BoxCh:
    space = box([0..64, 0..64, 0..64])
    walls = closed

prepare Opposing in BoxCh:
    clock ref_clk = 2
    inject 3:3:6 at (26, 32, 32) dir +X orient 1t shells 2
    inject 3:3:6 at (38, 32, 32) dir -X orient t1(-1) shells 2

observe Obs1:
    tracer tr1:
        region = box([20..44, 24..40, 24..40])
        capture = [support, returns, hinges, shedding]

observe Obs2:
    sensor s2:
        region = box([0..64, 0..64, 0..64])
        capture = [crossings, cadence]

observe Obs3:
    tracer p3:
        (view: ReadOnly<ChamberView>) -> Null:
            let _acc = sum(view.carriers)
            return null

observe Obs4:
    sensor p4:
        (view: ReadOnly<ChamberView>) -> int:
            return view.tick + view.carrier_count

dec main:
    () -> int:
        let r0 = simulate Opposing for 20 ticks
        let r1 = simulate Opposing for 20 ticks with Obs1
        let r4 = simulate Opposing for 20 ticks with Obs1, Obs2, Obs3, Obs4
        assert r0.model_status == ModelStatus::DeclaredSandboxNotS3Authenticated
        assert r0.final_state_hash == r1.final_state_hash
        assert r0.final_state_hash == r4.final_state_hash
        return 0
"#;
    let out = run_source(multi_obs, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "0\n");
}

#[test]
fn ac17_readonly_observer_firewall_direct_and_transitive_mutation_rejected() {
    // 1. Non-ReadOnly ChamberView parameter inside observe
    let non_ro_view = r#"
observe BadObs:
    tracer t:
        (view: ChamberView) -> Null:
            return null

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(non_ro_view, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 2. Calling list_set on `view.carriers` (`ReadOnly<[int]>`)
    let ro_list_set = r#"
use aod::core::list_set

observe BadObs:
    tracer t:
        (view: ReadOnly<ChamberView>) -> Null:
            let _bad = list_set(view.carriers, 0, 42)
            return null

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(ro_list_set, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 3. Calling `simulate` inside `observe`
    let obs_sim = r#"
chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare P in C:
    inject 3:3:6 at (2, 2, 2) dir +X orient 1t shells 1

observe BadObs:
    tracer t:
        (view: ReadOnly<ChamberView>) -> Null:
            let _r = simulate P for 1 ticks
            return null

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(obs_sim, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 4. P1-3 TRANSITIVE helper stripping ReadOnly<ChamberView>: observe -> helper_a(view) -> helper_b(v: ChamberView)
    let transitive_strip_ro = r#"
dec helper_b:
    (v: ChamberView) -> int:
        return v.tick

dec helper_a:
    (v: ReadOnly<ChamberView>) -> int:
        return helper_b(v)

observe BadTransitive:
    sensor s:
        (view: ReadOnly<ChamberView>) -> int:
            return helper_a(view)

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(transitive_strip_ro, CompatMode::V2)
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
    inject 3:3:6 at (2, 2, 2) dir +X orient 1t shells 1

dec run_hidden:
    () -> int:
        let r = simulate P for 2 ticks
        return r.ticks

dec wrapper:
    () -> int:
        return run_hidden()

observe BadTransitiveSim:
    sensor s:
        (view: ReadOnly<ChamberView>) -> int:
            return wrapper() + view.tick

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(transitive_sim, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 6. Positive control: pure read-only helper allowed!
    let pure_helper_ok = r#"
use aod::core::sum

dec count_plus:
    (v: ReadOnly<ChamberView>, offset: int) -> int:
        return offset + sum(v.carriers)

observe GoodObs:
    sensor s:
        (view: ReadOnly<ChamberView>) -> int:
            return count_plus(view, 1)

dec main:
    () -> int:
        return 0
"#;
    assert!(check_source(pure_helper_ok, CompatMode::V2).is_ok());

    // 7. Validator R03 Probe 1: `main() -> take_mut_list(run.final_snapshot.carriers)` (`ReadOnly<[int]>`)
    let ro_carriers_to_multiarm = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    inject 3:3:6 at (4, 4, 4) dir +X orient 1t shells 1

dec take_mut_list:
    (xs: [int]) -> int:
        return xs[0]

dec main:
    () -> int:
        let run = simulate P for 2 ticks
        return take_mut_list(run.final_snapshot.carriers)
"#;
    assert_eq!(
        check_source(ro_carriers_to_multiarm, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 8. Validator R03 Probe 2: `dec take_ro: (ro: ReadOnly<S>) -> int: return take_non_ro(ro)`
    let ro_struct_to_multiarm = r#"
struct S:
    x: int

dec take_non_ro:
    (s: S) -> int:
        return s.x

dec take_ro:
    (ro: ReadOnly<S>) -> int:
        return take_non_ro(ro)

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(ro_struct_to_multiarm, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 9. Validator R03 Probe 3: `ReadOnly<map[bytes, int]>` passed to non-ReadOnly map parameter
    let ro_map_to_multiarm = r#"
dec take_non_ro_map:
    (m: map[bytes, int]) -> int:
        return m[b"k"]

dec take_ro_map:
    (ro: ReadOnly<map[bytes, int]>) -> int:
        return take_non_ro_map(ro)

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(ro_map_to_multiarm, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 10. Positional `Struct(ro)` cannot strip `ReadOnly<T>`
    let pos_struct_strip_ro = r#"
struct Inner:
    x: int

struct Wrap:
    inner: Inner

dec take_ro:
    (ro: ReadOnly<Inner>) -> int:
        let w = Wrap(ro)
        return w.inner.x

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(pos_struct_strip_ro, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 11. Positional `Enum::Var(ro)` cannot strip `ReadOnly<T>`
    let pos_enum_strip_ro = r#"
struct Inner:
    x: int

enum Boxed:
    Hold(Inner)

dec unwrap_hold:
    (Boxed::Hold(v)) -> int:
        return v.x

dec take_ro:
    (ro: ReadOnly<Inner>) -> int:
        let b = Boxed::Hold(ro)
        return unwrap_hold(b)

dec main:
    () -> int:
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
    assert_eq!(out.to_stdout_string(), "Q_ext(b\"1/1\")\n4/5\n0\n");

    // (i) Hostile: `return` escaping `quarantine Q_ext:` in `main`
    let escape_return = r#"
use aod::qext::ext_decimal

dec main:
    () -> int:
        quarantine Q_ext:
            let _q = ext_decimal(b"1.25")
            return 1
"#;
    assert_eq!(
        check_source(escape_return, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ii-a) Hostile: `ext_decimal(b"1.25")` in native helper `dec`
    let ext_in_native_dec = r#"
use aod::qext::ext_decimal

dec helper:
    () -> int:
        let _q = ext_decimal(b"1.25")
        return 0

dec main:
    () -> int:
        return helper()
"#;
    assert_eq!(
        check_source(ext_in_native_dec, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ii-b) Hostile: `ext_decimal(b"1.25")` in `chamber`
    let ext_in_chamber = r#"
use aod::qext::ext_decimal

chamber BadCh:
    space = ext_decimal(b"1.25")
    walls = open

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(ext_in_chamber, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ii-c) Hostile: `ext_decimal(b"1.25")` in `prepare`
    let ext_in_prepare = r#"
use aod::qext::ext_decimal

chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare BadPrep in C:
    let bad = ext_decimal(b"1.25")
    inject 3:3:6 at (2, 2, 2) dir +X orient 1t shells 1

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(ext_in_prepare, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ii-d) Hostile: `ext_decimal(b"1.25")` in `observe`
    let ext_in_observe = r#"
use aod::qext::ext_decimal

observe BadObs:
    tracer t:
        (view: ReadOnly<ChamberView>) -> Null:
            let _bad = ext_decimal(b"1.25")
            return null

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(ext_in_observe, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iii) Hostile: `quarantine Q_ext:` statement inside native helper `dec`
    let qext_block_in_helper = r#"
use aod::qext::ext_decimal

dec native_helper:
    () -> int:
        quarantine Q_ext:
            let q = ext_decimal(b"1.25")
            emit q
        return 0

dec main:
    () -> int:
        return native_helper()
"#;
    assert_eq!(
        check_source(qext_block_in_helper, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (iv) Hostile: referencing quarantine Q_ext const/fn from native code
    let qext_ref_from_native = r#"
use aod::qext::ext_decimal

quarantine Q_ext Cal:
    const K = ext_decimal(b"1.25")

dec main:
    () -> int:
        let _x = Cal::K
        return 0
"#;
    assert_eq!(
        check_source(qext_ref_from_native, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (v) Hostile: native function returning QExt<ratio>
    let native_fn_returning_qext = r#"
use aod::qext::ext_decimal

quarantine Q_ext Cal:
    const K = ext_decimal(b"1.25")

dec forge:
    () -> QExt<ratio>:
        return Cal::K

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(native_fn_returning_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (vi) Hostile: native function accepting QExt<ratio> (direct or wrapped in List/ReadOnly)
    let native_fn_taking_qext = r#"
dec side:
    (x: QExt<ratio>) -> int:
        return 0

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(native_fn_taking_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );
    let native_fn_taking_nested_qext = r#"
dec side_list:
    (xs: [QExt<ratio>]) -> int:
        return 0

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(native_fn_taking_nested_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (vii) Hostile: transitive `simulate` invoked via native helper from inside `quarantine Q_ext:`
    let transitive_sim_in_qext = r#"
chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare P in C:
    inject 3:3:6 at (2, 2, 2) dir +X orient 1t shells 1

dec run_sim:
    () -> int:
        let r = simulate P for 2 ticks
        return r.ticks

dec wrapper:
    () -> int:
        return run_sim()

dec main:
    () -> int:
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

    // (viii) P0-A Hostile: `observe SneakyObs -> sensor s -> main() -> quarantine Q_ext:`
    let sneaky_obs_to_main_qext = r#"
use aod::qext::ext_decimal

observe SneakyObs:
    sensor s:
        (view: ReadOnly<ChamberView>) -> int:
            return main() + view.tick

dec main:
    () -> int:
        quarantine Q_ext:
            let q = ext_decimal(b"1.25")
            emit q
        return 0
"#;
    assert_eq!(
        check_source(sneaky_obs_to_main_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (ix) P2-2 Nested `>>` generics: `ReadOnly<QExt<ratio>>` parses cleanly & fails with QExtLeakageForbidden (NOT ParseError)
    let nested_generic_qext = r#"
dec leak:
    (x: ReadOnly<QExt<ratio>>) -> int:
        return 0

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(nested_generic_qext, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // (x) P2-3 Positive: sibling `quarantine Q_ext ModA` called from `quarantine Q_ext ModB`
    let sibling_qext_mods = r#"
use aod::qext:
    ext_decimal, qext_from_ratio, qext_render

quarantine Q_ext ModA:
    const BASE = ext_decimal(b"2.0")

    dec half:
        (x: QExt<ratio>) -> QExt<ratio>:
            return x / BASE

quarantine Q_ext ModB:
    const QUARTER = ModA::BASE * ext_decimal(b"2.0")

    dec scale_via_a:
        (r: ratio) -> QExt<ratio>:
            let lifted = qext_from_ratio(r)
            return ModA::half(lifted) + lifted / QUARTER

dec main:
    () -> int:
        let native_r = 4/1r
        quarantine Q_ext:
            let out = ModB::scale_via_a(native_r)
            assert out == ext_decimal(b"3.0")
            emit qext_render(out)
        return 0
"#;
    let sib_out = run_source(sibling_qext_mods, CompatMode::V2).unwrap();
    assert_eq!(sib_out.to_stdout_string(), "Q_ext(b\"3/1\")\n0\n");
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
    inject 3:3:6 at (4, 4, 4) dir +X orient 1t shells 1

dec main:
    () -> int:
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
    inject 3:3:6 at (4, 4, 4) dir +X orient 1t shells 0

dec main:
    () -> int:
        let r = simulate P for 2 ticks
        return r.ticks
"#;
    assert_eq!(
        run_source(bad_shells, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::RuntimeError
    );

    // 3. Zero orientation `0t` rejected explicitly (no silent coercion to 1t!)
    let bad_orient = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    inject 3:3:6 at (4, 4, 4) dir +X orient 0t shells 1

dec main:
    () -> int:
        let r = simulate P for 2 ticks
        return r.ticks
"#;
    assert_eq!(
        run_source(bad_orient, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::RuntimeError
    );
}
