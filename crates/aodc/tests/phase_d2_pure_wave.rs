use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;
use aodc::diagnostics::DiagnosticCode;
use aodc::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::parser::parse_source;
use aodc::{
    audit_dec_file_with_options, build_aot_file_with_options, check_file_with_options,
    check_source, check_source_with_options, compile_file_to_ir_with_options,
    emit_llvm_file_with_options, exec_ir_str, format_program, format_source,
    jit_file_with_profile_captured, LoadOptions, OptLevel,
};

static D2_SEQ: AtomicU64 = AtomicU64::new(0);

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
    let n = D2_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aodc_pr11_d2_{tag}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn assert_6mode_with_opts(
    path: &Path,
    expected_stdout: &str,
    opts: &LoadOptions,
    tmp: &Path,
    stem: &str,
) {
    // 1. Exact VM (`run`)
    let checked_vm = check_file_with_options(path, opts)
        .unwrap_or_else(|e| panic!("{stem} check failed: {e}"));
    let vm_out = Evaluator::new(&checked_vm, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap_or_else(|e| panic!("{stem} VM failed: {e}"))
        .to_stdout_string();
    assert_eq!(vm_out, expected_stdout, "{stem}: VM stdout mismatch");

    // 2. Hermetic DecIR v3 (`compile + exec`)
    let ir_json = compile_file_to_ir_with_options(path, opts)
        .unwrap_or_else(|e| panic!("{stem} IR compile failed: {e}"));
    assert!(ir_json.contains("\"schema\": \"aod.dec.ir.v3\""));
    assert!(ir_json.contains("\"syntax_discipline\": \"pure_wave_v2\""));
    let ir_out = exec_ir_str(&ir_json)
        .unwrap_or_else(|e| panic!("{stem} IR exec failed: {e}"))
        .to_stdout_string();
    assert_eq!(ir_out, expected_stdout, "{stem}: DecIR v3 stdout mismatch");

    // 3 & 4. AOT -O0 and AOT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_aot_{tag}"));
        build_aot_file_with_options(path, opt, &exe, opts)
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
            opts,
        )
        .unwrap_or_else(|e| panic!("{stem} JIT -{tag} compile failed: {e}"));
        res.unwrap_or_else(|e| panic!("{stem} JIT -{tag} runtime failed: {e}"));
        assert_eq!(stdout, expected_stdout, "{stem}: JIT -{tag} stdout mismatch");
    }
}

fn assert_6mode_runtime_trap(
    src: &str,
    opts: &LoadOptions,
    fuel: usize,
    tmp: &Path,
    stem: &str,
) {
    let path = tmp.join(format!("{stem}.dec"));
    fs::write(&path, src).unwrap();

    let checked_vm = check_file_with_options(&path, opts).unwrap();
    assert!(
        Evaluator::new(&checked_vm, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .run_main()
            .is_err(),
        "{stem}: expected VM trap"
    );
    let ir_json = compile_file_to_ir_with_options(&path, opts).unwrap();
    let checked_ir = loads_ir(&ir_json).unwrap();
    assert!(
        Evaluator::new(&checked_ir, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .run_main()
            .is_err(),
        "{stem}: expected DecIR v3 trap"
    );

    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_trap_{tag}"));
        build_aot_file_with_options(&path, opt, &exe, opts).unwrap();
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
            opts,
        )
        .unwrap();
        assert!(res.is_err(), "{stem}: expected JIT -{tag} trap");
    }
}

#[test]
fn ac01_signature_matrix_dispatch_guards_and_typed_procedural_observe() {
    let tmp = temp_dir("ac01_sig_matrix");
    let opts = LoadOptions::default();

    let src = r#"
use aod::core:
    t1

struct Packet:
    code: int
    role: t1

enum Mode:
    Idle
    Burst(int)
    Pair(int, t1)

dec eval_mode:
    (Mode::Idle) -> int:
        return 0
    (Mode::Burst(n)) where n > 10 -> int:
        return n * 2
    (Mode::Burst(n)) -> int:
        return n + 1
    (Mode::Pair(k, 1t)) -> int:
        return k + 100
    (Mode::Pair(k, _)) -> int:
        return k - 100

dec scale:
    (x: int) -> int:
        return x * 2
    (x: int, k: int) -> int:
        return x * k

chamber TinyLab:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare Single in TinyLab:
    inject 3:3:6 at (4, 8, 8) dir +X orient 1t shells 1

observe TypedObs:
    tracer tr:
        (view: ReadOnly<ChamberView>) -> null:
            assert view.carrier_count == 1
            return null
    sensor sn:
        (view: ReadOnly<ChamberView>) -> int:
            return view.tick + view.carrier_count

dec main:
    () -> int:
        let pkt = Packet(7, 1t)
        assert pkt.code == 7 and pkt.role == 1t
        assert eval_mode(Mode::Idle) == 0
        assert eval_mode(Mode::Burst(15)) == 30
        assert eval_mode(Mode::Burst(4)) == 5
        assert eval_mode(Mode::Pair(9, 1t)) == 109
        assert eval_mode(Mode::Pair(9, -1t)) == -91
        assert scale(6) + scale(6, 5) == 42
        let sim = simulate Single for 2 ticks with TypedObs
        emit sim.ticks
        return 42
"#;
    let src_path = tmp.join("sig_matrix.dec");
    fs::write(&src_path, src).unwrap();
    assert_6mode_with_opts(&src_path, "2\n42\n", &opts, &tmp, "sig_matrix");

    // Hostile: non-bool `where` guard rejected with TypeError
    let int_where = r#"
dec bad:
    (x: int) where x + 1 -> int:
        return x
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(int_where, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // Hostile: non-exhaustive multi-arm enum signature matrix rejected
    let non_exh = r#"
enum Two:
    A
    B

dec bad_arm:
    (Two::A) -> int:
        return 1

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(non_exh, &opts).unwrap_err().code,
        DiagnosticCode::TypeError
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac02_ac03_indented_use_and_brace_free_map_literals() {
    let tmp = temp_dir("ac02_ac03");
    let opts = LoadOptions::default();

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
    b8
    map_remove
    map_get_or

use helper_mod:
    PairBox, sum_pair
    triple

dec main:
    () -> int:
        let pb = PairBox(4, 9)
        assert sum_pair(pb) == 13
        assert triple(5) == 15

        let empty_m: map[b8, bytes] = map[]
        assert len(empty_m) == 0 and not contains(empty_m, b8(1))

        let m0 = map[(b8(3), b"three"), (b8(1), b"one"), (b8(2), b"two"), (b8(9), b"drop")]
        let m = map_remove(m0, b8(9))
        assert len(m0) == 4 and len(m) == 3 and contains(m, b8(2))
        assert map_get_or(m, b8(1), b"missing") == b"one"
        assert map_get_or(m, b8(9), b"missing") == b"missing"
        assert m[b8(1)] == b"one"
        assert map[(10, 99), (20, 88)][10] == 99
        emit m
        return sum_pair(pb) + triple(5)
"#;
    let entry_path = tmp.join("entry.dec");
    fs::write(&entry_path, entry).unwrap();
    let expected = "{b8(0x01): b\"one\", b8(0x02): b\"two\", b8(0x03): b\"three\"}\n28\n";
    assert_6mode_with_opts(&entry_path, expected, &opts, &tmp, "indented_use_and_map");

    // Hostile negative controls:
    // 1. Empty `use aod::core:` block
    let empty_use = "use aod::core:\n\ndec main:\n    () -> int:\n        return 0\n";
    assert_eq!(
        check_source_with_options(empty_use, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::ParseError
    );

    // 2. Unannotated empty `map[]`
    let unannot_map = "dec main:\n    () -> int:\n        let _m = map[]\n        return 0\n";
    assert_eq!(
        check_source_with_options(unannot_map, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );

    // 3. Malformed `map[1, 2]` without `(k, v)` or `k: v` pairs
    let bad_pair_map = "dec main:\n    () -> int:\n        let _m = map[1, 2]\n        return 0\n";
    assert_eq!(
        check_source_with_options(bad_pair_map, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::ParseError
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_ac05_evolve_scan_fold_seq_map_seq_sum_dot_and_unboxed_ir() {
    let tmp = temp_dir("ac04_ac05");
    let opts = LoadOptions::default();

    let src = r#"
use aod::core:
    evolve, scan, fold_seq, map_seq, sum, dot, pow
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

dec bump_int:
    (x: int) -> int:
        return x + 1

dec main:
    () -> int:
        let f0 = evolve((0, 1), 0..0, fib_step)
        let f10 = evolve((0, 1), 0..10, fib_step)
        assert f0 == (0, 1) and f10 == (55, 89)

        let arr: [int; 4] = [1, 3, 5, 7]
        let sc = scan(0, arr, add_int)
        assert sc == [1, 4, 9, 16]
        assert fold_seq(0, arr, add_int) == 16

        let t_arr: [int; 3] = [-1, 0, 4]
        let bumped: [int] = map_seq(t_arr, bump_int)
        assert sum(bumped) == 6
        assert dot([-1, 0, 4], bumped) == 20

        let empty_ints: [int] = []
        let empty_r: [ratio] = []
        assert (sum(empty_ints), sum(empty_r)) == (0, 0/1r)
        assert dot(empty_ints, empty_ints) == 0

        # BigTrit spill (> T32_LIMIT & > i64::MAX) in sum and dot
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
        &opts,
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
    let ll = emit_llvm_file_with_options(&ev_path, OptLevel::O2, &opts).unwrap();
    let has_arena_box_call = ll.lines().any(|line| {
        let t = line.trim();
        t.contains("call ") && (t.contains("@aod_rt_val_from_") || t.contains("@aod_rt_list_"))
    });
    assert!(
        !has_arena_box_call,
        "evolve hot loop must stay 100% unboxed without arena boxing:\n{ll}"
    );

    // Hostile compile-time controls:
    // 1. `dot` on mismatched static `[int; 2]` vs `[int; 3]`
    let bad_dot_arr = r#"
use aod::core:
    dot
dec main:
    () -> int:
        let a: [int; 2] = [1, 2]
        let b: [int; 3] = [1, 2, 3]
        return dot(a, b)
"#;
    assert_eq!(
        check_source_with_options(bad_dot_arr, &opts)
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
        check_source_with_options(leak_evolve, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 3a. Passing `ReadOnly<[int]>` into non-ReadOnly `evolve` step function -> ObserverMutationForbidden!
    let ro_strip_evolve = r#"
use aod::core:
    evolve
dec non_ro_step:
    (xs: [int], _: int) -> [int]:
        return xs
dec z_caller:
    (ro_xs: ReadOnly<[int]>) -> ReadOnly<[int]>:
        return evolve(ro_xs, 0..2, non_ro_step)
dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source_with_options(ro_strip_evolve, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 3b. `observe` probe calling `evolve(view.carriers, ...)` whose step calls `list_set` -> ObserverMutationForbidden!
    let obs_mut_evolve = r#"
use aod::core:
    len, list_set, evolve

dec mutating_carriers_step:
    (xs: [int], t: int) -> [int]:
        return list_set(xs, 0, t)

observe BadObs:
    tracer tr:
        (view: ReadOnly<ChamberView>) -> null:
            let res = evolve(view.carriers, 0..2, mutating_carriers_step)
            emit len(res)
            return null

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(obs_mut_evolve).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // Hostile 6-mode runtime traps:
    assert_6mode_runtime_trap(
        "use aod::core:\n    evolve\ndec st:\n    (s: int, t: int) -> int:\n        return s + t\ndec mk_w:\n    (x: int) -> window:\n        return (x, x]\ndec main:\n    () -> int:\n        return evolve(0, mk_w(5), st)\n",
        &opts,
        10_000,
        &tmp,
        "bad_win_evolve",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    evolve\ndec st:\n    (s: int, t: int) -> int:\n        return s + t\ndec main:\n    () -> int:\n        return evolve(0, 0..10000, st)\n",
        &opts,
        40,
        &tmp,
        "fuel_evolve",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    dot\ndec main:\n    () -> int:\n        return dot([1, 2], [1, 2, 3])\n",
        &opts,
        10_000,
        &tmp,
        "dot_len_mismatch",
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac06_ac07_bytes_overloads_and_qext_bytes_quarantine() {
    let tmp = temp_dir("ac06_ac07");
    let opts = LoadOptions::default();

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
        &opts,
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
        check_source_with_options(leak_ret, &opts).unwrap_err().code,
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
        check_source_with_options(leak_sha, &opts).unwrap_err().code,
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
        check_source_with_options(outside_qext, &opts)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 4. Invalid UTF-8 byte sequences trap cleanly across all 6 modes!
    assert_6mode_runtime_trap(
        "use aod::core:\n    addr8\ndec main:\n    () -> addr8:\n        return addr8(b\"\\xff\\xfe\")\n",
        &opts,
        10_000,
        &tmp,
        "bad_utf8_addr8",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    support\ndec main:\n    () -> support:\n        return support(b\"\\xff\")\n",
        &opts,
        10_000,
        &tmp,
        "bad_utf8_support",
    );
    assert_6mode_runtime_trap(
        "use aod::qext:\n    ext_decimal, qext_to_bytes_exact\ndec main:\n    () -> int:\n        quarantine Q_ext:\n            emit qext_to_bytes_exact(ext_decimal(b\"\\x80\"))\n        return 0\n",
        &opts,
        10_000,
        &tmp,
        "bad_utf8_qext",
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_exact_scalar_and_rational_intrinsics_and_pow_bit_ceiling() {
    let tmp = temp_dir("ac08");
    let opts = LoadOptions::default();

    // Includes BigTrit spill (`-2^63`, `2^100`, `3^50`) on `num`, `den`, `abs`, `signum`, `gcd`, `lcm`, `pow`!
    let src = r#"
use aod::core:
    t1, b8, b64_bits, ratio
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
        assert min(-5, 2) == -5 and max(-9, -2) == -2
        assert clamp(b8(200), b8(10), b8(100)) == b8(100)
        assert clamp(b64_bits(1), b64_bits(5), b64_bits(9)) == b64_bits(5)
        assert clamp(t1(-1), 0t, 1t) == 0t
        return 1
"#;
    let src_path = tmp.join("intrinsics_spill.dec");
    fs::write(&src_path, src).unwrap();
    assert_6mode_with_opts(&src_path, "1\n", &opts, &tmp, "intrinsics_spill");

    // Hostile runtime traps across all 6 modes:
    assert_6mode_runtime_trap(
        "use aod::core:\n    clamp\ndec main:\n    () -> int:\n        return clamp(5, 10, 2)\n",
        &opts,
        10_000,
        &tmp,
        "clamp_bad_bounds",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> int:\n        return pow(2, -1)\n",
        &opts,
        10_000,
        &tmp,
        "pow_neg_int",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> ratio:\n        return pow(0/1r, -1)\n",
        &opts,
        10_000,
        &tmp,
        "pow_zero_ratio_neg",
    );
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> int:\n        return pow(2, 65537)\n",
        &opts,
        10_000,
        &tmp,
        "pow_exp_cap",
    );

    // P1-6 OOM guard: chained `pow(pow(10, 65536), 65536)` MUST trap immediately (zero OOM!)
    let t0 = Instant::now();
    assert_6mode_runtime_trap(
        "use aod::core:\n    pow\ndec main:\n    () -> int:\n        return pow(pow(10, 65536), 65536)\n",
        &opts,
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
fn ac09_ac10_fmt_idempotence_symlink_guards_and_hermetic_ir_tamper_defense() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    let tmp = temp_dir("ac09_ac10");
    let opts = LoadOptions::default();

    // 1. `format_source` & `format_program` idempotence, `[T; N]` canonical formatting, and `0_H`/`0_D` preservation
    let messy_src = r#"
use aod::core:
    t1

dec check_trits:
    (a: array[t1, 2]) -> int:
        assert a[0] == 0_H and a[1] == 0_D
        return int(a[0]) + int(a[1])

dec main:
    () -> int:
        return   check_trits([0_H,   0_D])
"#;
    let f1 = format_source(messy_src).expect("format_source failed");
    assert!(f1.contains("[t1; 2]"), "expected canonical [T; N] formatting, got:\n{f1}");
    assert!(f1.contains("0_H") && f1.contains("0_D"), "expected 0_H/0_D preserved:\n{f1}");
    let prog = parse_source(&f1).unwrap();
    let f2 = format_program(&prog);
    assert_eq!(f1, f2, "format_source / format_program must be idempotent");
    check_source(&f1).expect("formatted program must typecheck");

    // 2. CLI `aodc fmt` (`stdout`, `--check`, `--write`, `-o`, self-overwrite refusal, and symlink guards)
    let unformatted = tmp.join("input_unfmt.dec");
    fs::write(&unformatted, messy_src).unwrap();

    // `--check` on unformatted file exits 1 without modifying the file
    let chk_dirty = Command::new(aodc_bin())
        .args(["fmt", "--check"])
        .arg(&unformatted)
        .output()
        .unwrap();
    assert_eq!(chk_dirty.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&unformatted).unwrap(), messy_src);

    // `-o` self-overwrite refused
    let self_dash_o = Command::new(aodc_bin())
        .args(["fmt", "-o"])
        .arg(&unformatted)
        .arg(&unformatted)
        .output()
        .unwrap();
    assert!(!self_dash_o.status.success());

    // `-o <separate>` writes canonical formatting
    let formatted_out = tmp.join("formatted_copy.dec");
    let ok_dash_o = Command::new(aodc_bin())
        .args(["fmt", "-o"])
        .arg(&formatted_out)
        .arg(&unformatted)
        .output()
        .unwrap();
    assert!(
        ok_dash_o.status.success(),
        "fmt -o failed: {}",
        String::from_utf8_lossy(&ok_dash_o.stderr)
    );
    assert_eq!(fs::read_to_string(&formatted_out).unwrap(), f1);

    // Symlink rejection on `aodc fmt --write` and `aodc fmt -o` (must NOT clobber victim!)
    #[cfg(unix)]
    {
        let victim = tmp.join("victim.dec");
        let sentinel = "# DO NOT CLOBBER VICTIM\ndec main:\n    () -> int:\n        return 99\n";
        fs::write(&victim, sentinel).unwrap();

        let sym_in_place = tmp.join("sym_write.dec");
        symlink(&victim, &sym_in_place).unwrap();
        let out_write = Command::new(aodc_bin())
            .args(["fmt", "--write"])
            .arg(&sym_in_place)
            .output()
            .unwrap();
        assert!(
            !out_write.status.success(),
            "fmt --write on symlink must fail"
        );
        assert_eq!(
            fs::read_to_string(&victim).unwrap(),
            sentinel,
            "victim clobbered by fmt --write!"
        );

        let sym_out = tmp.join("sym_out.dec");
        symlink(&victim, &sym_out).unwrap();
        let out_sym_o = Command::new(aodc_bin())
            .args(["fmt", "-o"])
            .arg(&sym_out)
            .arg(&unformatted)
            .output()
            .unwrap();
        assert!(
            !out_sym_o.status.success(),
            "fmt -o <symlink> must fail"
        );
        assert_eq!(
            fs::read_to_string(&victim).unwrap(),
            sentinel,
            "victim clobbered by fmt -o!"
        );
    }

    // Atomic `--write` followed by `--check` exits 0
    let ok_write = Command::new(aodc_bin())
        .args(["fmt", "--write"])
        .arg(&unformatted)
        .output()
        .unwrap();
    assert!(ok_write.status.success());
    let ok_check = Command::new(aodc_bin())
        .args(["fmt", "--check"])
        .arg(&unformatted)
        .output()
        .unwrap();
    assert!(ok_check.status.success());

    // 3. Transitive module check: clean entry importing a module with legacy `Dec`/`if` MUST fail!
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
    assert!(check_file_with_options(&imp_entry, &opts).is_err());

    // 4. Hermetic DecIR v3 `syntax_discipline: "pure_wave_v2"` tamper defense!
    let valid_pure = "dec main:\n    () -> int:\n        return 7\n";
    let valid_pure_path = tmp.join("valid_pure.dec");
    fs::write(&valid_pure_path, valid_pure).unwrap();
    let ir_json_str = compile_file_to_ir_with_options(&valid_pure_path, &opts).unwrap();
    assert_eq!(exec_ir_str(&ir_json_str).unwrap().to_stdout_string(), "7\n");

    // Tamper A: smuggle legacy `Dec` + `mut` + `"str"` source into `"pure_wave_v2"` IR with recomputed SHA-256!
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
    let tampered_smuggled = JsonValue::Object(obj).to_pretty_utf8_json();
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
fn ac11_ac12_showcase_6mode_cli_and_audit_zero_regressions() {
    let tmp = temp_dir("ac11_ac12");
    let root = repo_root();
    let opts = LoadOptions::default();
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

    // 2. Full 6-mode parity
    assert_6mode_with_opts(&showcase, &expected, &opts, &tmp, "showcase_pure");

    // 3. `audit_dec_file_with_options` passes all 6 hooks
    let audit = audit_dec_file_with_options(&showcase, &opts);
    assert!(audit.passed(), "showcase audit-dec failed: {:?}", audit.errors);
    assert_eq!(audit.hooks_passed(), 6);

    // 4. End-to-end CLI subcommands on `pure_wave_signature_matrix.dec`
    let out_check = Command::new(aodc_bin())
        .arg("check")
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_check.status.success());

    let out_run = Command::new(aodc_bin())
        .arg("run")
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_run.status.success());
    assert_eq!(String::from_utf8_lossy(&out_run.stdout), expected);

    let ir_out = tmp.join("cli_showcase.ir.json");
    let out_compile = Command::new(aodc_bin())
        .arg("compile")
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

    let out_audit = Command::new(aodc_bin())
        .arg("audit-dec")
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_audit.status.success());
    assert!(String::from_utf8_lossy(&out_audit.stdout).contains("PASS (6/6 hooks"));

    let out_test = Command::new(aodc_bin())
        .arg("test")
        .arg(&showcase)
        .output()
        .unwrap();
    assert!(out_test.status.success());
    assert!(String::from_utf8_lossy(&out_test.stdout).contains("1 passed; 0 failed; 1 total"));

    let _ = fs::remove_dir_all(&tmp);
}
