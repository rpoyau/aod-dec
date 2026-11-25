use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aod_core::json::{parse_strict_json, JsonValue};
use aodc::diagnostics::{CompatMode, DiagnosticCode, SyntaxDiscipline};
use aodc::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::{
    audit_dec_source_with_options, build_aot_file_with_options, check_file_with_options,
    check_source, check_source_with_options, compile_file_to_ir_with_options,
    format_source_with_discipline, jit_file_with_profile_captured, LoadOptions, OptLevel,
};

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aodc_r02_{}_{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn strict_opts() -> LoadOptions {
    LoadOptions {
        stdlib_dir: None,
        discipline: SyntaxDiscipline::PureWaveV2,
    }
}

fn assert_6mode_strict(path: &Path, expected_stdout: &str, tmp: &Path, stem: &str) {
    let opts = strict_opts();

    // 1. Exact Reference VM
    let checked_vm = check_file_with_options(path, CompatMode::V2, &opts)
        .unwrap_or_else(|e| panic!("{stem} check failed: {e}"));
    let vm = Evaluator::new(&checked_vm, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap_or_else(|e| panic!("{stem} VM failed: {e}"))
        .to_stdout_string();
    assert_eq!(vm, expected_stdout, "{stem}: VM stdout mismatch");

    // 2. Hermetic DecIR v2
    let ir_json = compile_file_to_ir_with_options(path, CompatMode::V2, &opts)
        .unwrap_or_else(|e| panic!("{stem} IR compile failed: {e}"));
    let checked =
        loads_ir(&ir_json).unwrap_or_else(|e| panic!("{stem} IR deserialize failed: {e}"));
    assert_eq!(checked.unit.discipline, SyntaxDiscipline::PureWaveV2);
    let ir_out = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap_or_else(|e| panic!("{stem} IR exec failed: {e}"))
        .to_stdout_string();
    assert_eq!(ir_out, expected_stdout, "{stem}: DecIR v2 stdout mismatch");

    // 3 & 4. AOT -O0 and AOT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_aot_{tag}"));
        build_aot_file_with_options(path, CompatMode::V2, opt, &exe, &opts)
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
            CompatMode::V2,
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
    let opts = strict_opts();
    let path = tmp.join(format!("{stem}.dec"));
    fs::write(&path, src).unwrap();

    let checked_vm = check_file_with_options(&path, CompatMode::V2, &opts).unwrap();
    assert!(
        Evaluator::new(&checked_vm, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .run_main()
            .is_err(),
        "{stem}: expected VM trap"
    );
    let ir_json = compile_file_to_ir_with_options(&path, CompatMode::V2, &opts).unwrap();
    let checked = loads_ir(&ir_json).unwrap();
    assert!(
        Evaluator::new(&checked, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .run_main()
            .is_err(),
        "{stem}: expected DecIR v2 trap"
    );
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_trap_{tag}"));
        build_aot_file_with_options(&path, CompatMode::V2, opt, &exe, &opts).unwrap();
        let out = Command::new(&exe)
            .args(["--fuel", &fuel.to_string()])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{stem}: expected AOT -{tag} trap");
    }
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let (_, res, _, _) = jit_file_with_profile_captured(
            &path,
            CompatMode::V2,
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
    let opts = strict_opts();

    let src = r#"
use aod::core:
    t1, t9, t32, q4, u8, b64

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

# Register pattern literals: `u8(...)`, `b64(...)`, `t9(...)`, `t32(...)`, `q4(...)`
dec reg_dispatch:
    (u8(0), b64(42), t9(-9841), t32(100), q4(15)) -> int:
        return 777
    (_: u8, _: b64, _: t9, _: t32, _: q4) -> int:
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

        assert reg_dispatch(u8(0), b64(42), t9(-9841), t32(100), q4(15)) == 777
        assert reg_dispatch(u8(1), b64(42), t9(-9841), t32(100), q4(15)) == -1

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
        check_source_with_options(bad_t1_pat, CompatMode::V2, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::InvalidTritLiteral
    );

    // 2. Colliding `dec Vec2: (a: int, b: int) -> Vec2:` with canonical `struct Vec2` rejected!
    let collide_struct = "struct Vec2:\n    x: int\n    y: int\ndec Vec2:\n    (a: int, b: int) -> Vec2:\n        return Vec2(a, b)\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(collide_struct, CompatMode::V2, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 3. `return ()`, `-> ()`, `let x: () = ...`, and `observe` probe `-> ()` all rejected in `--strict-wave` (`P0-1`)!
    for bad_unit in [
        "dec f:\n    () -> ():\n        return ()\ndec main:\n    () -> int:\n        return 0\n",
        "dec main:\n    () -> int:\n        return ()\n",
        "observe Obs:\n    tracer tr:\n        (v: ReadOnly<ChamberView>) -> ():\n            assert v.tick >= 0\ndec main:\n    () -> int:\n        return 0\n",
        "dec main:\n    () -> int:\n        let (_, _) = (1, 2)\n        return 0\n",
    ] {
        assert!(
            check_source_with_options(bad_unit, CompatMode::V2, &opts).is_err(),
            "expected --strict-wave rejection for:\n{bad_unit}"
        );
    }

    // 4. Procedural observer without `ReadOnly<ChamberView>` rejected with ObserverMutationForbidden (`P0-1`)
    let non_ro_obs = "observe Obs:\n    tracer tr:\n        (v: ChamberView) -> Null:\n            return null\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(non_ro_obs, CompatMode::V2, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_ac05_all_10_afc_higher_order_operators_env_builtins_and_i64_max_window() {
    let tmp = temp_dir("ac04_ac05");
    let opts = strict_opts();

    let src = r#"
use aod::core:
    u8, abs, evolve, scan, fold_seq, map_seq, filter_seq, tabulate, zip_seq, count, any, all

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
    (b: u8) -> bool:
        return b >= u8(65) and b <= u8(90)

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
        # 1. `evolve` and `scan` over `range` AND half-open `window`, including `v == i64::MAX` (`P0-3`)!
        let ev_rng = evolve(10, 1..4, step_env, 2)
        assert ev_rng == 22
        let ev_win = evolve(0, (0, 4], step_env, 3)
        assert ev_win == 30
        let sc_win = scan(0, (2, 5], step_env, 1)
        assert sc_win == [3, 7, 12]

        # Exact i64::MAX boundary window `(9223372036854775805, 9223372036854775807]` -> k = MAX-1, MAX!
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
        assert count(b"AoD_v2_R02", is_ascii_upper) == 3
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
    let zip_arr_mismatch = "use aod::core:\n    zip_seq\ndec main:\n    () -> int:\n        let a: array[int, 2] = [1, 2]\n        let b: array[int, 3] = [1, 2, 3]\n        let _z = zip_seq(a, b)\n        return 0\n";
    assert_eq!(
        check_source_with_options(zip_arr_mismatch, CompatMode::V2, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac06_ac07_ac08_pure_containers_contraction_u8_t1_qext_and_math_extensions() {
    let tmp = temp_dir("ac06_ac08");

    let src = r#"
use aod::core:
    t1, t9, t32, u8, ratio, len, contains
    map_put, map_remove, map_get_or, map_keys, map_entries
    list_set, list_push, concat_seq, slice_seq
    sum, dot, abs, min, max, gcd, lcm, pow

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

        # 2. Contractions on `u8` (including `bytes`) and `t1`, plus `r.num` / `r.den` and T9/T32 math!
        assert sum(b"\x0a\x14\x1e") == u8(60)
        assert dot([u8(2), u8(3)], [u8(4), u8(5)]) == u8(23)
        assert sum([1t, t1(-1), 0t, 1t]) == 1t
        assert dot([1t, t1(-1), 1t], [1t, 1t, 0t]) == 0t

        let rat = -14/21r
        assert (rat.num, rat.den) == (-2, 3)
        assert pow(t9(-3), 4) == t9(81) and pow(t9(2), t9(5)) == t9(32)
        assert pow(t32(-5), 3) == t32(-125) and pow(t32(3), t32(4)) == t32(81)
        assert gcd(t9(-84), t9(30)) == t9(6) and lcm(t9(-12), t9(18)) == t9(36)
        assert gcd(t32(-100), t32(75)) == t32(25) and lcm(t32(21), t32(6)) == t32(42)

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
            "sum_u8_ovf",
            "use aod::core:\n    u8, sum\ndec main:\n    () -> u8:\n        return sum([u8(200), u8(100)])\n",
        ),
        (
            "lcm_t9_ovf",
            "use aod::core:\n    t9, lcm\ndec main:\n    () -> t9:\n        return lcm(t9(9841), t9(2))\n",
        ),
        (
            "pow_t9_neg",
            "use aod::core:\n    t9, pow\ndec main:\n    () -> t9:\n        return pow(t9(2), -1)\n",
        ),
        (
            "slice_bad_bounds",
            "use aod::core:\n    slice_seq\ndec main:\n    () -> int:\n        let _s = slice_seq([1, 2, 3], 2, 1)\n        return 0\n",
        ),
    ] {
        assert_6mode_trap(bad_src, 10_000, &tmp, stem);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_ac10_stdlib_and_all_18_manual_examples_strict_wave_6mode_and_audit() {
    let root = repo_root();
    let tmp = temp_dir("ac09_ac10");
    let opts = strict_opts();

    // 1. All 7 `stdlib/aod/*.dec` modules pass `check --strict-wave`, idempotent `fmt --strict-wave`, and `audit --strict-wave` (6/6)!
    let stdlib_dir = root.join("stdlib/aod");
    let mut stdlib_count = 0usize;
    for ent in fs::read_dir(&stdlib_dir).unwrap() {
        let p = ent.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("dec") {
            continue;
        }
        stdlib_count += 1;
        let raw = fs::read_to_string(&p).unwrap();
        let loaded =
            aodc::modules::load_from_source_with_options(&raw, p.parent(), CompatMode::V2, &opts)
                .unwrap_or_else(|e| panic!("stdlib {} failed --strict-wave load: {e}", p.display()));
        aodc::checker::check_unit(loaded, false)
            .unwrap_or_else(|e| panic!("stdlib {} failed --strict-wave check: {e}", p.display()));
        let f1 = format_source_with_discipline(&raw, CompatMode::V2, SyntaxDiscipline::PureWaveV2)
            .unwrap_or_else(|e| panic!("stdlib {} failed fmt --strict-wave: {e}", p.display()));
        let f2 = format_source_with_discipline(&f1, CompatMode::V2, SyntaxDiscipline::PureWaveV2)
            .unwrap();
        assert_eq!(f1, f2, "fmt idempotence failed on {}", p.display());

        let rep = audit_dec_source_with_options(
            &raw,
            &p.to_string_lossy(),
            p.parent(),
            CompatMode::V2,
            opts.clone(),
        );
        assert!(
            rep.passed() && rep.hooks_passed() == 6,
            "stdlib {} failed audit --strict-wave: {:?}",
            p.display(),
            rep.errors
        );
    }
    assert_eq!(stdlib_count, 7);

    // 2. All 11 positive `manual_examples/01..11.dec` pass `--strict-wave` across all 6 modes!
    let manual_dir = root.join("manual_examples");
    let mut pos_files: Vec<PathBuf> = fs::read_dir(&manual_dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("dec") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    pos_files.sort();
    assert_eq!(pos_files.len(), 11);
    for dec_p in &pos_files {
        let stem = dec_p.file_stem().unwrap().to_str().unwrap();
        let expected = fs::read_to_string(dec_p.with_extension("stdout")).unwrap();
        assert_6mode_strict(dec_p, &expected, &tmp, stem);
    }

    // 3. `aodc test --strict-wave manual_examples` -> 18 passed; 0 failed; 18 total!
    let test_out = Command::new(aodc_bin())
        .args(["test", "--strict-wave"])
        .arg(&manual_dir)
        .output()
        .unwrap();
    assert!(
        test_out.status.success(),
        "aodc test --strict-wave manual_examples failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&test_out.stdout).contains("18 passed; 0 failed; 18 total"),
        "unexpected test summary: {}",
        String::from_utf8_lossy(&test_out.stdout)
    );

    // 4. `aodc manual-data --check` passes with zero JSON drift!
    let md_out = Command::new(aodc_bin())
        .args(["manual-data", "--check"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        md_out.status.success(),
        "aodc manual-data --check failed: {}",
        String::from_utf8_lossy(&md_out.stderr)
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac03_ac11_ac12_ac13_cli_strict_wave_flags_audit_json_and_register_fold() {
    let root = repo_root();
    let tmp = temp_dir("ac03_ac13");

    // 1. `--help` lists `--strict-wave` as canonical
    let help = Command::new(aodc_bin()).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help_txt = String::from_utf8_lossy(&help.stdout);
    assert!(help_txt.contains("--strict-wave"));
    assert!(help_txt.contains("emit-ir"));
    assert!(help_txt.contains("fmt"));
    assert!(help_txt.contains("audit"));

    // 2. Both `--strict-wave` and `--pure-wave` work across `check`, `run`, `build`, `emit-ir`, `fmt`, `audit`
    let ex01 = root.join("manual_examples/01_dec_ledger.dec");
    for flag in ["--strict-wave", "--pure-wave"] {
        assert!(Command::new(aodc_bin())
            .args(["check", flag])
            .arg(&ex01)
            .output()
            .unwrap()
            .status
            .success());
        assert!(Command::new(aodc_bin())
            .args(["run", flag])
            .arg(&ex01)
            .output()
            .unwrap()
            .status
            .success());
        let exe = tmp.join(format!("ex01_{}", flag.trim_start_matches('-')));
        assert!(Command::new(aodc_bin())
            .args(["build", flag, "-O2"])
            .arg(&ex01)
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap()
            .status
            .success());
        assert!(Command::new(aodc_bin())
            .args(["emit-ir", flag, "-O2"])
            .arg(&ex01)
            .output()
            .unwrap()
            .status
            .success());
        assert!(Command::new(aodc_bin())
            .args(["fmt", flag])
            .arg(&ex01)
            .output()
            .unwrap()
            .status
            .success());
        assert!(Command::new(aodc_bin())
            .args(["audit", flag, "--json"])
            .arg(&ex01)
            .output()
            .unwrap()
            .status
            .success());
    }

    // 3. Combining `--compat v1 --strict-wave` fails with exit 2, while `--compat v2 --strict-wave` is accepted idempotently (`AC-04`)!
    for sub in ["check", "run", "fmt", "audit", "audit-dec", "test"] {
        let out_v1 = Command::new(aodc_bin())
            .args([sub, "--compat", "v1", "--strict-wave"])
            .arg(&ex01)
            .output()
            .unwrap();
        assert_eq!(
            out_v1.status.code(),
            Some(2),
            "{sub} --compat v1 --strict-wave must exit 2, got {:?}: {}",
            out_v1.status.code(),
            String::from_utf8_lossy(&out_v1.stderr)
        );
        let out_v2 = Command::new(aodc_bin())
            .args([sub, "--compat", "v2", "--strict-wave"])
            .arg(&ex01)
            .output()
            .unwrap();
        assert!(
            out_v2.status.success(),
            "{sub} --compat v2 --strict-wave must exit 0, got {:?}: {}",
            out_v2.status.code(),
            String::from_utf8_lossy(&out_v2.stderr)
        );
    }
    for sub in ["build", "emit-ir"] {
        let out_v1 = Command::new(aodc_bin())
            .args([sub, "--compat", "v1", "--strict-wave"])
            .arg(&ex01)
            .output()
            .unwrap();
        assert_eq!(out_v1.status.code(), Some(2));
    }

    // 4. Default `CompatMode::V2` (without `--strict-wave`) unconditionally rejects `Dec`, `mut`, `while`, `str` (`AC-02`)!
    let legacy_v2 = "Dec main() -> int:\n    mut i = 0\n    let s: str = \"ok\"\n    while i < 2:\n        i = i + 1\n    assert s == \"ok\"\n    return i\n";
    assert!(check_source(legacy_v2, CompatMode::V2).is_err());

    // 5. `aodc audit --json` (both bare and `--strict-wave`) on banned construct emits valid `aod.dec.audit.v1` JSON (`status: "FAIL"`) and exits 1!
    let legacy_path = tmp.join("legacy_for_audit.dec");
    fs::write(&legacy_path, legacy_v2).unwrap();
    let aud_fail = Command::new(aodc_bin())
        .args(["audit", "--json"])
        .arg(&legacy_path)
        .output()
        .unwrap();
    assert_eq!(aud_fail.status.code(), Some(1));
    let parsed_json = parse_strict_json(&String::from_utf8_lossy(&aud_fail.stdout))
        .expect("audit --json must emit valid JSON on stdout even on failure");
    assert_eq!(
        parsed_json.get("schema").and_then(JsonValue::as_str),
        Some("aod.dec.audit.v1")
    );
    assert_eq!(
        parsed_json.get("status").and_then(JsonValue::as_str),
        Some("FAIL")
    );

    // 6. `audit-dec` constant folding over register casts `u8(...)`, `t9(...)`, `t32(...)`, `int(...)` (`AC-13`)!
    let reg_audit_ok = r#"
use aod::core:
    u8, t9, t32, verify_sheddic_route, verify_frg_cross_return

const XS: int = int(t9(3))
const ENC: u8 = u8(4)
const T0: int = int(t32(10))
const CX: int = int(t9(5))

dec main:
    () -> int:
        assert verify_sheddic_route(XS, ENC)
        assert verify_frg_cross_return(T0, T0 + CX, CX)
        return 0
"#;
    let rep_ok = audit_dec_source_with_options(
        reg_audit_ok,
        "reg_ok.dec",
        None,
        CompatMode::V2,
        strict_opts(),
    );
    assert!(rep_ok.passed() && rep_ok.hooks_passed() == 6, "{:?}", rep_ok.errors);

    let reg_audit_forged = r#"
use aod::core:
    t9, t32, verify_frg_cross_return

const T0: int = int(t32(10))
const CX: int = int(t9(4))

dec main:
    () -> int:
        assert verify_frg_cross_return(T0, 15, CX)
        return 0
"#;
    let rep_forged = audit_dec_source_with_options(
        reg_audit_forged,
        "reg_forged.dec",
        None,
        CompatMode::V2,
        strict_opts(),
    );
    assert!(!rep_forged.passed());

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn p1_1_fmt_roundtrip_idempotence_0h_0d_transitional_v1_and_symlink_guards() {
    use std::os::unix::fs::symlink;

    let root = repo_root();
    let tmp = temp_dir("p1_1_fmt");

    // (a) `0_H` and `0_D` expressions under `PureWaveV2`: round-trip, idempotent, and `check` passes
    let hinge_dwell_src = r#"
use aod::core:
    t1

dec check_trits:
    (a: t1, b: t1) -> int:
        assert a == 0_H and b == 0_D
        return int(a) + int(b)

dec main:
    () -> int:
        return check_trits(0_H, 0_D)
"#;
    let f1_hd = format_source_with_discipline(
        hinge_dwell_src,
        CompatMode::V2,
        SyntaxDiscipline::PureWaveV2,
    )
    .expect("fmt --strict-wave on 0_H / 0_D failed");
    assert!(f1_hd.contains("0_H") && f1_hd.contains("0_D"), "{f1_hd}");
    assert!(!f1_hd.contains("0H") && !f1_hd.contains("0D"), "{f1_hd}");
    let f2_hd =
        format_source_with_discipline(&f1_hd, CompatMode::V2, SyntaxDiscipline::PureWaveV2)
            .unwrap();
    assert_eq!(f1_hd, f2_hd, "0_H/0_D fmt must be idempotent");
    check_source_with_options(&f1_hd, CompatMode::V2, &strict_opts())
        .expect("formatted 0_H/0_D source must typecheck under --strict-wave");

    // (b) Explicit `(CompatMode::V2, SyntaxDiscipline::LegacyV1)` and `(CompatMode::V1, SyntaxDiscipline::PureWaveV2)` rejected with ParseError (`[P0-3]`)!
    assert_eq!(
        format_source_with_discipline(
            hinge_dwell_src,
            CompatMode::V2,
            SyntaxDiscipline::LegacyV1,
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ParseError
    );
    assert_eq!(
        format_source_with_discipline(
            "Dec main() -> out:\n    () -> int out:\n        out = 0\n",
            CompatMode::V1,
            SyntaxDiscipline::PureWaveV2,
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ParseError
    );

    // (c) All 10 `tests/fixtures/v1_examples/*.dec` under `--compat v1`: round-trip, idempotent, and `check`
    let v1_dir = root.join("tests/fixtures/v1_examples");
    let mut v1_count = 0usize;
    for ent in fs::read_dir(&v1_dir).unwrap() {
        let p = ent.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("dec") {
            continue;
        }
        v1_count += 1;
        let raw = fs::read_to_string(&p).unwrap();
        let f1 = format_source_with_discipline(
            &raw,
            CompatMode::V1,
            SyntaxDiscipline::LegacyV1,
        )
        .unwrap_or_else(|e| panic!("v1 fmt failed on {}: {e}", p.display()));
        let f2 = format_source_with_discipline(
            &f1,
            CompatMode::V1,
            SyntaxDiscipline::LegacyV1,
        )
        .unwrap_or_else(|e| panic!("v1 second fmt failed on {}: {e}", p.display()));
        assert_eq!(f1, f2, "v1 fmt idempotence failed on {}", p.display());
        check_source(&f1, CompatMode::V1)
            .unwrap_or_else(|e| panic!("formatted v1 {} failed check: {e}", p.display()));
    }
    assert_eq!(v1_count, 10);

    // (d) Symlink rejection on `aodc fmt --write` and `aodc fmt -o` (exit 2 without clobbering victim!)
    let victim = tmp.join("victim.dec");
    let sentinel = "# DO NOT CLOBBER VICTIM\ndec main:\n    () -> int:\n        return 99\n";
    fs::write(&victim, sentinel).unwrap();

    let unformatted = tmp.join("input_unfmt.dec");
    fs::write(
        &unformatted,
        "dec main:\n    () -> int:\n        return   1 + 2\n",
    )
    .unwrap();

    let sym_in_place = tmp.join("sym_write.dec");
    symlink(&victim, &sym_in_place).unwrap();
    let out_write = Command::new(aodc_bin())
        .args(["fmt", "--strict-wave", "--write"])
        .arg(&sym_in_place)
        .output()
        .unwrap();
    assert_eq!(
        out_write.status.code(),
        Some(2),
        "fmt --write on symlink must exit 2: {}",
        String::from_utf8_lossy(&out_write.stderr)
    );
    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        sentinel,
        "victim clobbered by fmt --write!"
    );

    let sym_out = tmp.join("sym_out.dec");
    symlink(&victim, &sym_out).unwrap();
    let out_dash_o = Command::new(aodc_bin())
        .args(["fmt", "--strict-wave", "-o"])
        .arg(&sym_out)
        .arg(&unformatted)
        .output()
        .unwrap();
    assert_eq!(
        out_dash_o.status.code(),
        Some(2),
        "fmt -o <symlink> must exit 2: {}",
        String::from_utf8_lossy(&out_dash_o.stderr)
    );
    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        sentinel,
        "victim clobbered by fmt -o!"
    );

    // Positive atomic `--write` and `--check` round-trip on regular file
    let ok_write = Command::new(aodc_bin())
        .args(["fmt", "--strict-wave", "--write"])
        .arg(&unformatted)
        .output()
        .unwrap();
    assert!(ok_write.status.success());
    let ok_check = Command::new(aodc_bin())
        .args(["fmt", "--strict-wave", "--check"])
        .arg(&unformatted)
        .output()
        .unwrap();
    assert!(ok_check.status.success());

    let _ = fs::remove_dir_all(&tmp);
}

