use std::path::PathBuf;
use std::process::Command;

use aod_rt::abi::OwnedRtContext;
use aod_rt::arena::RtArena;
use aod_rt::value::RtValue;
use aodc::diagnostics::DiagnosticCode;
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
    let expected = run_source(src).unwrap().to_stdout_string();
    let tmp = temp_dir(tag);

    let exe = tmp.join("bin_o2");
    build_aot_source(src, OptLevel::O2, &exe).unwrap();
    let out = Command::new(&exe).output().unwrap();
    assert!(
        out.status.success(),
        "AOT failed for {tag}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected, "AOT stdout mismatch ({tag})");

    let (jout, jres) =
        jit_source_captured(src, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).unwrap();
    jres.unwrap_or_else(|e| panic!("JIT failed for {tag}: {e}"));
    assert_eq!(jout, expected, "JIT stdout mismatch ({tag})");

    let _ = std::fs::remove_dir_all(&tmp);
}

fn assert_parity_runtime_fail(
    src: &str,
    expected_prior_stdout: &str,
    fuel: usize,
    max_depth: usize,
    tag: &str,
) {
    // 1. VM fails
    assert!(
        run_source_with_budget(src, fuel, max_depth).is_err(),
        "expected VM failure for {tag}"
    );

    // 2. AOT -O2 fails while preserving prior emits + `aodc:` on stderr + exit 1
    let tmp = temp_dir(tag);
    let exe = tmp.join("fail_o2");
    build_aot_source(src, OptLevel::O2, &exe).unwrap();
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
    let (jout, jres) = jit_source_captured(src, OptLevel::O2, fuel, max_depth).unwrap();
    assert!(jres.is_err(), "expected JIT failure for {tag}");
    assert_eq!(
        jout, expected_prior_stdout,
        "JIT did not preserve prior emits for {tag}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_trit_boxed_int_inline_boundary_spill_floor_div_and_b64_bitwise_parity() {
    let src = r#"
dec main() -> int:
    let max_t32 = 926510094425920
    let min_t32 = -926510094425920

    # Inline 32-trit boundary -> BigTrit spill (`0xD000... | handle`) -> unspill round-trip
    let p1 = max_t32 + 1
    let m1 = min_t32 - 1
    let sq = max_t32 * max_t32
    let neg_min = -m1
    let div_min = m1 // -1
    let rem_min = m1 % -1
    let roundtrip = p1 - 1
    assert roundtrip == max_t32
    assert neg_min == p1
    assert div_min == p1
    assert rem_min == 0

    # Multi-limb base-3^32 BigTrit arithmetic
    let big = sq * sq
    assert big // sq == sq

    # 4-quadrant exact floor division & remainder
    assert 7 // 3 == 2 and 7 % 3 == 1
    assert -7 // 3 == -3 and -7 % 3 == 2
    assert 7 // -3 == -3 and 7 % -3 == -2
    assert -7 // -3 == 2 and -7 % -3 == -1

    # Balanced-ternary trit shifts & b64 raw bitwise operations
    let t40 = trit_shl(1, 40)
    assert trit_shr(t40, 40) == 1
    let raw12 = b64_bits(12)
    let raw10 = b64_bits(10)
    assert u64_int(raw12 & raw10) == 8
    assert u64_int(raw12 | raw10) == 14
    assert u64_int(raw12 ^ raw10) == 6
    assert u64_int(b64_bits(1) << 16) == 65536

    emit p1
    emit m1
    emit sq
    emit big
    emit t40
    0
"#;
    assert_parity_success(src, "ac09_int_spill");

    // Div by zero rejection across VM, AOT, and JIT
    let div0 = "dec main() -> int:\n    emit \"before-div0\"\n    7 // 0\n";
    assert_parity_runtime_fail(div0, "before-div0\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac09_div0");
}

#[test]
fn ac10_trit_boxed_b64_view_pack_ratio_and_huge_ratio_spill_parity() {
    let src = r#"
dec main() -> int:
    # 0-cycle Trit-Boxed `b64(int)` <-> `int(b64)` register view at ±(3^32 - 1)/2!
    let lim = 926510094425920
    let neg_lim = -926510094425920
    let p_pos = b64(lim)
    let p_neg = b64(neg_lim)
    assert int(p_pos) == lim
    assert int(p_neg) == neg_lim
    assert u64_int(p_pos) == 6148914691236517205
    assert u64_int(p_neg) == 12297829382473034410
    assert lim + neg_lim == 0

    # Compact 1-word `[num][11][den]` `pack_ratio` / `unpack_ratio` round-trip
    let pr1 = pack_ratio(1/3r)
    let pr2 = pack_ratio(-22/7r)
    assert unpack_ratio(pr1) == 1/3r
    assert unpack_ratio(pr2) == -22/7r

    # Exact ratio -> int conversion
    assert int(6/2r) == 3

    # Coupled 2-word `ratio` with one negative inline word (`0x8000...`) and positive inline word (`0x4000...`),
    # plus huge BigTrit numerator/denominator spill!
    let r_mixed = ratio(neg_lim, lim - 1)
    assert r_mixed < 0/1r
    let big_n = 9223372036854775807 * 9223372036854775807
    let big_d = 9223372036854775807 + 2
    let r_big = ratio(big_n, big_d)
    let r_sum = r_big + 1/3r
    assert r_sum > r_big

    emit u64_int(p_pos)
    emit u64_int(p_neg)
    emit r_sum
    0
"#;
    assert_parity_success(src, "ac10_trit_ratio_spill");

    // Hostile `b64(spilled_int)` (> 32 trits) rejected across VM, AOT, JIT
    let spill_b64 = "dec main() -> int:\n    emit \"before-b64\"\n    let w = b64(926510094425920 + 1)\n    u64_int(w)\n";
    assert_parity_runtime_fail(spill_b64, "before-b64\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_spill_b64");

    // Hostile `int(b64_bits(3))` (invalid 0b11 lane) rejected across VM, AOT, JIT
    let bad_unpack = "dec main() -> int:\n    emit \"before-unpack\"\n    int(b64_bits(3))\n";
    assert_parity_runtime_fail(bad_unpack, "before-unpack\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_bad_unpack");

    // Hostile `pack_ratio` exceeding 31 combined trits rejected across VM, AOT, JIT
    let ovf_pack_ratio = "dec main() -> int:\n    emit \"before-pr\"\n    let w = pack_ratio(ratio(926510094425920, 926510094425919))\n    u64_int(w)\n";
    assert_parity_runtime_fail(ovf_pack_ratio, "before-pr\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_ovf_pr");

    // Hostile non-integer ratio -> int(7/2r)
    let bad_int_rat = "dec main() -> int:\n    emit \"before-rat-int\"\n    int(7/2r)\n";
    assert_parity_runtime_fail(bad_int_rat, "before-rat-int\n", DEFAULT_FUEL, DEFAULT_MAX_DEPTH, "ac10_bad_int_rat");
}

#[test]
fn ac12_budgets_assert_and_procedural_observer_failure_preserve_emits() {
    // 1. Coarsened native fuel exhaustion preserves prior emit
    let fuel_loop = "dec inc(x: int) -> int:\n    x + 1\n\ndec main() -> int:\n    emit \"pre-fuel\"\n    walk(0, 1000, inc)\n";
    assert_parity_runtime_fail(fuel_loop, "pre-fuel\n", 15, DEFAULT_MAX_DEPTH, "ac12_fuel");

    // 2. Call depth exhaustion preserves prior emit
    let depth_rec = "dec dive(n: int) -> int:\n    emit n\n    dive(n + 1)\n\ndec main() -> int:\n    dive(1)\n";
    assert_parity_runtime_fail(depth_rec, "1\n2\n3\n4\n", DEFAULT_FUEL, 5, "ac12_depth");

    // 3. Assertion failure preserves prior emit
    let assert_fail = "dec main() -> int:\n    emit \"pre-assert\"\n    assert false\n    0\n";
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
        view.tick

dec main() -> int:
    emit "start-sim"
    let r = simulate P over 10 ticks with FailObs
    emit "unreachable"
    r.ticks
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
dec main() -> int:
    let a = "one"
    let b = "two"
    let c = "three"
    emit a
    emit b
    emit c
    0
"#;
    let sess = compile_jit_source(alloc_prog, OptLevel::O0).unwrap();
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
            "use aod::qext:\n    ext_decimal\ndec main() -> int:\n    let q = ext_decimal(\"1.25\")\n    0\n",
            DiagnosticCode::QExtLeakageForbidden,
        ),
        (
            "dec take_mut(v: ChamberView) -> int:\n    v.tick\nobserve Bad:\n    sensor t(view) -> int:\n        take_mut(view)\ndec main() -> int:\n    0\n",
            DiagnosticCode::ObserverMutationForbidden,
        ),
        (
            "dec main() -> int:\n    let x = 1.5\n    0\n",
            DiagnosticCode::FloatLiteralForbidden,
        ),
    ];

    for (src, expected_code) in hostile_cases {
        assert_eq!(
            emit_llvm_source(src, OptLevel::O2).unwrap_err().code,
            expected_code
        );
        assert_eq!(
            build_aot_source(src, OptLevel::O2, &out_exe)
                .unwrap_err()
                .code,
            expected_code
        );
        assert!(!out_exe.exists(), "firewall failure must not create output executable");
        assert_eq!(
            compile_jit_source(src, OptLevel::O2).err().unwrap().code,
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
                "dec main() -> int:\n    emit {i}\n    assert false\n    0\n"
            );
            let sess = compile_jit_source(&src, OptLevel::O2).unwrap();
            let (out, res) = sess.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
            assert!(res.is_err());
            assert_eq!(out, format!("{i}\n"));
        } else {
            // Succeeding session with unique value
            let src = format!(
                "dec main() -> int:\n    let x = {i} * 7 + 1\n    emit x\n    x\n"
            );
            let sess = compile_jit_source(&src, OptLevel::O2).unwrap();
            let (out, res) = sess.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
            res.unwrap();
            let expected = i * 7 + 1;
            assert_eq!(out, format!("{expected}\n{expected}\n"));
        }
    }
}
