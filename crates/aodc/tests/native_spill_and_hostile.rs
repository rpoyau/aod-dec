use std::path::PathBuf;
use std::process::Command;

use aod_rt::abi::OwnedRtContext;
use aod_rt::arena::RtArena;
use aod_rt::value::RtValue;
use aodc::diagnostics::{CompatMode, DiagnosticCode};
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::{
    build_aot_source, compile_jit_source, emit_llvm_source, jit_source_captured, run_source,
    run_source_with_budget, OptLevel, OrcJitSession,
};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aodc_spill_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn assert_parity_success(src: &str, tag: &str) {
    let expected = run_source(src, CompatMode::V2).unwrap().to_stdout_string();
    let tmp = temp_dir(tag);

    let exe = tmp.join("bin_o2");
    build_aot_source(src, CompatMode::V2, OptLevel::O2, &exe).unwrap();
    let out = Command::new(&exe).output().unwrap();
    assert!(
        out.status.success(),
        "AOT failed for {tag}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected, "AOT stdout mismatch ({tag})");

    let (jout, jres) =
        jit_source_captured(src, CompatMode::V2, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
            .unwrap();
    jres.unwrap_or_else(|e| panic!("JIT failed for {tag}: {e}"));
    assert_eq!(jout, expected, "JIT stdout mismatch ({tag})");

    let _ = std::fs::remove_dir_all(&tmp);
}

fn assert_parity_runtime_fail(src: &str, expected_prior_stdout: &str, fuel: usize, max_depth: usize, tag: &str) {
    // 1. VM fails
    assert!(
        run_source_with_budget(src, CompatMode::V2, fuel, max_depth).is_err(),
        "expected VM failure for {tag}"
    );

    // 2. AOT -O2 fails while preserving prior emits + `aodc:` on stderr + exit 1
    let tmp = temp_dir(tag);
    let exe = tmp.join("fail_o2");
    build_aot_source(src, CompatMode::V2, OptLevel::O2, &exe).unwrap();
    let out = Command::new(&exe)
        .args(["--fuel", &fuel.to_string(), "--max-depth", &max_depth.to_string()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected AOT exit 1 for {tag}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        expected_prior_stdout,
        "AOT did not preserve prior emits for {tag}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).starts_with("aodc:"),
        "AOT stderr missing aodc: prefix for {tag}: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 3. ORC LLJIT -O2 fails while preserving prior emits
    let (jout, jres) = jit_source_captured(src, CompatMode::V2, OptLevel::O2, fuel, max_depth).unwrap();
    assert!(jres.is_err(), "expected JIT failure for {tag}");
    assert_eq!(
        jout, expected_prior_stdout,
        "JIT did not preserve prior emits for {tag}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_int_overflow_spill_floor_div_bitwise_parity() {
    let src = r#"
Dec main() -> int:
    let max64 = 9223372036854775807
    let min64 = -9223372036854775807 - 1

    # Overflow spills + spill-and-unbox round-trip
    let p1 = max64 + 1
    let m1 = min64 - 1
    let sq = max64 * max64
    let neg_min = -min64
    let div_min = min64 // -1
    let rem_min = min64 % -1
    let roundtrip = p1 - 1
    assert roundtrip == max64
    assert neg_min == p1
    assert div_min == p1
    assert rem_min == 0

    # 256-bit arithmetic
    let big = sq * sq
    assert big // sq == sq

    # 4-quadrant exact Python floor division & remainder
    assert 7 // 3 == 2 and 7 % 3 == 1
    assert -7 // 3 == -3 and -7 % 3 == 2
    assert 7 // -3 == -3 and 7 % -3 == -2
    assert -7 // -3 == 2 and -7 % -3 == -1

    # Bitwise edge cases: << 63 spills cleanly, >> 64 returns 0 (no poison!)
    let s63 = 1 << 63
    assert s63 == p1
    assert (max64 >> 64) == 0
    assert (max64 >> 100) == 0
    assert (12 & 10) == 8 and (12 | 10) == 14 and (12 ^ 10) == 6

    emit p1
    emit m1
    emit sq
    emit big
    emit s63
    return 0
"#;
    assert_parity_success(src, "ac09_int_spill");

    // Negative bitwise rejection across VM, AOT, and JIT
    let neg_bit = "Dec main() -> int:\n    emit \"before-neg-bit\"\n    let x = (-1) & 3\n    return x\n";
    assert_parity_runtime_fail(neg_bit, "before-neg-bit\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac09_neg_bit");

    // Div by zero rejection across VM, AOT, and JIT
    let div0 = "Dec main() -> int:\n    emit \"before-div0\"\n    let x = 7 // 0\n    return x\n";
    assert_parity_runtime_fail(div0, "before-div0\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac09_div0");
}

#[test]
fn ac10_t1_t32_q4_ratio_spill_and_bit63_pack_unpack_parity() {
    let src = r#"
Dec main() -> int:
    # P0-3: T32 extremes + bit-63-set pack(t32(-T32::LIMIT)) spill & unpack round-trip!
    let lim = t32(926510094425920)
    let neg_lim = t32(-926510094425920)
    let p_pos = pack(lim)
    let p_neg = pack(neg_lim)
    assert unpack(p_pos) == lim
    assert unpack(p_neg) == neg_lim
    assert p_pos == 6148914691236517205 and p_neg == 12297829382473034410
    assert lim + neg_lim == t32(0)

    # Exact ratio -> int conversion
    assert int(6/2r) == 3

    # Huge ratio spill beyond i64 numerator/denominator
    let big_n = 9223372036854775807 * 9223372036854775807
    let big_d = 9223372036854775807 + 2
    let r_big = ratio(big_n, big_d)
    let r_sum = r_big + 1/3r
    assert r_sum > r_big

    emit p_pos
    emit p_neg
    emit r_sum
    return 0
"#;
    assert_parity_success(src, "ac10_t32_ratio_spill");

    // Hostile T32 overflow
    let t32_ov = "Dec main() -> int:\n    emit \"before-t32\"\n    let x = t32(926510094425920) + t32(1)\n    return int(x)\n";
    assert_parity_runtime_fail(t32_ov, "before-t32\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_t32_ov");

    // Hostile unpack(3) (invalid 0b11 lane)
    let bad_unpack = "Dec main() -> int:\n    emit \"before-unpack\"\n    let x = unpack(3)\n    return int(x)\n";
    assert_parity_runtime_fail(bad_unpack, "before-unpack\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_bad_unpack");

    // Hostile unpack(-1)
    let neg_unpack = "Dec main() -> int:\n    emit \"before-neg-unpack\"\n    let x = unpack(-1)\n    return int(x)\n";
    assert_parity_runtime_fail(neg_unpack, "before-neg-unpack\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_neg_unpack");

    // Hostile non-integer ratio -> int(7/2r)
    let bad_int_rat = "Dec main() -> int:\n    emit \"before-rat-int\"\n    let x = int(7/2r)\n    return x\n";
    assert_parity_runtime_fail(bad_int_rat, "before-rat-int\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_bad_int_rat");
}

#[test]
fn ac12_budgets_assert_and_procedural_observer_failure_preserve_emits() {
    // 1. Coarsened native fuel exhaustion preserves prior emit
    let fuel_loop = "Dec main() -> int:\n    emit \"pre-fuel\"\n    mut i = 0\n    while true:\n        i = i + 1\n    return i\n";
    assert_parity_runtime_fail(fuel_loop, "pre-fuel\n", 15, DEFAULT_MAX_DEPTH, "ac12_fuel");

    // 2. Call depth exhaustion preserves prior emit
    let depth_rec = "Dec dive(n: int) -> int:\n    emit n\n    return dive(n + 1)\n\nDec main() -> int:\n    return dive(1)\n";
    assert_parity_runtime_fail(depth_rec, "1\n2\n3\n4\n", DEFAULT_FUEL, 5, "ac12_depth");

    // 3. Assertion failure preserves prior emit
    let assert_fail = "Dec main() -> int:\n    emit \"pre-assert\"\n    assert false\n    return 0\n";
    assert_parity_runtime_fail(assert_fail, "pre-assert\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac12_assert");

    // 4. P1-3: Procedural observer callback failure halts simulate immediately & preserves prior emits!
    let obs_fail = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in C:
    inject 3:3:6 at (4, 4, 4) dir +X orient +1t shells 1

observe FailObs:
    sensor s(view) -> int:
        emit view.tick
        assert view.tick < 2
        return view.tick

Dec main() -> int:
    emit "start-sim"
    let r = simulate P for 10 ticks with FailObs
    emit "unreachable"
    return r.ticks
"#;
    assert_parity_runtime_fail(
        obs_fail,
        "start-sim\n1\n2\n",
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
        "ac12_obs_fail",
    );
}

#[test]
fn ac13_arena_invalid_handle_and_max_entries_cap_safety() {
    // 1. Safe Rust arena direct verification
    let mut arena = RtArena::new(3);
    assert!(arena.get(0).is_err());
    assert!(arena.get(1).is_err());
    let h1 = arena.insert(RtValue::Str("a".to_string())).unwrap();
    let h2 = arena.insert(RtValue::Str("b".to_string())).unwrap();
    let h3 = arena.insert(RtValue::Str("c".to_string())).unwrap();
    assert_eq!((h1, h2, h3), (1, 2, 3));
    assert!(arena.insert(RtValue::Str("overflow".to_string())).is_err());
    assert!(arena.get(99).is_err());

    // 2. End-to-end C ABI cap exhaustion via ORC LLJIT with tiny max_arena_entries=2 (zero unsafe in test!)
    let alloc_prog = r#"
Dec main() -> int:
    let a = "one"
    let b = "two"
    let c = "three"
    emit a
    emit b
    emit c
    return 0
"#;
    let sess = compile_jit_source(alloc_prog, CompatMode::V2, OptLevel::O0).unwrap();
    let mut cap_ctx = OwnedRtContext::with_arena_cap(1000, 32, true, 2);
    sess.execute(&mut cap_ctx);
    assert!(cap_ctx.failed(), "expected arena cap failure");
    assert!(
        cap_ctx
            .error_message()
            .unwrap_or("")
            .contains("arena entry limit"),
        "unexpected cap error: {:?}",
        cap_ctx.error_message()
    );

    // 3. End-to-end C ABI null handle 0 and stale handle 999 rejection without panic or UB!
    let bad_handle0_ir = r#"
declare i8 @aod_rt_handle_eq(ptr, i64, i64)
define void @aod_jit_entry(ptr %ctx) {
entry:
  %r = call i8 @aod_rt_handle_eq(ptr %ctx, i64 0, i64 0)
  ret void
}
"#;
    let s0 = OrcJitSession::compile(bad_handle0_ir, OptLevel::O0).unwrap();
    let mut c0 = OwnedRtContext::new(100, 16, true);
    s0.execute(&mut c0);
    assert!(c0.failed());
    assert!(
        c0.error_message().unwrap_or("").contains("handle 0"),
        "got: {:?}",
        c0.error_message()
    );

    let bad_stale_ir = r#"
declare void @aod_rt_emit_handle(ptr, i64)
define void @aod_jit_entry(ptr %ctx) {
entry:
  call void @aod_rt_emit_handle(ptr %ctx, i64 999)
  ret void
}
"#;
    let s999 = OrcJitSession::compile(bad_stale_ir, OptLevel::O0).unwrap();
    let mut c999 = OwnedRtContext::new(100, 16, true);
    s999.execute(&mut c999);
    assert!(c999.failed());
    assert!(
        c999.error_message().unwrap_or("").contains("handle 999"),
        "got: {:?}",
        c999.error_message()
    );
}

#[test]
fn ac14_compile_time_firewalls_enforced_before_ir_or_artifacts() {
    let tmp = temp_dir("ac14_firewalls");
    let out_exe = tmp.join("should_not_exist");

    let hostile_cases = [
        (
            "use aod::qext::ext_decimal\nDec main() -> int:\n    let q = ext_decimal(\"1.25\")\n    return 0\n",
            DiagnosticCode::QExtLeakageForbidden,
        ),
        (
            "observe Bad:\n    tracer t(view):\n        view.tick = 1\nDec main() -> int:\n    return 0\n",
            DiagnosticCode::ObserverMutationForbidden,
        ),
        (
            "Dec main() -> int:\n    let x = 1.5\n    return 0\n",
            DiagnosticCode::FloatLiteralForbidden,
        ),
    ];

    for (src, expected_code) in hostile_cases {
        assert_eq!(
            emit_llvm_source(src, CompatMode::V2, OptLevel::O2).unwrap_err().code,
            expected_code
        );
        assert_eq!(
            build_aot_source(src, CompatMode::V2, OptLevel::O2, &out_exe)
                .unwrap_err()
                .code,
            expected_code
        );
        assert!(!out_exe.exists(), "firewall failure must not create output executable");
        assert_eq!(
            compile_jit_source(src, CompatMode::V2, OptLevel::O2)
                .err()
                .unwrap()
                .code,
            expected_code
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac16_twenty_sequential_orc_jit_sessions_mix_ok_and_fail() {
    for i in 0..20 {
        if i % 5 == 4 {
            // Malformed LLVM IR failure cleanly rejected
            assert!(OrcJitSession::compile("define broken {{{", OptLevel::O0).is_err());
        } else if i % 3 == 2 {
            // Runtime assertion failure inside ORC JIT session cleanly dropped
            let src = format!(
                "Dec main() -> int:\n    emit {i}\n    assert false\n    return 0\n"
            );
            let sess = compile_jit_source(&src, CompatMode::V2, OptLevel::O2).unwrap();
            let (out, res) = sess.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
            assert!(res.is_err());
            assert_eq!(out, format!("{i}\n"));
        } else {
            // Succeeding session with unique value
            let src = format!(
                "Dec main() -> int:\n    let x = {i} * 7 + 1\n    emit x\n    return x\n"
            );
            let sess = compile_jit_source(&src, CompatMode::V2, OptLevel::O2).unwrap();
            let (out, res) = sess.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
            res.unwrap();
            let expected = i * 7 + 1;
            assert_eq!(out, format!("{expected}\n{expected}\n"));
        }
    }
}
