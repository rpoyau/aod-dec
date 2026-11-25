use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;
use aodc::diagnostics::{CompatMode, DiagnosticCode, SyntaxDiscipline};
use aodc::ir::{dumps_ir, loads_ir};
use aodc::eval::EvalOutput;
use aodc::{
    audit_dec_file_with_options, build_aot_file_with_options, check_source,
    check_source_with_options, compile_file_to_ir_with_options, emit_llvm_file_with_options,
    exec_ir_file, exec_ir_file_with_budget_capturing, exec_ir_str, jit_file_with_options_captured,
    run_file_with_options_capturing, LoadOptions, OptLevel,
};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aodc_pr11_d2_{tag}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn pure_opts() -> LoadOptions {
    LoadOptions {
        stdlib_dir: None,
        discipline: SyntaxDiscipline::PureWaveV2,
    }
}

fn assert_6mode_with_opts(
    src_path: &Path,
    expected: &str,
    opts: &LoadOptions,
    tmp: &Path,
    stem: &str,
) {
    // 1. Exact VM (`run`)
    let (emits, vm_val) =
        run_file_with_options_capturing(src_path, CompatMode::V2, 100_000, 128, opts)
            .unwrap_or_else(|e| panic!("{stem} check/init failed: {e}"));
    let vm_out = EvalOutput {
        emits,
        result: vm_val.unwrap_or_else(|e| panic!("{stem} VM failed: {e}")),
    }
    .to_stdout_string();
    assert_eq!(vm_out, expected, "{stem}: VM stdout mismatch");

    // 2. Hermetic DecIR v2 (`compile + exec`) + deterministic round-trip
    let ir_text = compile_file_to_ir_with_options(src_path, CompatMode::V2, opts)
        .unwrap_or_else(|e| panic!("{stem} compile failed: {e}"));
    if opts.discipline == SyntaxDiscipline::PureWaveV2 {
        assert!(
            ir_text.contains("\"syntax_discipline\": \"pure_wave_v2\""),
            "{stem}: PureWaveV2 IR must persist syntax_discipline"
        );
    }
    let ir_path = tmp.join(format!("{stem}.ir.json"));
    fs::write(&ir_path, &ir_text).unwrap();
    let reloaded = loads_ir(&ir_text).unwrap_or_else(|e| panic!("{stem} loads_ir failed: {e}"));
    assert_eq!(reloaded.unit.discipline, opts.discipline);
    assert_eq!(dumps_ir(&reloaded), ir_text);
    let ir_out = exec_ir_file(&ir_path)
        .unwrap_or_else(|e| panic!("{stem} exec_ir_file failed: {e}"))
        .to_stdout_string();
    assert_eq!(ir_out, expected, "{stem}: DecIR exec stdout mismatch");

    // 3 & 4. Standalone AOT ELF (`-O0` and `-O2`)
    for (tag, opt) in [("o0", OptLevel::O0), ("o2", OptLevel::O2)] {
        let exe = tmp.join(format!("{stem}_{tag}_exe"));
        build_aot_file_with_options(src_path, CompatMode::V2, opt, &exe, opts)
            .unwrap_or_else(|e| panic!("{stem} AOT {tag} build failed: {e}"));
        let aot = Command::new(&exe).output().unwrap();
        assert!(
            aot.status.success(),
            "{stem} AOT {tag} failed: {}",
            String::from_utf8_lossy(&aot.stderr)
        );
        assert_eq!(
            String::from_utf8(aot.stdout).unwrap(),
            expected,
            "{stem}: AOT {tag} stdout mismatch"
        );
    }

    // 5 & 6. In-process ORC LLJIT (`-O0` and `-O2`)
    for (tag, opt) in [("o0", OptLevel::O0), ("o2", OptLevel::O2)] {
        let (jit_out, res) =
            jit_file_with_options_captured(src_path, CompatMode::V2, opt, 100_000, 128, opts)
                .unwrap_or_else(|e| panic!("{stem} JIT {tag} compile failed: {e}"));
        assert!(res.is_ok(), "{stem} JIT {tag} trapped: {res:?}");
        assert_eq!(jit_out, expected, "{stem}: JIT {tag} stdout mismatch");
    }
}

fn assert_6mode_runtime_trap(src: &str, opts: &LoadOptions, fuel: usize, tmp: &Path, stem: &str) {
    let src_path = tmp.join(format!("{stem}.dec"));
    fs::write(&src_path, src).unwrap();

    // 1. VM
    let (_, vm_res) =
        run_file_with_options_capturing(&src_path, CompatMode::V2, fuel, 128, opts).unwrap();
    assert!(vm_res.is_err(), "{stem}: expected VM trap, got {vm_res:?}");

    // 2. DecIR exec
    let ir_text = compile_file_to_ir_with_options(&src_path, CompatMode::V2, opts).unwrap();
    let ir_path = tmp.join(format!("{stem}.ir.json"));
    fs::write(&ir_path, &ir_text).unwrap();
    let (_, ir_res) = exec_ir_file_with_budget_capturing(&ir_path, fuel, 128).unwrap();
    assert!(
        ir_res.is_err(),
        "{stem}: expected DecIR exec trap, got {ir_res:?}"
    );

    // 3. AOT -O2
    let exe = tmp.join(format!("{stem}_o2_exe"));
    build_aot_file_with_options(&src_path, CompatMode::V2, OptLevel::O2, &exe, opts).unwrap();
    let aot = Command::new(&exe)
        .arg("--fuel")
        .arg(fuel.to_string())
        .output()
        .unwrap();
    assert!(!aot.status.success(), "{stem}: expected AOT trap");

    // 4. JIT -O0 & -O2
    for opt in [OptLevel::O0, OptLevel::O2] {
        let (_, jit_res) =
            jit_file_with_options_captured(&src_path, CompatMode::V2, opt, fuel, 128, opts)
                .unwrap();
        assert!(jit_res.is_err(), "{stem}: expected JIT {opt:?} trap");
    }
}

#[test]
fn ac01_pure_dec_blocks_colon_params_where_guards_and_multi_arity() {
    let tmp = temp_dir("ac01");
    let pure = pure_opts();

    // Pure multi-arity + colon-typed + tuple-typed + `where` guard dispatch
    let src = r#"
enum Tag:
    Zero
    Val(int)

dec overload_mix:
    () -> int:
        return 10
    (x: int) where x > 100 -> int:
        return x - 100
    (0) -> int:
        return 1
    (x: int) -> int:
        return x * 2
    (b: bytes) -> int:
        return len(b)
    ((a, b): (int, int), scale: int) where a + b > 0 -> int:
        return (a + b) * scale
    (_: (int, int), _: int) -> int:
        return -9
    (Tag::Zero, _: t1) -> int:
        return 77
    (Tag::Val(k), s: t1) where int(s) < 0 -> int:
        return -k
    (Tag::Val(k), _: t1) -> int:
        return k

dec main:
    () -> int:
        emit overload_mix()
        emit overload_mix(125)
        emit overload_mix(0)
        emit overload_mix(6)
        emit overload_mix(b"wave")
        emit overload_mix((3, 4), 5)
        emit overload_mix((-5, 2), 5)
        emit overload_mix(Tag::Zero, 0t)
        emit overload_mix(Tag::Val(9), t1(-1))
        emit overload_mix(Tag::Val(9), 1t)
        return 0
"#;
    let src_path = tmp.join("multi_arity.dec");
    fs::write(&src_path, src).unwrap();
    let expected = "10\n25\n1\n12\n4\n35\n-9\n77\n-9\n9\n0\n";
    assert_6mode_with_opts(&src_path, expected, &pure, &tmp, "multi_arity");

    // Balanced delimiter tracking inside `where` guard under TransitionalV2 (`AC-01`, Section 3.2.1):
    // named argument `shift(x: n, delta: 2)` contains `:` inside `(...)` before `->`!
    let trans_src = r#"
Dec shift(x: int, delta: int) -> int:
    return x + delta

Dec guarded(n) -> out:
    (n: int) where shift(x: n, delta: 2) > 10 -> int:
        return n * 10
    (n: int) -> int:
        return n

dec main:
    () -> int:
        assert guarded(9) == 90
        assert guarded(3) == 3
        return 1
"#;
    let trans_path = tmp.join("where_colon_balance.dec");
    fs::write(&trans_path, trans_src).unwrap();
    assert_6mode_with_opts(
        &trans_path,
        "1\n",
        &LoadOptions::default(),
        &tmp,
        "where_colon_balance",
    );

    // Hostile negative controls
    // 1. Conflicting return types across arms with identical concrete parameter signature
    let bad_ret = r#"
dec bad:
    (0) -> int:
        return 0
    (x: int) -> bytes:
        return b"no"
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(bad_ret, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 2. Trailing `where` after `->` rejected
    let trailing_where = r#"
dec bad:
    (x: int) -> int where x > 0:
        return x
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(trailing_where, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::ParseError
    );

    // 3. Non-bool `where` guard rejected
    let int_where = r#"
dec bad:
    (x: int) where x + 1 -> int:
        return x
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(int_where, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 4. `--compat v1` rejects `dec main:`
    assert!(check_source("dec main:\n    () -> int:\n        return 0\n", CompatMode::V1).is_err());

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac02_ac03_indented_use_and_brace_free_map_literals() {
    let tmp = temp_dir("ac02_ac03");
    let pure = pure_opts();

    fs::write(
        tmp.join("helper_mod.dec"),
        r#"
struct PairBox:
    left: int
    right: int

dec sum_pair:
    (p: PairBox) -> int:
        return p.left + p.right

dec triple:
    (x: int) -> int:
        return x * 3
"#,
    )
    .unwrap();

    let entry = r#"
use aod::core:
    len
    contains
    u8

use helper_mod:
    PairBox, sum_pair
    triple

dec main:
    () -> int:
        let pb = PairBox(4, 9)
        assert sum_pair(pb) == 13
        assert triple(5) == 15

        let empty_m: map[u8, bytes] = map[]
        assert len(empty_m) == 0 and not contains(empty_m, u8(1))

        let m = map[(u8(3), b"three"), (u8(1), b"one"), (u8(2), b"two")]
        assert len(m) == 3 and contains(m, u8(2))
        assert m[u8(1)] == b"one"
        assert map[(10, 99), (20, 88)][10] == 99
        emit m
        return sum_pair(pb) + triple(5)
"#;
    let entry_path = tmp.join("entry.dec");
    fs::write(&entry_path, entry).unwrap();
    let expected = "{u8(1): b\"one\", u8(2): b\"two\", u8(3): b\"three\"}\n28\n";
    assert_6mode_with_opts(&entry_path, expected, &pure, &tmp, "indented_use_and_map");

    // Hostile negative controls:
    // 1. Empty `use aod::core:` block
    let empty_use = "use aod::core:\n\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(empty_use, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::ParseError
    );

    // 2. Unannotated empty `map[]`
    let unannot_map = "dec main:\n    () -> int:\n        let m = map[]\n        return 0\n";
    assert_eq!(
        check_source_with_options(unannot_map, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 3. Malformed `map[1, 2]` without `(k, v)` tuple pairs
    let bad_pair_map = "dec main:\n    () -> int:\n        let m = map[1, 2]\n        return 0\n";
    assert_eq!(
        check_source_with_options(bad_pair_map, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::ParseError
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_ac05_evolve_scan_fold_seq_map_seq_sum_dot_and_unboxed_ir() {
    let tmp = temp_dir("ac04_ac05");
    let pure = pure_opts();

    let src = r#"
use aod::core:
    t9, t32, evolve, scan, fold_seq, map_seq, sum, dot, pow
use aod::qext:
    ext_decimal, qext_add, qext_to_bytes_exact

quarantine Q_ext QCalc:
    dec q_step:
        (acc: QExt<ratio>, _: int) -> QExt<ratio>:
            return qext_add(acc, ext_decimal(b"0.25"))

    dec run_q_evolve:
        (n: int) -> QExt<bytes>:
            let total = evolve(ext_decimal(b"1.00"), 0..n, q_step)
            return qext_to_bytes_exact(total)

dec fib_step:
    ((a, b): (int, int), _: int) -> (int, int):
        return (b, a + b)

dec add_int:
    (s: int, x: int) -> int:
        return s + x

dec bump_t9:
    (x: t9) -> t9:
        return x + t9(1)

dec main:
    () -> int:
        let f0 = evolve((0, 1), 0..0, fib_step)
        let f10 = evolve((0, 1), 0..10, fib_step)
        assert f0 == (0, 1) and f10 == (55, 89)

        let arr: array[int, 4] = [1, 3, 5, 7]
        let sc = scan(0, arr, add_int)
        assert sc == [1, 4, 9, 16]
        assert fold_seq(0, arr, add_int) == 16

        let t_arr: array[t9, 3] = [t9(-1), t9(0), t9(4)]
        let bumped: [t9] = map_seq(t_arr, bump_t9)
        assert sum(bumped) == t9(6)
        assert dot([t9(-1), t9(0), t9(4)], bumped) == t9(20)

        let empty_t9: [t9] = []
        let empty_t32: [t32] = []
        let empty_r: [ratio] = []
        assert (sum(empty_t9), sum(empty_t32), sum(empty_r)) == (t9(0), t32(0), 0/1r)
        assert dot(empty_t9, empty_t9) == t9(0)

        # BigInt spill (> i64::MAX) in sum and dot
        let big = pow(2, 80)
        assert sum([big, big]) == pow(2, 81)
        assert dot([big, 3], [2, 5]) == big * 2 + 15

        quarantine Q_ext:
            emit QCalc::run_q_evolve(6)

        assert fold_seq(0, sc, add_int) == 30
        return 89
"#;
    let src_path = tmp.join("scope_ops.dec");
    fs::write(&src_path, src).unwrap();
    assert_6mode_with_opts(
        &src_path,
        "Q_ext(b\"5/2\")\n89\n",
        &pure,
        &tmp,
        "scope_ops",
    );

    // P1-4: Verify `evolve` at `-O2` lowers to an unboxed loop with ZERO `@aod_rt_val_from_*` or `@aod_rt_list_*`!
    let ev_only = r#"
use aod::core:
    evolve

dec step_int:
    (s: int, t: int) -> int:
        return s + t

dec main:
    () -> int:
        return evolve(0, 0..1000, step_int)
"#;
    let ev_path = tmp.join("ev_unboxed.dec");
    fs::write(&ev_path, ev_only).unwrap();
    let ll = emit_llvm_file_with_options(&ev_path, CompatMode::V2, OptLevel::O2, &pure).unwrap();
    let has_arena_box_call = ll.lines().any(|line| {
        let t = line.trim();
        t.contains("call ")
            && (t.contains("@aod_rt_val_from_") || t.contains("@aod_rt_list_"))
    });
    assert!(
        !has_arena_box_call,
        "evolve hot loop must stay 100% unboxed without arena boxing:\n{ll}"
    );

    // Hostile compile-time controls:
    // 1. `dot` on mismatched static `array[int, 2]` vs `array[int, 3]`
    let bad_dot_arr = r#"
use aod::core:
    dot
dec main:
    () -> int:
        let a: array[int, 2] = [1, 2]
        let b: array[int, 3] = [1, 2, 3]
        return dot(a, b)
"#;
    assert_eq!(
        check_source_with_options(bad_dot_arr, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 2. Passing QExt step function to `evolve` outside `quarantine Q_ext`
    let leak_evolve = r#"
use aod::core:
    evolve
quarantine Q_ext QMod:
    dec qstep:
        (s: int, t: int) -> int:
            return s + t
dec main:
    () -> int:
        return evolve(0, 0..5, QMod::qstep)
"#;
    assert_eq!(
        check_source_with_options(leak_evolve, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 3a. PureWaveV2: passing `ReadOnly<[int]>` into non-ReadOnly `evolve` step function -> ObserverMutationForbidden!
    let ro_strip_evolve = r#"
use aod::core:
    evolve
dec non_ro_step:
    (xs: [int], _: int) -> ReadOnly<[int]>:
        return xs
dec caller:
    (ro_xs: ReadOnly<[int]>) -> ReadOnly<[int]>:
        return evolve(ro_xs, 0..2, non_ro_step)
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(ro_strip_evolve, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 3b. TransitionalV2: `observe` probe calling `evolve(view.carriers, ...)` whose step function mutates its list parameter -> ObserverMutationForbidden!
    let obs_mut_evolve = r#"
use aod::core:
    evolve
Dec mutating_step(xs, t) -> out:
    ([int] xs, int t) -> [int]:
        xs[0] = t
        return xs
observe BadObs:
    tracer tr(view):
        let res = evolve(view.carriers, 0..2, mutating_step)
        emit len(res)
Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(obs_mut_evolve, CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // Hostile 6-mode runtime traps:
    assert_6mode_runtime_trap(
        "use aod::core:\n    evolve\ndec st:\n    (s: int, t: int) -> int:\n        return s + t\ndec mk_w:\n    (x: int) -> window:\n        return (x, x]\ndec main:\n    () -> int:\n        return evolve(0, mk_w(5), st)\n",
        &pure,
        10_000,
        &tmp,
        "bad_win_evolve",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    evolve\ndec st:\n    (s: int, t: int) -> int:\n        return s + t\ndec main:\n    () -> int:\n        return evolve(0, 0..10000, st)\n",
        &pure,
        40,
        &tmp,
        "fuel_evolve",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    t9, sum\ndec main:\n    () -> t9:\n        return sum([t9(9841), t9(1)])\n",
        &pure,
        10_000,
        &tmp,
        "sum_t9_ovf",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    t9, dot\ndec main:\n    () -> t9:\n        return dot([t9(200)], [t9(200)])\n",
        &pure,
        10_000,
        &tmp,
        "dot_t9_ovf",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    dot\ndec main:\n    () -> int:\n        return dot([1, 2], [1, 2, 3])\n",
        &pure,
        10_000,
        &tmp,
        "dot_len_mismatch",
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac06_ac07_bytes_overloads_and_qext_bytes_quarantine() {
    let tmp = temp_dir("ac06_ac07");
    let pure = pure_opts();

    let src = r#"
use aod::core:
    select
use aod::qext:
    ext_decimal, qext_div, qext_from_ratio, qext_to_bytes_exact, qext_format_bytes_fixed

dec main:
    () -> int:
        quarantine Q_ext:
            let q1 = qext_div(ext_decimal(b"1.0"), ext_decimal(b"8.0"))
            let q2 = qext_from_ratio(1/8r)
            let eq_b = qext_to_bytes_exact(q1) == qext_to_bytes_exact(q2)
            let chosen = select(eq_b, qext_format_bytes_fixed(q1, 4), qext_to_bytes_exact(q1))
            emit qext_to_bytes_exact(q1)
            emit chosen
        return 1
"#;
    let src_path = tmp.join("qext_bytes.dec");
    fs::write(&src_path, src).unwrap();
    assert_6mode_with_opts(
        &src_path,
        "Q_ext(b\"1/8\")\nQ_ext(b\"0.1250\")\n1\n",
        &pure,
        &tmp,
        "qext_bytes",
    );

    // Hostile QExt<bytes> leakage controls (`P1-5`):
    // 1. Returning `QExt<bytes>` from native function
    let leak_ret = r#"
use aod::qext:
    ext_decimal, qext_to_bytes_exact
dec leak:
    () -> QExt<bytes>:
        return qext_to_bytes_exact(ext_decimal(b"1.5"))
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(leak_ret, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 2. Passing `QExt<bytes>` to native `sha256_bytes` inside `quarantine Q_ext:`
    let leak_sha = r#"
use aod::core:
    sha256_bytes
use aod::qext:
    ext_decimal, qext_to_bytes_exact
dec main:
    () -> int:
        quarantine Q_ext:
            let qb = qext_to_bytes_exact(ext_decimal(b"1.5"))
            let _ = sha256_bytes(qb)
        return 0
"#;
    assert_eq!(
        check_source_with_options(leak_sha, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 3. Calling `qext_to_bytes_exact` outside `quarantine Q_ext`
    let outside_qext = r#"
use aod::qext:
    qext_to_bytes_exact
dec main:
    () -> int:
        let _ = qext_to_bytes_exact(1)
        return 0
"#;
    assert_eq!(
        check_source_with_options(outside_qext, CompatMode::V2, &pure)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 4. Invalid UTF-8 byte sequences trap cleanly across all 6 modes!
    assert_6mode_runtime_trap(
        "use aod::core:\n    addr8\ndec main:\n    () -> addr8:\n        return addr8(b\"\\xff\\xfe\")\n",
        &pure,
        10_000,
        &tmp,
        "bad_utf8_addr8",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    support\ndec main:\n    () -> support:\n        return support(b\"\\xff\")\n",
        &pure,
        10_000,
        &tmp,
        "bad_utf8_support",
    );
    assert_6mode_runtime_trap(
        "use aod::qext:\n    ext_decimal, qext_to_bytes_exact\ndec main:\n    () -> int:\n        quarantine Q_ext:\n            emit qext_to_bytes_exact(ext_decimal(b\"\\x80\"))\n        return 0\n",
        &pure,
        10_000,
        &tmp,
        "bad_utf8_qext",
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_exact_scalar_and_rational_intrinsics_and_pow_bit_ceiling() {
    let tmp = temp_dir("ac08");
    let pure = pure_opts();

    // Includes BigInt spill (`i64::MIN`, `2^100`) on `num`, `den`, `abs`, `signum`, `gcd`, `lcm`, `pow`!
    let src = r#"
use aod::core:
    t1, t9, t32, u8, b64, ratio
    num, den, abs, signum, min, max, clamp, gcd, lcm, pow

dec main:
    () -> int:
        let i64_min = -pow(2, 63)
        assert abs(i64_min) == pow(2, 63)
        assert signum(i64_min) == t1(-1)
        assert signum(pow(2, 100)) == 1t

        let big_r = ratio(-pow(3, 50), pow(2, 80))
        assert num(big_r) == -pow(3, 50)
        assert den(big_r) == pow(2, 80)
        assert abs(big_r) == ratio(pow(3, 50), pow(2, 80))
        assert signum(big_r) == t1(-1)

        assert gcd(0, 0) == 0 and lcm(0, 99) == 0
        assert gcd(pow(2, 70) * 15, pow(2, 70) * 21) == pow(2, 70) * 3
        assert min(t9(-5), t9(2)) == t9(-5) and max(t32(-9), t32(-2)) == t32(-2)
        assert clamp(u8(200), u8(10), u8(100)) == u8(100)
        assert clamp(b64(1), b64(5), b64(9)) == b64(5)
        assert clamp(t1(-1), 0t, 1t) == 0t
        return 1
"#;
    let src_path = tmp.join("intrinsics_spill.dec");
    fs::write(&src_path, src).unwrap();
    assert_6mode_with_opts(&src_path, "1\n", &pure, &tmp, "intrinsics_spill");

    // Hostile runtime traps across all 6 modes:
    // 1. `clamp(x, lo, hi)` with `lo > hi`
    assert_6mode_runtime_trap(
        "use aod::core:\n    clamp\ndec main:\n    () -> int:\n        return clamp(5, 10, 2)\n",
        &pure,
        10_000,
        &tmp,
        "clamp_bad_bounds",
    );
    // 2. `pow(int, negative)`
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> int:\n        return pow(2, -1)\n",
        &pure,
        10_000,
        &tmp,
        "pow_neg_int",
    );
    // 3. `pow(0/1r, -1)`
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> ratio:\n        return pow(0/1r, -1)\n",
        &pure,
        10_000,
        &tmp,
        "pow_zero_ratio_neg",
    );
    // 4. `pow(2, 65537)`
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> int:\n        return pow(2, 65537)\n",
        &pure,
        10_000,
        &tmp,
        "pow_exp_cap",
    );
    // 5. P1-6 OOM guard: chained `pow(pow(10, 65536), 65536)` MUST trap immediately (zero OOM!)
    let t0 = Instant::now();
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> int:\n        return pow(pow(10, 65536), 65536)\n",
        &pure,
        10_000,
        &tmp,
        "pow_chained_oom_guard",
    );
    assert!(
        t0.elapsed().as_secs() < 10,
        "chained pow OOM guard took too long: {:?}",
        t0.elapsed()
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_ac10_pure_wave_rejections_and_hermetic_ir_tamper_defense() {
    let tmp = temp_dir("ac09_ac10");
    let pure = pure_opts();

    let banned_cases: &[(&str, &str)] = &[
        ("brace_struct_decl", "struct S { x: int }\ndec main:\n    () -> int:\n        return 0\n"),
        ("brace_enum_decl", "enum E { A, B }\ndec main:\n    () -> int:\n        return 0\n"),
        ("brace_struct_lit", "struct S:\n    x: int\ndec main:\n    () -> int:\n        let s = S { x: 1 }\n        return s.x\n"),
        ("brace_map_lit", "dec main:\n    () -> int:\n        let m = {1: 2}\n        return 0\n"),
        ("brace_use", "use aod::core::{t1}\ndec main:\n    () -> int:\n        return 0\n"),
        ("uppercase_dec", "Dec main:\n    () -> int:\n        return 0\n"),
        ("single_arm_header", "dec main() -> int:\n    return 0\n"),
        ("prefix_arm_param", "dec f:\n    (int x) -> int:\n        return x\ndec main:\n    () -> int:\n        return f(1)\n"),
        ("legacy_if_guard", "dec f:\n    (x: int) if x > 0 -> int:\n        return x\n    (x: int) -> int:\n        return 0\ndec main:\n    () -> int:\n        return f(1)\n"),
        ("legacy_arm_out_binding", "dec main:\n    () -> int out:\n        return 0\n"),
        ("if_stmt", "dec main:\n    () -> int:\n        if true:\n            return 1\n        return 0\n"),
        ("ternary_expr", "dec main:\n    () -> int:\n        return 1 if true else 0\n"),
        ("for_loop", "dec main:\n    () -> int:\n        for i in 0..3:\n            pass\n        return 0\n"),
        ("while_loop", "dec main:\n    () -> int:\n        while false:\n            pass\n        return 0\n"),
        ("break_stmt", "dec main:\n    () -> int:\n        break\n        return 0\n"),
        ("continue_stmt", "dec main:\n    () -> int:\n        continue\n        return 0\n"),
        ("mut_local", "dec main:\n    () -> int:\n        mut x = 1\n        return x\n"),
        ("local_rebinding_p1_3", "dec main:\n    () -> int:\n        let x = 1\n        x = 2\n        return x\n"),
        ("implicit_assign", "dec main:\n    () -> int:\n        x = 1\n        return x\n"),
        ("field_mutation", "struct S:\n    x: int\ndec main:\n    () -> int:\n        let s = S(1)\n        s.x = 2\n        return s.x\n"),
        ("index_mutation", "dec main:\n    () -> int:\n        let xs = [1, 2]\n        xs[0] = 9\n        return xs[0]\n"),
        ("typed_local_v1", "dec main:\n    () -> int:\n        int x = 1\n        return x\n"),
        ("match_stmt", "enum E:\n    A\ndec main:\n    () -> int:\n        match E::A:\n            E::A:\n                return 1\n"),
        ("named_call_arg", "struct S:\n    x: int\ndec main:\n    () -> int:\n        let s = S(x: 1)\n        return s.x\n"),
        ("double_quote_str", "dec main:\n    () -> int:\n        let s = \"hello\"\n        return 0\n"),
        ("single_quote_str", "dec main:\n    () -> int:\n        let s = 'hello'\n        return 0\n"),
        ("str_type_annot", "dec main:\n    () -> int:\n        let xs: [str] = []\n        return len(xs)\n"),
        ("str_expr_boundary_box", "dec main:\n    () -> int:\n        let b = box([0..2, 0..2, 0..2])\n        emit ∂b\n        return 0\n"),
        ("qext_render_str_in_pure", "use aod::qext:\n    ext_decimal, qext_render\ndec main:\n    () -> int:\n        quarantine Q_ext:\n            emit qext_render(ext_decimal(b\"1.5\"))\n        return 0\n"),
    ];

    for (label, code) in banned_cases {
        let res = check_source_with_options(code, CompatMode::V2, &pure);
        assert!(
            res.is_err(),
            "PureWaveV2 MUST reject banned case {label:?}, but check succeeded!"
        );
    }

    // Transitive module check: a `--pure-wave` entry file importing a module with legacy `if` MUST fail!
    fs::write(
        tmp.join("dirty_dep.dec"),
        "Dec helper(x: int) -> int:\n    if x > 0:\n        return x\n    return 0\n",
    )
    .unwrap();
    let imp_entry = tmp.join("pure_entry.dec");
    fs::write(
        &imp_entry,
        "use dirty_dep:\n    helper\ndec main:\n    () -> int:\n        return helper(3)\n",
    )
    .unwrap();
    assert!(aodc::check_file_with_options(&imp_entry, CompatMode::V2, &pure).is_err());

    // P0-5 Hermetic DecIR v2 `syntax_discipline: "pure_wave_v2"` tamper defense!
    let valid_pure = "dec main:\n    () -> int:\n        return 7\n";
    let valid_pure_path = tmp.join("valid_pure.dec");
    fs::write(&valid_pure_path, valid_pure).unwrap();
    let ir_json_str =
        compile_file_to_ir_with_options(&valid_pure_path, CompatMode::V2, &pure).unwrap();
    assert_eq!(
        exec_ir_str(&ir_json_str).unwrap().to_stdout_string(),
        "7\n"
    );

    // Tamper A: smuggle legacy `Dec` + `mut` + `"str"` source into a `"pure_wave_v2"` IR with valid SHA-256!
    let smuggled_src = "Dec main() -> int:\n    mut x = 7\n    let s = \"smuggled\"\n    return x\n";
    let mut obj = match parse_strict_json(&ir_json_str).unwrap() {
        JsonValue::Object(m) => m,
        _ => unreachable!(),
    };
    obj.insert(
        "source".to_string(),
        JsonValue::from_str_val(smuggled_src.to_string()),
    );
    obj.insert(
        "source_sha256".to_string(),
        JsonValue::from_str_val(Sha256::digest_hex(smuggled_src.as_bytes())),
    );
    let tampered_smuggled = JsonValue::Object(obj.clone()).to_pretty_utf8_json();
    assert_eq!(
        loads_ir(&tampered_smuggled).unwrap_err().code,
        DiagnosticCode::IrVerificationError
    );

    // Tamper B: unknown `syntax_discipline` value rejected
    let mut obj_bad_disc = match parse_strict_json(&ir_json_str).unwrap() {
        JsonValue::Object(m) => m,
        _ => unreachable!(),
    };
    obj_bad_disc.insert(
        "syntax_discipline".to_string(),
        JsonValue::from_str_val("bogus_mode".to_string()),
    );
    assert_eq!(
        loads_ir(&JsonValue::Object(obj_bad_disc).to_pretty_utf8_json())
            .unwrap_err()
            .code,
        DiagnosticCode::IrVerificationError
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac11_ac12_showcase_6mode_cli_pure_wave_and_zero_regressions() {
    let tmp = temp_dir("ac11_ac12");
    let root = repo_root();
    let showcase = root.join("tests/fixtures/v2_smoke/pure_wave_signature_matrix.dec");
    let expected =
        fs::read_to_string(root.join("tests/fixtures/v2_smoke/pure_wave_signature_matrix.stdout"))
            .unwrap();

    // 1. Static textual scan of `pure_wave_signature_matrix.dec`: ZERO `{`, ZERO `}`!
    let showcase_src = fs::read_to_string(&showcase).unwrap();
    assert!(
        !showcase_src.contains('{') && !showcase_src.contains('}'),
        "pure_wave_signature_matrix.dec must contain zero braces"
    );

    // 2. Full 6-mode parity under `PureWaveV2`
    let pure = pure_opts();
    assert_6mode_with_opts(&showcase, &expected, &pure, &tmp, "showcase_pure");

    // 3. `audit_dec_file_with_options` passes all 6 hooks under `PureWaveV2`
    let audit = audit_dec_file_with_options(&showcase, CompatMode::V2, &pure);
    assert!(audit.passed(), "showcase audit-dec failed: {:?}", audit.errors);
    assert_eq!(audit.hooks_passed(), 6);

    // 4. CLI `--pure-wave` end-to-end across all 10 supported subcommands!
    // (a) `check --pure-wave`
    let out_check = Command::new(aodc_bin())
        .args(["check", "--pure-wave"])
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_check.status.success());
    assert!(String::from_utf8_lossy(&out_check.stdout).contains("OK: Dec v2 (pure-wave)"));

    // (b) `run --pure-wave`
    let out_run = Command::new(aodc_bin())
        .args(["run", "--pure-wave"])
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_run.status.success());
    assert_eq!(String::from_utf8_lossy(&out_run.stdout), expected);

    // (c) `compile --pure-wave` + `exec`
    let ir_out = tmp.join("cli_showcase.ir.json");
    let out_compile = Command::new(aodc_bin())
        .args(["compile", "--pure-wave"])
        .arg(&showcase)
        .arg("-o")
        .arg(&ir_out)
        .output()
        .unwrap();
    assert!(out_compile.status.success());
    let out_exec = Command::new(aodc_bin())
        .arg("exec")
        .arg(&ir_out)
        .output()
        .unwrap();
    assert!(out_exec.status.success());
    assert_eq!(String::from_utf8_lossy(&out_exec.stdout), expected);

    // (d) `emit-llvm --pure-wave`
    let ll_out = tmp.join("cli_showcase.ll");
    let out_ll = Command::new(aodc_bin())
        .args(["emit-llvm", "--pure-wave", "-O2"])
        .arg(&showcase)
        .arg("-o")
        .arg(&ll_out)
        .output()
        .unwrap();
    assert!(out_ll.status.success());
    assert!(ll_out.is_file());

    // (e) `build --pure-wave`
    let exe_out = tmp.join("cli_showcase_exe");
    let out_build = Command::new(aodc_bin())
        .args(["build", "--pure-wave", "-O2"])
        .arg(&showcase)
        .arg("-o")
        .arg(&exe_out)
        .output()
        .unwrap();
    assert!(out_build.status.success());
    let exe_run = Command::new(&exe_out).output().unwrap();
    assert!(exe_run.status.success());
    assert_eq!(String::from_utf8_lossy(&exe_run.stdout), expected);

    // (f) `jit --pure-wave`
    let out_jit = Command::new(aodc_bin())
        .args(["jit", "--pure-wave", "-O2"])
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_jit.status.success());
    assert_eq!(String::from_utf8_lossy(&out_jit.stdout), expected);

    // (g) `bench --pure-wave --iters 1`
    let out_bench = Command::new(aodc_bin())
        .args(["bench", "--pure-wave", "--iters", "1"])
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_bench.status.success());
    assert!(String::from_utf8_lossy(&out_bench.stdout).contains("5/5 byte-identical"));

    // (h) `learn --pure-wave --pairs 2 --heldout 2`
    let prof_out = tmp.join("cli_profile.json");
    let out_learn = Command::new(aodc_bin())
        .args([
            "learn",
            "--pure-wave",
            "--pairs",
            "2",
            "--heldout",
            "2",
            "--profile",
        ])
        .arg(&prof_out)
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(
        out_learn.status.success(),
        "learn --pure-wave failed: {}",
        String::from_utf8_lossy(&out_learn.stderr)
    );

    // (i) `audit-dec --pure-wave`
    let out_audit = Command::new(aodc_bin())
        .args(["audit-dec", "--pure-wave"])
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_audit.status.success());
    assert!(String::from_utf8_lossy(&out_audit.stdout).contains("PASS (6/6 hooks"));

    // (j) `test --pure-wave`
    let out_test = Command::new(aodc_bin())
        .args(["test", "--pure-wave"])
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_test.status.success());
    assert!(String::from_utf8_lossy(&out_test.stdout).contains("1 passed; 0 failed; 1 total"));

    // (k) `--compat v1 --strict-wave` / `--pure-wave` rejected with exit code 2 across CLI subcommands
    for sub in ["check", "run", "audit-dec", "test"] {
        for flag in ["--strict-wave", "--pure-wave"] {
            let bad = Command::new(aodc_bin())
                .args([sub, "--compat", "v1", flag])
                .arg(&showcase)
                .output()
                .unwrap();
            assert_eq!(
                bad.status.code(),
                Some(2),
                "{sub} --compat v1 {flag} must exit 2"
            );
        }
    }

    // (l) Legacy file rejected by `aodc check --pure-wave`
    let legacy_smoke = root.join("tests/fixtures/v2_smoke/opposing_336_chamber.dec");
    let bad_legacy = Command::new(aodc_bin())
        .args(["check", "--pure-wave"])
        .arg(&legacy_smoke)
        .output()
        .unwrap();
    assert!(!bad_legacy.status.success());

    let _ = fs::remove_dir_all(&tmp);
}
