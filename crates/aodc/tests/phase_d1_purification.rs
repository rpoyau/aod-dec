use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::json::{parse_strict_json, JsonValue};
use aodc::diagnostics::{CompatMode, DiagnosticCode};
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::{dumps_ir, loads_ir};
use aodc::{
    audit_dec_source, build_aot_file, certify_frg_channel, check_source, compile_file_to_ir,
    exec_ir_file, exec_ir_str, jit_file_captured, jit_source_captured, lex_live_walk,
    run_all_hw_probes, run_file, run_probe_frg_ring_gate, run_source, validate_portable_report,
    verify_frg_identity, verify_sheddic_route, OptLevel, DEC_AUDIT_SCHEMA,
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
        "aodc_pr10_test_{tag}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Assert exact 6-mode (`run`, `compile+exec`, `build -O0`, `build -O2`, `jit -O0`, `jit -O2`)
/// + `-O3` stdout parity on an in-memory `.dec` source.
fn assert_6mode_source_parity(src: &str, expected: &str, tmp: &Path, stem: &str) {
    let src_path = tmp.join(format!("{stem}.dec"));
    fs::write(&src_path, src).unwrap();
    assert_6mode_file_parity(&src_path, expected, tmp, stem);
}

/// Assert exact 6-mode (`run`, `compile+exec`, `build -O0`, `build -O2`, `jit -O0`, `jit -O2`)
/// + `-O3` stdout parity on a `.dec` file.
fn assert_6mode_file_parity(src_path: &Path, expected: &str, tmp: &Path, stem: &str) {
    // 1. Exact VM (`run`)
    let vm_out = run_file(src_path, CompatMode::V2)
        .unwrap_or_else(|e| panic!("{stem} VM failed: {e}"))
        .to_stdout_string();
    assert_eq!(vm_out, expected, "{stem}: VM stdout mismatch");

    // 2. Hermetic DecIR v2 (`compile + exec`) + deterministic round-trip
    let ir_text = compile_file_to_ir(src_path, CompatMode::V2)
        .unwrap_or_else(|e| panic!("{stem} compile failed: {e}"));
    let ir_path = tmp.join(format!("{stem}.ir.json"));
    fs::write(&ir_path, &ir_text).unwrap();
    let reloaded = loads_ir(&ir_text).unwrap_or_else(|e| panic!("{stem} loads_ir failed: {e}"));
    assert_eq!(
        dumps_ir(&reloaded),
        ir_text,
        "{stem}: DecIR v2 round-trip must be byte-for-byte deterministic"
    );
    let ir_out = exec_ir_file(&ir_path)
        .unwrap_or_else(|e| panic!("{stem} exec failed: {e}"))
        .to_stdout_string();
    assert_eq!(ir_out, expected, "{stem}: DecIR exec stdout mismatch");

    // 3 & 4. Native AOT ELF (`build -O0`, `build -O2`, and `build -O3`)
    for (tag, opt) in [
        ("o0", OptLevel::O0),
        ("o2", OptLevel::O2),
        ("o3", OptLevel::O3),
    ] {
        let exe = tmp.join(format!("{stem}_{tag}_exe"));
        build_aot_file(src_path, CompatMode::V2, opt, &exe)
            .unwrap_or_else(|e| panic!("{stem} AOT {tag} build failed: {e}"));
        let aot = Command::new(&exe).output().unwrap();
        assert!(
            aot.status.success(),
            "{stem} AOT {tag} exited {:?}: {}",
            aot.status.code(),
            String::from_utf8_lossy(&aot.stderr)
        );
        assert_eq!(
            String::from_utf8(aot.stdout).unwrap(),
            expected,
            "{stem}: AOT {tag} stdout mismatch"
        );
    }

    // 5 & 6. In-process ORC LLJIT (`jit -O0`, `jit -O2`, and `jit -O3`)
    for (tag, opt) in [
        ("o0", OptLevel::O0),
        ("o2", OptLevel::O2),
        ("o3", OptLevel::O3),
    ] {
        let (jit_out, res) =
            jit_file_captured(src_path, CompatMode::V2, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .unwrap_or_else(|e| panic!("{stem} JIT {tag} compile failed: {e}"));
        res.unwrap_or_else(|e| panic!("{stem} JIT {tag} runtime failed: {e}"));
        assert_eq!(jit_out, expected, "{stem}: JIT {tag} stdout mismatch");
    }
}

/// Assert that a runtime trap fails cleanly across ALL 6 execution modes (`run`, `compile+exec`,
/// `build -O0`, `build -O2`, `jit -O0`, `jit -O2`) without UB, poison, or SIGFPE.
fn assert_6mode_runtime_trap(src: &str, tmp: &Path, stem: &str) {
    let vm_err = run_source(src, CompatMode::V2).expect_err(&format!("{stem}: VM must trap"));
    assert_eq!(vm_err.code, DiagnosticCode::RuntimeError);

    let src_path = tmp.join(format!("{stem}_trap.dec"));
    fs::write(&src_path, src).unwrap();

    let ir = compile_file_to_ir(&src_path, CompatMode::V2)
        .unwrap_or_else(|e| panic!("{stem}: compile_file_to_ir failed: {e}"));
    let ir_err = exec_ir_str(&ir).expect_err(&format!("{stem}: DecIR exec must trap"));
    assert_eq!(ir_err.code, DiagnosticCode::RuntimeError);

    for (tag, opt) in [("o0", OptLevel::O0), ("o2", OptLevel::O2)] {
        let exe = tmp.join(format!("{stem}_trap_{tag}_exe"));
        build_aot_file(&src_path, CompatMode::V2, opt, &exe)
            .unwrap_or_else(|e| panic!("{stem}: AOT {tag} build failed: {e}"));
        let out = Command::new(&exe).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{stem}: AOT {tag} must exit cleanly with code 1 (no SIGFPE/crash), got {:?}",
            out.status
        );
    }

    for (tag, opt) in [("o0", OptLevel::O0), ("o2", OptLevel::O2)] {
        let (_, jit_res) =
            jit_source_captured(src, CompatMode::V2, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .unwrap_or_else(|e| panic!("{stem}: JIT {tag} compile failed: {e}"));
        assert!(jit_res.is_err(), "{stem}: JIT {tag} must trap at runtime");
    }
}

#[test]
fn ac01_null_distinctness_incomparability_and_v1_isolation() {
    // Compile-time incomparability with all numeric, ternary, hypercube, anchor, and unit types
    for (rhs, expected_code) in [
        ("0", DiagnosticCode::ImplicitCoercionForbidden),
        ("u8(0)", DiagnosticCode::ImplicitCoercionForbidden),
        ("b64(0)", DiagnosticCode::ImplicitCoercionForbidden),
        ("0t", DiagnosticCode::ImplicitCoercionForbidden),
        ("t9(0)", DiagnosticCode::ImplicitCoercionForbidden),
        ("t32(0)", DiagnosticCode::ImplicitCoercionForbidden),
        ("q4(0)", DiagnosticCode::ImplicitCoercionForbidden),
        ("00_8", DiagnosticCode::ImplicitCoercionForbidden),
        ("false", DiagnosticCode::ImplicitCoercionForbidden),
        ("()", DiagnosticCode::ImplicitCoercionForbidden),
    ] {
        let src = format!("Dec main() -> int:\n    let ok = null == {rhs}\n    return 0\n");
        let err = check_source(&src, CompatMode::V2)
            .expect_err(&format!("null == {rhs} must fail at compile time"));
        assert_eq!(err.code, expected_code, "mismatch on null == {rhs}: {err}");
    }

    // Arithmetic / ordering on Null is forbidden
    for bad in [
        "Dec main() -> int:\n    let x = -null\n    return 0\n",
        "Dec main() -> int:\n    let x = null < null\n    return 0\n",
        "Dec main() -> int:\n    let null = 1\n    return null\n",
        "Dec f(null: int) -> int:\n    return 0\nDec main() -> int:\n    return f(1)\n",
    ] {
        assert!(check_source(bad, CompatMode::V2).is_err());
    }

    // In --compat v1: `null` is unreserved as a v1 identifier, while unbound `null` expr & `Null` type reject
    let v1_ident_ok = r#"
Dec add_one(null) -> out:
    (int null) -> int out:
        out = null + 1

Dec main() -> out:
    () -> int out:
        out = add_one(4)
"#;
    let v1_res = run_source(v1_ident_ok, CompatMode::V1).unwrap();
    assert_eq!(v1_res.to_stdout_string(), "5\n");

    let v1_unbound_null = "Dec main() -> out:\n    () -> int out:\n        x = null\n        out = 0\n";
    assert!(check_source(v1_unbound_null, CompatMode::V1).is_err());
    let v1_null_type = "Dec main() -> out:\n    () -> Null out:\n        out = 0\n";
    assert!(check_source(v1_null_type, CompatMode::V1).is_err());
}

#[test]
fn ac02_u8_checked_arithmetic_and_6mode_traps() {
    let tmp = temp_dir("ac02_u8");

    // Compile-time rejections: unary `-u8`, implicit coercion with `int`
    assert_eq!(
        check_source("Dec main() -> int:\n    let x = -u8(1)\n    return 0\n", CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::TypeError
    );
    assert_eq!(
        check_source("Dec main() -> int:\n    let x = u8(1) + 1\n    return 0\n", CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    assert!(check_source("Dec main() -> out:\n    () -> u8 out:\n        out = 0\n", CompatMode::V1).is_err());

    // Positive 6-mode parity
    let pos = r#"
Dec main() -> int:
    let a = u8(200)
    let b = u8(55)
    emit a + b
    emit a - b
    emit u8(15) * u8(17)
    emit a // u8(7)
    emit a % u8(7)
    emit (u8(0xAA) & u8(0x0F)) | (u8(1) << 7)
    emit u8(0xFF) >> u8(4)
    emit u8(true)
    emit u8(q4(12))
    emit u8(t9(255))
    emit u8(t32(255))
    emit u8(b64(255))
    return 0
"#;
    assert_6mode_source_parity(
        pos,
        "u8(255)\nu8(145)\nu8(255)\nu8(28)\nu8(4)\nu8(138)\nu8(15)\nu8(1)\nu8(12)\nu8(255)\nu8(255)\nu8(255)\n0\n",
        &tmp,
        "u8_pos",
    );

    // 6-mode runtime traps: overflow, underflow, //0, %0, <<8, >>8, <<-1, u8(256), u8(-1)
    for (tag, expr) in [
        ("u8_add_ovf", "let _x = u8(250) + u8(10)"),
        ("u8_sub_unf", "let _x = u8(3) - u8(4)"),
        ("u8_mul_ovf", "let _x = u8(20) * u8(20)"),
        ("u8_div0", "let _x = u8(9) // u8(0)"),
        ("u8_rem0", "let _x = u8(9) % u8(0)"),
        ("u8_shl8_u8", "let _x = u8(1) << u8(8)"),
        ("u8_shr8_u8", "let _x = u8(1) >> u8(8)"),
        ("u8_shl8_int", "let _x = u8(1) << 8"),
        ("u8_shr_neg", "let _x = u8(1) >> (-1)"),
        ("u8_from_256", "let _x = u8(256)"),
        ("u8_from_neg", "let _x = u8(-1)"),
        ("u8_from_t9_neg", "let _x = u8(t9(-1))"),
        ("u8_from_b64_ovf", "let _x = u8(b64(256))"),
    ] {
        let src = format!("Dec main() -> int:\n    {expr}\n    return 0\n");
        assert_6mode_runtime_trap(&src, &tmp, tag);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac03_b64_hex_bin_lexing_compile_guards_and_6mode_traps() {
    let tmp = temp_dir("ac03_b64");

    // Malformed hex/binary literals -> LexError
    for bad_lex in [
        "0x",
        "0b",
        "0x_1",
        "0b_0",
        "0xFF_",
        "0b10_",
        "0xFF__00",
        "0b1__0",
        "0b102",
        "0x1G",
    ] {
        let src = format!("Dec main() -> int:\n    let x = {bad_lex}\n    return x\n");
        let err = check_source(&src, CompatMode::V2)
            .expect_err(&format!("{bad_lex} must fail lexing"));
        assert_eq!(err.code, DiagnosticCode::LexError, "for {bad_lex}: {err}");
    }

    // Float hex/binary -> FloatLiteralForbidden
    for bad_flt in ["0x1.5", "0b1.0"] {
        let src = format!("Dec main() -> int:\n    let x = {bad_flt}\n    return 0\n");
        let err = check_source(&src, CompatMode::V2).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::FloatLiteralForbidden);
    }

    // Hex range `0x1..0x5` must lex cleanly as IntLit(1) .. IntLit(5)
    assert!(check_source(
        "Dec main() -> int:\n    mut s = 0\n    for i in 0x1..0x5:\n        s = s + i\n    assert s == 10\n    return 0\n",
        CompatMode::V2
    ).is_ok());

    // Rejected in --compat v1
    assert_eq!(
        check_source("Dec main() -> out:\n    () -> int out:\n        out = 0x10\n", CompatMode::V1)
            .unwrap_err()
            .code,
        DiagnosticCode::LexError
    );

    // Arithmetic (`+`, `-`, `*`, `/`, `//`, `%`) and unary `-` on `b64` are forbidden at compile time
    for op_expr in [
        "-b64(1)",
        "b64(1) + b64(2)",
        "b64(2) - b64(1)",
        "b64(2) * b64(3)",
        "b64(4) / b64(2)",
        "b64(4) // b64(2)",
        "b64(4) % b64(2)",
    ] {
        let src = format!("Dec main() -> int:\n    let _x = {op_expr}\n    return 0\n");
        let err = check_source(&src, CompatMode::V2)
            .expect_err(&format!("{op_expr} must fail on b64"));
        assert_eq!(err.code, DiagnosticCode::TypeError);
    }

    // Positive 6-mode parity including bit 63 set (`0xFFFF_FFFF_FFFF_FFFF`, `0xAAAA_AAAA_AAAA_AAAA`)
    // AND 1-cycle T32 wire-swap phase flip `((w & 0x5555...) << 1) | ((w & 0xAAAA...) >> 1)` == `b64(neg(t))`
    let pos = r#"
Dec wire_swap_flip(x: t32) -> t32:
    let w = b64(x)
    let lo = (w & b64(0x5555_5555_5555_5555)) << 1
    let hi = (w & b64(0xAAAA_AAAA_AAAA_AAAA)) >> 1
    return t32(lo | hi)

Dec main() -> int:
    let top = b64(0xFFFF_FFFF_FFFF_FFFF)
    assert int(top) == 18446744073709551615
    assert popcnt(top) == 64
    assert bitnot(top) == b64(0)
    let neg_trits = b64(0xAAAA_AAAA_AAAA_AAAA)
    let decoded = t32(neg_trits)
    assert decoded == t32(-926510094425920)
    assert b64(decoded) == neg_trits
    for v in [t32(0), t32(1), t32(-1), t32(926510094425920), t32(-926510094425920)]:
        assert wire_swap_flip(v) == neg(v)
    emit top
    emit decoded
    return 0
"#;
    assert_6mode_source_parity(
        pos,
        "b64(0xffffffffffffffff)\nt32(-926510094425920)\n0\n",
        &tmp,
        "b64_pos",
    );

    // 6-mode runtime traps: shift >= 64, shift < 0, b64(-1), b64(2^64), invalid 0b11 lane in t32(b64)
    for (tag, expr) in [
        ("b64_shl64_b64", "let _x = b64(1) << b64(64)"),
        ("b64_shr64_b64", "let _x = b64(1) >> b64(64)"),
        ("b64_shl64_int", "let _x = b64(1) << 64"),
        ("b64_shr_neg", "let _x = b64(1) >> (-1)"),
        ("b64_neg", "let _x = b64(-1)"),
        ("b64_ovf", "let _x = b64(0x1_0000_0000_0000_0000)"),
        ("t32_bad_lane0", "let _x = t32(b64(0b11))"),
        ("t32_bad_lane31", "let _x = t32(b64(0xC000_0000_0000_0000))"),
    ] {
        let src = format!("Dec main() -> int:\n    {expr}\n    return 0\n");
        assert_6mode_runtime_trap(&src, &tmp, tag);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_t9_checked_ternary_and_18bit_pack_unpack_6mode() {
    let tmp = temp_dir("ac04_t9");

    let pos = r#"
Dec main() -> int:
    let hi = t9(9841)
    let lo = t9(-9841)
    assert hi + lo == t9(0)
    assert -hi == lo and -lo == hi
    assert (t9(-8) // t9(3)) == t9(-3) and (t9(-8) % t9(3)) == t9(1)
    assert (t9(6) / t9(9)) == 2/3r
    let p_hi = pack(hi)
    let p_lo = b64(lo)
    assert unpack_t9(p_hi) == hi
    assert unpack_t9(p_lo) == lo
    emit hi
    emit lo
    emit p_hi
    emit p_lo
    return 0
"#;
    assert_6mode_source_parity(
        pos,
        "t9(9841)\nt9(-9841)\n87381\nb64(0x000000000002aaaa)\n0\n",
        &tmp,
        "t9_pos",
    );

    for (tag, expr) in [
        ("t9_hi_ovf", "let _x = t9(9842)"),
        ("t9_lo_ovf", "let _x = t9(-9842)"),
        ("t9_add_ovf", "let _x = t9(9841) + t9(1)"),
        ("t9_sub_ovf", "let _x = t9(-9841) - t9(1)"),
        ("t9_mul_ovf", "let _x = t9(200) * t9(200)"),
        ("t9_div0", "let _x = t9(5) // t9(0)"),
        ("t9_rem0", "let _x = t9(5) % t9(0)"),
        ("t9_unpack_oob", "let _x = unpack_t9(262144)"),
        ("t9_unpack_neg", "let _x = unpack_t9(-1)"),
        ("t9_unpack_bad0", "let _x = unpack_t9(3)"),
        ("t9_unpack_bad8", "let _x = unpack_t9(b64(0x30000))"),
    ] {
        let src = format!("Dec main() -> int:\n    {expr}\n    return 0\n");
        assert_6mode_runtime_trap(&src, &tmp, tag);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac05_q4_superpose_cancel_and_eager_select_parity_and_firewalls() {
    let tmp = temp_dir("ac05_select");

    // Compile-time firewalls on `select`
    struct S;
    let _ = S;
    assert_eq!(
        check_source(
            "Dec main() -> int:\n    let _x = select(true, 1, u8(1))\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    assert_eq!(
        check_source(
            "struct Boxed { v: int }\nDec f(ro: ReadOnly<Boxed>, m: Boxed) -> int:\n    let _x = select(true, ro, m)\n    return 0\nDec main() -> int:\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    assert_eq!(
        check_source(
            "Dec nop() -> ():\n    return\nDec main() -> int:\n    select(true, nop(), nop())\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::TypeError
    );
    assert_eq!(
        check_source(
            "use aod::qext::{ext_decimal}\nquarantine Q_ext LeakMod:\n    Dec leak_cond(a: int, b: int) -> QExt<ratio>:\n        let q = ext_decimal(\"1.0\")\n        let c = q == q\n        let _bad = select(c, a, b)\n        return q\nDec main() -> int:\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // Full 9-pair `(t1, t1)` truth table + multi-lane `t32` constructive/destructive `superpose_cancel`
    let sup9 = r#"
Dec main() -> int:
    assert superpose_cancel(0t, 0t) == 0t
    assert superpose_cancel(0t, +1t) == +1t
    assert superpose_cancel(0t, -1t) == -1t
    assert superpose_cancel(+1t, 0t) == +1t
    assert superpose_cancel(+1t, +1t) == +1t
    assert superpose_cancel(+1t, -1t) == 0t
    assert superpose_cancel(-1t, 0t) == -1t
    assert superpose_cancel(-1t, +1t) == 0t
    assert superpose_cancel(-1t, -1t) == -1t
    emit superpose_cancel(t32(1 + 3 - 9), t32(-1 + 3 - 9))
    return 0
"#;
    assert_6mode_source_parity(sup9, "t32(-6)\n0\n", &tmp, "sup9_pos");

    // Eager left-to-right semantics: BOTH `select(false, 10 // 0, 0)` and `select(true, 0, 10 // 0)` MUST trap!
    assert_6mode_runtime_trap(
        "Dec main() -> int:\n    let x = select(false, 10 // 0, 0)\n    return x\n",
        &tmp,
        "select_eager_left_trap",
    );
    assert_6mode_runtime_trap(
        "Dec main() -> int:\n    let x = select(true, 0, 10 // 0)\n    return x\n",
        &tmp,
        "select_eager_right_trap",
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac06_bytes_escapes_immutability_nist_sha256_and_6mode_traps() {
    let tmp = temp_dir("ac06_bytes");

    // Lex rejections: non-ASCII, \uXXXX, invalid \x, incomplete \x
    for bad in [
        "b\"caf\u{00e9}\"",
        "b\"\\u0041\"",
        "b\"\\xZZ\"",
        "b\"\\x0\"",
        "b\"\\q\"",
    ] {
        let src = format!("Dec main() -> int:\n    let _b = {bad}\n    return 0\n");
        let err = check_source(&src, CompatMode::V2).unwrap_err();
        assert_eq!(err.code, DiagnosticCode::LexError, "for {bad}: {err}");
    }

    // str == bytes -> ImplicitCoercionForbidden; b[0] = u8(1) -> TypeError
    assert_eq!(
        check_source(
            "Dec main() -> int:\n    let _ok = \"abc\" == b\"abc\"\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    assert_eq!(
        check_source(
            "Dec main() -> int:\n    mut b = b\"abc\"\n    b[0] = u8(1)\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::TypeError
    );

    // NIST SHA-256 empty "" and "abc" + single-quoted b'...' + empty bytes([])
    let nist = r#"
Dec main() -> int:
    let empty_a = b''
    let empty_b = bytes([])
    assert empty_a == empty_b and len(empty_a) == 0
    let h0 = hex_bytes(sha256_bytes(empty_a))
    let h1 = hex_bytes(sha256_bytes(b"abc"))
    emit h0
    emit h1
    emit b"\0\t\r\n\\\"'\xff"
    return 0
"#;
    assert_6mode_source_parity(
        nist,
        "b\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"\nb\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"\nb\"\\0\\t\\r\\n\\\\\\\"'\\xff\"\n0\n",
        &tmp,
        "bytes_nist",
    );

    for (tag, body) in [
        ("bytes_idx_neg", "let b = b\"abc\"\n    let _x = b[-1]"),
        ("bytes_idx_oob", "let b = b\"abc\"\n    let _x = b[3]"),
        ("slice_neg", "let _x = slice_bytes(b\"abc\", -1, 2)"),
        ("slice_rev", "let _x = slice_bytes(b\"abc\", 2, 1)"),
        ("slice_oob", "let _x = slice_bytes(b\"abc\", 0, 4)"),
    ] {
        let src = format!("Dec main() -> int:\n    {body}\n    return 0\n");
        assert_6mode_runtime_trap(&src, &tmp, tag);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac07_fixed_array_bounds_readonly_firewall_and_6mode_traps() {
    let tmp = temp_dir("ac07_array");

    // N == 0, N < 0, N > 65536, length mismatch, heterogeneous, array == list, direct arr[i] = v
    for (bad, expected_code) in [
        (
            "Dec main() -> int:\n    let a: array[u8, 0] = [u8(1)]\n    return 0\n",
            DiagnosticCode::TypeError,
        ),
        (
            "Dec main() -> int:\n    let a: array[u8, 65537] = [u8(1)]\n    return 0\n",
            DiagnosticCode::TypeError,
        ),
        (
            "Dec main() -> int:\n    let a = array()\n    return 0\n",
            DiagnosticCode::TypeError,
        ),
        (
            "Dec main() -> int:\n    let a: array[u8, 3] = [u8(1), u8(2)]\n    return 0\n",
            DiagnosticCode::ImplicitCoercionForbidden,
        ),
        (
            "Dec main() -> int:\n    let a = array(1, u8(2))\n    return 0\n",
            DiagnosticCode::ImplicitCoercionForbidden,
        ),
        (
            "Dec main() -> int:\n    let a: array[int, 2] = [1, 2]\n    let l: [int] = [1, 2]\n    let _eq = a == l\n    return 0\n",
            DiagnosticCode::ImplicitCoercionForbidden,
        ),
        (
            "Dec main() -> int:\n    mut a: array[int, 2] = [1, 2]\n    a[0] = 9\n    return 0\n",
            DiagnosticCode::TypeError,
        ),
    ] {
        let err = check_source(bad, CompatMode::V2)
            .expect_err(&format!("expected error for:\n{bad}"));
        assert_eq!(err.code, expected_code, "got {err} for:\n{bad}");
    }

    // ReadOnly<array[T, N]> firewall: array_set on ReadOnly<array> and mutating ReadOnly<array[Cell, 2]>[0]
    let ro_array_set = r#"
Dec bad(ro: ReadOnly<array[int, 2]>) -> array[int, 2]:
    return array_set(ro, 0, 99)

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ro_array_set, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    let ro_nested_elem = r#"
struct Cell:
    v: int

Dec mutate_cell(mut_c: Cell) -> int:
    return mut_c.v

Dec bad(ro: ReadOnly<array[Cell, 1]>) -> int:
    return mutate_cell(ro[0])

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(ro_nested_elem, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ObserverMutationForbidden
    );

    // 6-mode out-of-bounds traps on array indexing and array_set
    for (tag, body) in [
        ("arr_idx_neg", "let a = array(10, 20)\n    let _x = a[-1]"),
        ("arr_idx_oob", "let a = array(10, 20)\n    let _x = a[2]"),
        (
            "arr_set_neg",
            "let a = array(10, 20)\n    let _b = array_set(a, -1, 5)",
        ),
        (
            "arr_set_oob",
            "let a = array(10, 20)\n    let _b = array_set(a, 2, 5)",
        ),
    ] {
        let src = format!("Dec main() -> int:\n    {body}\n    return 0\n");
        assert_6mode_runtime_trap(&src, &tmp, tag);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_ac13_hw_register_primitives_6mode_parity() {
    let root = repo_root();
    let src_path = root.join("tests/fixtures/v2_smoke/hw_register_primitives.dec");
    let expected = fs::read_to_string(
        root.join("tests/fixtures/v2_smoke/hw_register_primitives.stdout"),
    )
    .unwrap();
    let tmp = temp_dir("ac08_smoke");
    assert_6mode_file_parity(&src_path, &expected, &tmp, "hw_register_primitives");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_first_principles_derivation_sha256_and_readme_link() {
    use aod_core::sha256::Sha256;

    let root = repo_root();
    let fp_bytes = fs::read(root.join("docs/FIRST_PRINCIPLES_DERIVATION.md")).unwrap();
    assert_eq!(
        Sha256::digest_hex(&fp_bytes),
        "091d8f1ef7dd3b9c1f96eab7adc9ff5db67194418c2dbfad1a0fcab68b5afabc"
    );
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains("docs/FIRST_PRINCIPLES_DERIVATION.md"));
}

#[test]
fn ac10_live_walk_anchor_invariance_and_all_11_manual_examples_pass() {
    // 1. Initial anchor `00_8` and 2-digit minimum octal width (`07_8`, `10_8`)
    let empty_walk = lex_live_walk(b"");
    assert_eq!(empty_walk.initial_anchor.render(), "00_8");
    assert_eq!(empty_walk.final_anchor.render(), "00_8");
    assert_eq!(empty_walk.committed_crossings, 0);
    assert_eq!(empty_walk.dwell_burned, 0);

    let seven = lex_live_walk(b"a b c d e f g");
    assert_eq!(seven.committed_crossings, 7);
    assert_eq!(seven.final_anchor.render(), "07_8");

    let eight = lex_live_walk(b"a b c d e f g h");
    assert_eq!(eight.committed_crossings, 8);
    assert_eq!(eight.final_anchor.render(), "10_8");

    // 2. Monotonic dwell accumulation & final_anchor invariance across compact vs padded/commented `.dec`
    let compact = b"Dec f(x: int) -> int:\n return x + 1\n";
    let padded = b"# Leading header comment\n\nDec    f (  x :   int  )   ->   int :\n    # inner comment -> ignored in dwell\n    return   x   +   1\n\n";
    let w_compact = lex_live_walk(compact);
    let w_padded = lex_live_walk(padded);
    assert_eq!(
        w_compact.final_anchor, w_padded.final_anchor,
        "whitespace/comment padding must preserve final_anchor"
    );
    assert_eq!(w_compact.committed_crossings, w_padded.committed_crossings);
    assert!(
        w_padded.dwell_burned > w_compact.dwell_burned,
        "padded source must burn strictly more monotonic dwell"
    );

    // 3. CLI `aodc audit-dec --json` passes 6/6 hooks on all 11 positive `manual_examples/01..11.dec`
    let manual_dir = repo_root().join("manual_examples");
    let mut files: Vec<PathBuf> = fs::read_dir(&manual_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dec"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 11);

    for f in &files {
        let out = Command::new(aodc_bin())
            .args(["audit-dec", "--json"])
            .arg(f)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "audit-dec failed on {}: {}",
            f.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        let json_str = String::from_utf8(out.stdout).unwrap();
        let parsed = parse_strict_json(&json_str).unwrap();
        validate_portable_report(&parsed, "$")
            .unwrap_or_else(|e| panic!("non-portable audit report on {}: {e}", f.display()));
        let JsonValue::Object(map) = parsed else {
            panic!("expected JSON object");
        };
        assert_eq!(
            map.get("schema"),
            Some(&JsonValue::from_str_val(DEC_AUDIT_SCHEMA))
        );
        assert_eq!(map.get("status"), Some(&JsonValue::from_str_val("PASS")));
        assert_eq!(map.get("hooks_passed"), Some(&JsonValue::from_i64(6)));
    }
}

#[test]
fn ac11_audit_dec_6_hostile_hooks_and_cli_exit2_report_guards() {
    let tmp = temp_dir("ac11_audit");

    // 6 distinct hostile `.dec` programs individually falsifying each §5.17 audit hook (`AC-11`)!
    let hostile_cases: [(&str, &str, &str); 6] = [
        (
            "hook1_missing_hinge",
            "spine_audit",
            "Dec missing_arrow():\n    emit 1\n",
        ),
        (
            "hook2_degenerate_window",
            "boundary_audit",
            "Dec main() -> int:\n    let _w = (6, 6]\n    return 0\n",
        ),
        (
            "hook3_negative_sadar_c3",
            "sadar_audit",
            "Dec main() -> int:\n    let _s = sadar_promote(0, -1, 1/2r)\n    return 0\n",
        ),
        (
            "hook4_odd_sheddic_enclosure",
            "sheddic_path_audit",
            "Dec main() -> int:\n    let _ok = audit_sheddic_route(3, u8(7))\n    return 0\n",
        ),
        (
            "hook5_cycle_unclosed_or_zero_rd",
            "cycle_audit",
            "Dec main() -> int:\n    let _r = support_rd(3:3:6, 0)\n    let _rt = rate(trace(\"s\", \"L\", q4(0)))\n    return 0\n",
        ),
        (
            "hook6_frg_identity_broken",
            "frg_audit",
            "Dec main() -> int:\n    let _ok = verify_frg_cross_return(100, 112, 9)\n    return 0\n",
        ),
    ];

    for (name, failing_hook, src) in hostile_cases {
        let rep = audit_dec_source(src, CompatMode::V2);
        assert_eq!(rep.status, "FAIL", "{name} must FAIL");

        let fpath = tmp.join(format!("{name}.dec"));
        let rpath = tmp.join(format!("{name}.report.json"));
        fs::write(&fpath, src).unwrap();

        let out = Command::new(aodc_bin())
            .args(["audit-dec", "--json", "--report"])
            .arg(&rpath)
            .arg(&fpath)
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{name}: expected exit code 1, got {:?}",
            out.status.code()
        );
        let parsed = parse_strict_json(&String::from_utf8(out.stdout).unwrap()).unwrap();
        validate_portable_report(&parsed, "$").unwrap();
        let file_json = parse_strict_json(&fs::read_to_string(&rpath).unwrap()).unwrap();
        assert_eq!(parsed, file_json);

        let JsonValue::Object(root) = parsed else {
            panic!("expected object");
        };
        assert_eq!(root.get("status"), Some(&JsonValue::from_str_val("FAIL")));
        let Some(JsonValue::Object(hooks)) = root.get("hooks") else {
            panic!("missing hooks");
        };
        let Some(JsonValue::Object(h_obj)) = hooks.get(failing_hook) else {
            panic!("missing hook {failing_hook}");
        };
        assert_eq!(
            h_obj.get("status"),
            Some(&JsonValue::from_str_val("FAIL")),
            "{name}: hook {failing_hook} must have status FAIL"
        );
    }

    // Also verify empty/pass-only body fails `spine_audit` cleanly
    let pass_only = audit_dec_source("Dec noop() -> ():\n    pass\n", CompatMode::V2);
    assert!(!pass_only.spine_audit.passed());

    // CLI usage & pre-execution `--report` guards MUST exit 2 BEFORE reading `<source.dec>`!
    let valid_dec = repo_root().join("manual_examples/01_dec_ledger.dec");

    let bad_flag = Command::new(aodc_bin())
        .args(["audit-dec", "--bogus"])
        .arg(&valid_dec)
        .output()
        .unwrap();
    assert_eq!(bad_flag.status.code(), Some(2));

    let no_src = Command::new(aodc_bin()).arg("audit-dec").output().unwrap();
    assert_eq!(no_src.status.code(), Some(2));

    let real_target = tmp.join("real.json");
    fs::write(&real_target, "{}").unwrap();
    let sym_report = tmp.join("sym_report.json");
    symlink(&real_target, &sym_report).unwrap();
    let nonexistent_src = tmp.join("does_not_exist.dec");
    let sym_out = Command::new(aodc_bin())
        .args(["audit-dec", "--report"])
        .arg(&sym_report)
        .arg(&nonexistent_src)
        .output()
        .unwrap();
    assert_eq!(sym_out.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&real_target).unwrap(), "{}");

    let forbidden_in_repo = repo_root().join("crates/forbidden_pr10_audit_report.json");
    let repo_out = Command::new(aodc_bin())
        .args(["audit-dec", "--report"])
        .arg(&forbidden_in_repo)
        .arg(&valid_dec)
        .output()
        .unwrap();
    assert_eq!(repo_out.status.code(), Some(2));
    assert!(!forbidden_in_repo.exists());

    assert!(verify_sheddic_route(0, 1));
    assert!(verify_sheddic_route(3, 2));
    assert!(!verify_sheddic_route(3, 0));
    assert!(!verify_sheddic_route(3, 7));
    assert!(verify_frg_identity(65536, 65600, 64));
    assert!(!verify_frg_identity(65536, 65600, 63));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac12_frg_ring_gate_probe_and_3state_op_e_certification() {
    // 1. Verify all 3 `retained: Option<i64>` states (`Some(r > 0)`, `Some(0)`, `None` `S_k^{+,-e}`)
    let active = certify_frg_channel("C8", 8, 137, 274, 411, Some(137), 4).unwrap();
    assert!(active.retained_present);
    assert_eq!(active.retained, Some(137));
    assert_eq!(active.r_e, 137);
    assert!(active.identity_verified);
    assert!(active.op_certified);
    assert_eq!(active.role_trit, 1);
    assert_eq!(active.w_adapted, 141);

    let zero_retained = certify_frg_channel("C8", 8, 137, 274, 274, Some(0), 4).unwrap();
    assert!(zero_retained.retained_present);
    assert_eq!(zero_retained.retained, Some(0));
    assert_eq!(zero_retained.r_e, 0);
    assert!(zero_retained.identity_verified);
    assert!(
        !zero_retained.op_certified,
        "Some(0) must yield op_certified == false"
    );
    assert_eq!(zero_retained.role_trit, 0);
    assert_eq!(zero_retained.w_adapted, 4);

    let removed_src = certify_frg_channel("C8", 8, 137, 274, 274, None, 4).unwrap();
    assert!(!removed_src.retained_present);
    assert_eq!(removed_src.retained, None);
    assert!(!removed_src.op_certified);
    assert_eq!(removed_src.role_trit, -1);
    assert_eq!(removed_src.w_adapted, 0);

    // 2. Live 3-tier hardware execution (`E4 RD=9`, `C6 RD=61`, `C8 RD=137 T_sync=8357`)
    let live = run_probe_frg_ring_gate().unwrap();
    assert_eq!(
        live.policy_scope,
        "COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW"
    );
    assert_eq!(live.tier0_e4.rd_cadence, 9);
    assert!(live.tier0_e4.verified && live.tier0_e4.op_certified);
    assert_eq!(live.tier1_c6.rd_cadence, 61);
    assert!(live.tier1_c6.verified && live.tier1_c6.op_certified);
    assert_eq!(live.tier2_c8.rd_cadence, 137);
    assert_eq!(live.t_sync, 8357);
    assert_eq!(live.s_gate_a, 137);
    assert_eq!(live.s_gate_b, 137);
    assert_eq!(live.t_zero, 274);
    assert!(live.c_cross >= 137);
    assert_eq!(live.t_cross - live.t_zero, live.r_e);
    assert_eq!(live.r_e, live.c_cross);
    assert!(live.identity_verified && live.op_certified && live.verified);

    let full = run_all_hw_probes().unwrap();
    let json = full.to_json();
    let JsonValue::Object(map) = json else {
        panic!("expected object");
    };
    assert!(map.contains_key("frg_ring_gate"));
}

#[test]
fn ac14_repository_zero_float_unsafe_allowlist_and_no_clamping_guard() {
    let root = repo_root();

    let allowlisted_unsafe: [&str; 5] = [
        "crates/aod-rt/src/abi.rs",
        "crates/aod-llvm/src/ffi.rs",
        "crates/aod-llvm/src/aot.rs",
        "crates/aod-llvm/src/jit.rs",
        "crates/aod-llvm/src/hw_probe.rs",
    ];

    let mut rs_files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut rs_files);
    assert!(!rs_files.is_empty());

    let f_32 = ["f", "32"].concat();
    let f_64 = ["f", "64"].concat();

    for path in &rs_files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if !rel.contains("/src/") {
            continue;
        }
        let text = fs::read_to_string(path).unwrap();
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            assert!(
                !contains_ident_token(trimmed, &f_32) && !contains_ident_token(trimmed, &f_64),
                "forbidden float token on {rel}:{}: {trimmed}",
                idx + 1
            );
            if !allowlisted_unsafe.contains(&rel.as_str()) && !trimmed.starts_with("#![forbid(") {
                assert!(
                    !contains_ident_token(trimmed, "unsafe"),
                    "forbidden unsafe token outside 5-file allowlist on {rel}:{}: {trimmed}",
                    idx + 1
                );
            }
        }
    }

    let dec_audit_src = fs::read_to_string(root.join("crates/aodc/src/dec_audit.rs")).unwrap();
    assert!(dec_audit_src.starts_with("#![forbid(unsafe_code)]"));

    let hw_probe_src = fs::read_to_string(root.join("crates/aod-llvm/src/hw_probe.rs")).unwrap();
    let frg_tail = hw_probe_src
        .split("pub fn certify_frg_channel")
        .nth(1)
        .expect("certify_frg_channel must exist in hw_probe.rs");
    for line in frg_tail.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        assert!(
            !trimmed.contains(".max(1)"),
            "FRG probe / certification executable code must have zero `.max(1)` clamping: {trimmed}"
        );
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

fn contains_ident_token(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() || bytes.len() < nb.len() {
        return false;
    }
    for i in 0..=(bytes.len() - nb.len()) {
        if &bytes[i..i + nb.len()] == nb {
            let before_ok =
                i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let after_ok = i + nb.len() == bytes.len()
                || !(bytes[i + nb.len()].is_ascii_alphanumeric() || bytes[i + nb.len()] == b'_');
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}
