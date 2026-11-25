use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aod_core::json::{parse_strict_json, JsonValue};
use aodc::diagnostics::DiagnosticCode;
use aodc::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::{
    audit_dec_source, build_aot_file_with_options, check_file_with_options, check_source,
    check_source_with_options, compile_file_to_ir_with_options, jit_file_with_profile_captured,
    LoadOptions, OptLevel,
};

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aodc_r02_{}_{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn assert_6mode_strict(path: &Path, expected_stdout: &str, tmp: &Path, stem: &str) {
    let opts = LoadOptions::default();

    // 1. Exact Reference VM
    let checked_vm = check_file_with_options(path, &opts)
        .unwrap_or_else(|e| panic!("{stem} check failed: {e}"));
    let vm = Evaluator::new(&checked_vm, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap_or_else(|e| panic!("{stem} VM failed: {e}"))
        .to_stdout_string();
    assert_eq!(vm, expected_stdout, "{stem}: VM stdout mismatch");

    // 2. Hermetic DecIR v3
    let ir_json = compile_file_to_ir_with_options(path, &opts)
        .unwrap_or_else(|e| panic!("{stem} IR compile failed: {e}"));
    assert!(ir_json.contains("\"schema\": \"aod.dec.ir.v3\""));
    assert!(ir_json.contains("\"syntax_discipline\": \"pure_wave_v2\""));
    let checked =
        loads_ir(&ir_json).unwrap_or_else(|e| panic!("{stem} IR deserialize failed: {e}"));
    let ir_out = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap_or_else(|e| panic!("{stem} IR exec failed: {e}"))
        .to_stdout_string();
    assert_eq!(ir_out, expected_stdout, "{stem}: DecIR v3 stdout mismatch");

    // 3 & 4. AOT -O0 and AOT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_aot_{tag}"));
        build_aot_file_with_options(path, opt, &exe, &opts)
            .unwrap_or_else(|e| panic!("{stem} AOT -{tag} build failed: {e}"));
        let out = Command::new(&exe).output().unwrap();
        assert!(
            out.status.success(),
            "{stem} AOT -{tag} exec failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected_stdout,
            "{stem}: AOT -{tag} stdout mismatch"
        );
    }

    // 5 & 6. ORC LLJIT -O0 and ORC LLJIT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let (stdout, res, _, _) = jit_file_with_profile_captured(
            path,
            opt,
            DEFAULT_FUEL,
            DEFAULT_MAX_DEPTH,
            None,
            &opts,
        )
        .unwrap_or_else(|e| panic!("{stem} JIT -{tag} compile failed: {e}"));
        res.unwrap_or_else(|e| panic!("{stem} JIT -{tag} runtime failed: {e}"));
        assert_eq!(stdout, expected_stdout, "{stem}: JIT -{tag} stdout mismatch");
    }
}

fn assert_6mode_trap(src: &str, fuel: usize, tmp: &Path, stem: &str) {
    let opts = LoadOptions::default();
    let path = tmp.join(format!("{stem}.dec"));
    fs::write(&path, src).unwrap();

    let checked_vm = check_file_with_options(&path, &opts).unwrap();
    assert!(
        Evaluator::new(&checked_vm, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .run_main()
            .is_err(),
        "{stem}: expected VM trap"
    );
    let ir_json = compile_file_to_ir_with_options(&path, &opts).unwrap();
    let checked = loads_ir(&ir_json).unwrap();
    assert!(
        Evaluator::new(&checked, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .run_main()
            .is_err(),
        "{stem}: expected DecIR v3 trap"
    );
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_trap_{tag}"));
        build_aot_file_with_options(&path, opt, &exe, &opts).unwrap();
        let out = Command::new(&exe)
            .args(["--fuel", &fuel.to_string()])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{stem}: expected AOT -{tag} trap");
    }
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let (_, res, _, _) = jit_file_with_profile_captured(
            &path,
            opt,
            fuel,
            DEFAULT_MAX_DEPTH,
            None,
            &opts,
        )
        .unwrap();
        assert!(res.is_err(), "{stem}: expected JIT -{tag} trap");
    }
}

#[test]
fn ac01_ac02_dec_blocks_register_patterns_contextual_t1_and_struct_overloads() {
    let tmp = temp_dir("ac01_ac02");
    let opts = LoadOptions::default();

    let src = r#"
use aod::core:
    t1, q4, b8, b64, b64_bits

struct Vec2:
    x: int
    y: int

# Custom non-colliding `dec Vec2:` constructor overloads alongside canonical `Vec2(int, int)`!
dec Vec2:
    () -> Vec2:
        return Vec2(0, 0)
    (diag: int) -> Vec2:
        return Vec2(diag, diag)
    ((a, b): (int, int)) -> Vec2:
        return Vec2(a * 2, b * 2)

# Contextual `t1` bare integer patterns `(-1)`, `(0)`, `(1)` inferred from sibling typed arm!
dec classify_t1:
    (-1) -> int:
        return -10
    (0) -> int:
        return 0
    (1) -> int:
        return 10
    (_: t1) -> int:
        return 99

# Register pattern literals: `b8(...)`, `b64(...)`, `b64_bits(...)`, `q4(...)`
dec reg_dispatch:
    (b8(0), b64(-9841), b64_bits(0xFF), q4(15)) -> int:
        return 777
    (_: b8, _: b64, _: b64, _: q4) -> int:
        return -1

dec main:
    () -> int:
        let v0 = Vec2()
        let v1 = Vec2(5)
        let v2 = Vec2(3, 4)
        let v3 = Vec2((6, 7))
        assert (v0.x, v0.y) == (0, 0)
        assert (v1.x, v1.y) == (5, 5)
        assert (v2.x, v2.y) == (3, 4)
        assert (v3.x, v3.y) == (12, 14)

        assert classify_t1(t1(-1)) == -10
        assert classify_t1(0t) == 0
        assert classify_t1(1t) == 10

        assert reg_dispatch(b8(0), b64(-9841), b64_bits(0xFF), q4(15)) == 777
        assert reg_dispatch(b8(1), b64(-9841), b64_bits(0xFF), q4(15)) == -1

        # LetTuple destructuring with wildcard and optional element annotations
        let (ax: int, _) = (v3.x, v3.y)
        let (bx, by): (int, int) = (v1.x, v2.y)
        assert ax + bx + by == 21
        emit ax + bx + by
        return 0
"#;
    let p = tmp.join("reg_struct_ov.dec");
    fs::write(&p, src).unwrap();
    assert_6mode_strict(&p, "21\n0\n", &tmp, "reg_struct_ov");

    // Hostile controls (`P0-1`):
    // 1. Contextual `t1` out-of-bounds `(-2)` rejected with InvalidTritLiteral
    let bad_t1_pat = "use aod::core:\n    t1\ndec f:\n    (-2) -> int:\n        return 0\n    (_: t1) -> int:\n        return 1\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(bad_t1_pat, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::InvalidTritLiteral
    );

    // 2. Colliding `dec Vec2: (a: int, b: int) -> Vec2:` with canonical `struct Vec2` rejected!
    let collide_struct = "struct Vec2:\n    x: int\n    y: int\ndec Vec2:\n    (a: int, b: int) -> Vec2:\n        return Vec2(a, b)\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(collide_struct, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 3. Procedural observer without `ReadOnly<ChamberView>` rejected with ObserverMutationForbidden (`P0-1`)
    let non_ro_obs = "observe Obs:\n    tracer tr:\n        (v: ChamberView) -> null:\n            return null\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(non_ro_obs, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_ac05_all_10_afc_higher_order_operators_env_builtins_and_boundary_windows() {
    let tmp = temp_dir("ac04_ac05");
    let opts = LoadOptions::default();

    let src = r#"
use aod::core:
    b8, abs, evolve, scan, fold_seq, map_seq, filter_seq, tabulate, zip_seq, count, any, all

use aod::qext:
    ext_decimal, qext_add, qext_render_bytes

dec step_env:
    (acc: int, k: int, bias: int) -> int:
        return acc + k * bias

dec keep_above:
    (x: int, thresh: int) -> bool:
        return x > thresh

dec tab_with_scale:
    (k: int, scale: int) -> int:
        return k * scale

dec is_ascii_upper:
    (b: b8) -> bool:
        return b >= b8(65) and b <= b8(90)

dec trap_if_seen:
    (x: int) -> bool:
        assert x != 999
        return x == 2

quarantine Q_ext QPred:
    dec q_gt_two:
        (x: QExt<ratio>) -> QExt<bool>:
            return x > ext_decimal(b"2.0")

dec main:
    () -> int:
        # 1. `evolve` and `scan` over `range` AND half-open `window`, including `T32_LIMIT` & `i64::MAX` boundaries!
        let ev_rng = evolve(10, 1..4, step_env, 2)
        assert ev_rng == 22
        let ev_win = evolve(0, (0, 4], step_env, 3)
        assert ev_win == 30
        let sc_win = scan(0, (2, 5], step_env, 1)
        assert sc_win == [3, 7, 12]

        let max_tab = tabulate((9223372036854775805, 9223372036854775807], tab_with_scale, 1)
        assert max_tab == [9223372036854775806, 9223372036854775807]

        # 2. `map_seq` with `aod::core::abs` builtin callback + `filter_seq` + `tabulate` + `zip_seq`
        let abs_xs = map_seq([-5, 0, 7, -2], abs)
        assert abs_xs == [5, 0, 7, 2]
        let filt = filter_seq(abs_xs, keep_above, 3)
        assert filt == [5, 7]
        let tab = tabulate(1..5, tab_with_scale, 10)
        assert tab == [10, 20, 30, 40]
        let zipped = zip_seq(filt, [100, 200])
        assert zipped == [(5, 100), (7, 200)]

        # 3. `count`, `any`, `all` over `bytes` and `[int]`, including short-circuit proof!
        assert count(b"AoD_v3_R02", is_ascii_upper) == 3
        assert any([1, 2, 999], trap_if_seen)
        assert not all([2, 3, 999], trap_if_seen)

        # 4. Quarantined `fold_seq` + `any` returning `QExt<bool>` inside `quarantine Q_ext:`
        quarantine Q_ext:
            let qs = [ext_decimal(b"1.5"), ext_decimal(b"2.5")]
            let qsum = fold_seq(ext_decimal(b"0.0"), qs, qext_add)
            let qhas = any(qs, QPred::q_gt_two)
            emit qext_render_bytes(qsum)
            emit qhas

        emit ev_win
        return 0
"#;
    let p = tmp.join("ho_all10.dec");
    fs::write(&p, src).unwrap();
    assert_6mode_strict(&p, "Q_ext(b\"4/1\")\nQ_ext(true)\n30\n0\n", &tmp, "ho_all10");

    // Hostile runtime & compile-time controls:
    assert_6mode_trap(
        "use aod::core:\n    zip_seq\ndec main:\n    () -> int:\n        let _z = zip_seq([1, 2], [1, 2, 3])\n        return 0\n",
        10_000,
        &tmp,
        "zip_mismatch",
    );
    assert_6mode_trap(
        "use aod::core:\n    tabulate\ndec id:\n    (k: int) -> int:\n        return k\ndec main:\n    () -> int:\n        let _t = tabulate(0..10000, id)\n        return 0\n",
        25,
        &tmp,
        "tabulate_fuel",
    );
    assert_6mode_trap(
        "use aod::core:\n    tabulate, pow\ndec id:\n    (k: int) -> int:\n        return k\ndec main:\n    () -> int:\n        let _t = tabulate(0..pow(10, 30), id)\n        return 0\n",
        DEFAULT_FUEL,
        &tmp,
        "tabulate_prealloc_cap",
    );

    let zip_arr_mismatch = "use aod::core:\n    zip_seq\ndec main:\n    () -> int:\n        let a: [int; 2] = [1, 2]\n        let b: [int; 3] = [1, 2, 3]\n        let _z = zip_seq(a, b)\n        return 0\n";
    assert_eq!(
        check_source_with_options(zip_arr_mismatch, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    let ro_leak = "use aod::core:\n    map_seq, list_set\ndec mut_step:\n    (xs: [int]) -> [int]:\n        return list_set(xs, 0, 99)\ndec bad:\n    (ro: ReadOnly<[[int]]>) -> [[int]]:\n        return map_seq(ro, mut_step)\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(ro_leak, &opts).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    let qext_leak = "use aod::core::map_seq\nquarantine Q_ext QMod:\n    dec qf:\n        (x: int) -> int:\n            return x + 1\ndec main:\n    () -> int:\n        let _ys = map_seq([1, 2], QMod::qf)\n        return 0\n";
    assert_eq!(
        check_source_with_options(qext_leak, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac06_ac07_ac08_pure_containers_contraction_b8_t1_qext_and_math_extensions() {
    let tmp = temp_dir("ac06_ac08");

    let src = r#"
use aod::core:
    t1, b8, ratio, len, contains
    map_put, map_remove, map_get_or, map_keys, map_entries
    list_set, list_push, concat_seq, slice_seq
    sum, dot, num, den, signum, clamp, abs, min, max, gcd, lcm, pow

use aod::qext:
    ext_decimal, qext_render_bytes, qext_format_fixed_bytes

dec main:
    () -> int:
        # 1. All 9 pure persistent map & list helpers (original bindings remain untouched!)
        let m0: map[int, int] = map[(2, 20), (1, 10)]
        let m1 = map_put(m0, 3, 30)
        let m2 = map_remove(m1, 2)
        assert len(m0) == 2 and not contains(m0, 3)
        assert len(m1) == 3 and map_get_or(m1, 3, -1) == 30 and map_get_or(m2, 2, -1) == -1
        assert map_keys(m2) == [1, 3]
        assert map_entries(m2) == [(1, 10), (3, 30)]

        let xs0 = [10, 20, 30]
        let xs1 = list_set(xs0, 1, 99)
        let xs2 = list_push(xs1, 40)
        let xs3 = concat_seq(xs0, [40, 50])
        let xs4 = slice_seq(xs3, 1, 4)
        assert xs0 == [10, 20, 30]
        assert xs2 == [10, 99, 30, 40]
        assert xs4 == [20, 30, 40]

        # 2. Contractions on `b8` (including `bytes`) and `t1`, plus `r.num`/`r.den`, `num`/`den`, `signum`, `clamp`, and Trit-Boxed `int` math!
        assert sum(b"\x0a\x14\x1e") == b8(60)
        assert dot([b8(2), b8(3)], [b8(4), b8(5)]) == b8(23)
        assert sum([1t, t1(-1), 0t, 1t]) == 1t
        assert dot([1t, t1(-1), 1t], [1t, 1t, 0t]) == 0t

        let rat = -14/21r
        assert (rat.num, rat.den) == (-2, 3)
        assert (num(rat), den(rat)) == (-2, 3)
        assert signum(rat) == -1t and signum(0) == 0t and signum(99) == 1t
        assert clamp(-15, -10, 10) == -10 and clamp(25, -10, 10) == 10
        assert pow(-3, 4) == 81 and pow(-5, 3) == -125
        assert gcd(-84, 30) == 6 and lcm(-12, 18) == 36
        assert gcd(-100, 75) == 25 and lcm(21, 6) == 42

        # 3. `sum`, `dot`, `abs`, `min`, `max`, `qext_render_bytes`, `qext_format_fixed_bytes` in Q_ext
        quarantine Q_ext:
            let qa = ext_decimal(b"-1.25")
            let qb = ext_decimal(b"2.00")
            assert abs(qa) == ext_decimal(b"1.25")
            assert min(qa, qb) == qa and max(qa, qb) == qb
            let qsum = sum([qa, qb, ext_decimal(b"0.50")])
            let qdot = dot([qa, qb], [ext_decimal(b"2.0"), ext_decimal(b"3.0")])
            emit qext_render_bytes(qsum)
            emit qext_format_fixed_bytes(qdot, 2)

        return 0
"#;
    let p = tmp.join("containers_math.dec");
    fs::write(&p, src).unwrap();
    assert_6mode_strict(
        &p,
        "Q_ext(b\"5/4\")\nQ_ext(b\"3.50\")\n0\n",
        &tmp,
        "containers_math",
    );

    // Hostile runtime traps across all 6 modes:
    for (stem, bad_src) in [
        (
            "sum_t1_ovf",
            "use aod::core:\n    sum\ndec main:\n    () -> t1:\n        return sum([1t, 1t])\n",
        ),
        (
            "sum_b8_ovf",
            "use aod::core:\n    b8, sum\ndec main:\n    () -> b8:\n        return sum([b8(200), b8(100)])\n",
        ),
        (
            "dot_b8_ovf",
            "use aod::core:\n    b8, dot\ndec main:\n    () -> b8:\n        return dot([b8(20)], [b8(20)])\n",
        ),
    ] {
        assert_6mode_trap(bad_src, DEFAULT_FUEL, &tmp, stem);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac13_audit_dec_constant_folding_over_register_casts_and_cli_fail_json() {
    let tmp = temp_dir("ac13_audit");

    // 1. Clean-break frontend unconditionally rejects `Dec`, `mut`, `while`, `str`
    let legacy_src = "Dec main() -> int:\n    mut i = 0\n    let s: str = \"ok\"\n    while i < 2:\n        i = i + 1\n    return i\n";
    assert!(check_source(legacy_src).is_err());

    // 2. `aodc audit-dec --json` on banned construct emits valid `aod.dec.audit.v1` JSON (`status: "FAIL"`) and exits 1!
    let legacy_path = tmp.join("legacy_for_audit.dec");
    fs::write(&legacy_path, legacy_src).unwrap();
    let aud_fail = Command::new(aodc_bin())
        .args(["audit-dec", "--json"])
        .arg(&legacy_path)
        .output()
        .unwrap();
    assert_eq!(aud_fail.status.code(), Some(1));
    let parsed_json = parse_strict_json(&String::from_utf8_lossy(&aud_fail.stdout))
        .expect("audit-dec --json must emit valid JSON on stdout even on failure");
    assert_eq!(
        parsed_json.get("schema").and_then(JsonValue::as_str),
        Some("aod.dec.audit.v1")
    );
    assert_eq!(
        parsed_json.get("status").and_then(JsonValue::as_str),
        Some("FAIL")
    );

    // 3. `audit-dec` constant folding over register casts `b8(...)`, `b64(...)`, `int(...)` (`AC-13`)!
    let reg_audit_ok = r#"
use aod::core:
    int, b8, b64, verify_sheddic_route, verify_frg_cross_return

const XS: int = int(b64(3))
const ENC: b8 = b8(4)
const T0: int = int(b64(10))
const CX: int = int(b8(5))

dec main:
    () -> int:
        assert verify_sheddic_route(XS, ENC)
        assert verify_frg_cross_return(T0, T0 + CX, CX)
        return 0
"#;
    let rep_ok = audit_dec_source(reg_audit_ok);
    assert!(rep_ok.passed() && rep_ok.hooks_passed() == 6, "{:?}", rep_ok.errors);

    let reg_audit_forged = r#"
use aod::core:
    int, b8, b64, verify_frg_cross_return

const T0: int = int(b64(10))
const CX: int = int(b8(4))

dec main:
    () -> int:
        assert verify_frg_cross_return(T0, 15, CX)
        return 0
"#;
    let rep_forged = audit_dec_source(reg_audit_forged);
    assert!(!rep_forged.passed());
    assert!(!rep_forged.frg_audit.passed());

    let _ = fs::remove_dir_all(&tmp);
}
