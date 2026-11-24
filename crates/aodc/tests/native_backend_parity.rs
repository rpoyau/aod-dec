use std::path::{Path, PathBuf};
use std::process::Command;

use aod_llvm::verify_and_optimize_ir;
use aodc::diagnostics::CompatMode;
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::{
    build_aot_file, build_aot_source, emit_llvm_file, jit_file_captured, jit_source_captured,
    run_file, run_source, OptLevel,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aodc_native_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn assert_5mode_file_parity(path: &Path, compat: CompatMode, expected_stdout: &str, tmp: &Path, stem: &str) {
    // 1. Exact VM
    let vm = run_file(path, compat)
        .unwrap_or_else(|e| panic!("VM failed on {}: {e}", path.display()))
        .to_stdout_string();
    assert_eq!(vm, expected_stdout, "VM stdout mismatch on {}", path.display());

    // 2 & 3. AOT -O0 and AOT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_{tag}"));
        build_aot_file(path, compat, opt, &exe)
            .unwrap_or_else(|e| panic!("AOT -{tag} build failed on {}: {e}", path.display()));
        let out = Command::new(&exe).output().unwrap();
        assert!(
            out.status.success(),
            "AOT -{tag} exec failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected_stdout,
            "AOT -{tag} stdout mismatch on {}",
            path.display()
        );
    }

    // 4 & 5. ORC LLJIT -O0 and ORC LLJIT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let (stdout, res) = jit_file_captured(path, compat, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
            .unwrap_or_else(|e| panic!("JIT -{tag} compile failed on {}: {e}", path.display()));
        res.unwrap_or_else(|e| panic!("JIT -{tag} runtime failed on {}: {e}", path.display()));
        assert_eq!(
            stdout,
            expected_stdout,
            "JIT -{tag} stdout mismatch on {}",
            path.display()
        );
    }
}

#[test]
fn ac04_llvm18_verifier_and_malformed_ir_rejection() {
    let valid = "define i32 @f(i32 %x) {\nentry:\n  %y = add i32 %x, 1\n  ret i32 %y\n}\n";
    for opt in [OptLevel::O0, OptLevel::O1, OptLevel::O2, OptLevel::O3] {
        let out = verify_and_optimize_ir(valid, opt).unwrap();
        assert!(out.contains("define i32 @f"), "missing @f at {opt:?}");
    }

    // Malformed syntax returns clean Err without aborting process!
    assert!(verify_and_optimize_ir("define broken {{{", OptLevel::O0).is_err());

    // Syntactically parseable but verifier-invalid IR (missing return terminator) returns clean Err (action=2)!
    let invalid_verifier = "define i32 @bad() {\nentry:\n  %x = add i32 1, 2\n}\n";
    let err = verify_and_optimize_ir(invalid_verifier, OptLevel::O0).unwrap_err();
    assert!(
        err.message.contains("verification") || err.message.contains("parse"),
        "unexpected err: {err}"
    );
}

#[test]
fn ac05_emit_llvm_o2_unboxed_hot_loop_verification() {
    let bench = repo_root().join("tests/fixtures/v2_bench/unboxed_loop_bench.dec");
    let ir_o2 = emit_llvm_file(&bench, CompatMode::V2, OptLevel::O2).unwrap();

    // Unboxed overflow-checked i64 arithmetic + binary Stein GCD present
    assert!(
        ir_o2.contains("@llvm.sadd.with.overflow.i64"),
        "expected @llvm.sadd.with.overflow.i64 in -O2 IR"
    );
    assert!(
        ir_o2.contains("@llvm.cttz.i128"),
        "expected @llvm.cttz.i128 binary Stein GCD in -O2 IR"
    );

    // P0-1 guarantee: ZERO i128 division/remainder instructions (no __udivti3/__umodti3 traps!)
    assert!(!ir_o2.contains("urem i128"), "forbidden urem i128 in IR");
    assert!(!ir_o2.contains("udiv i128"), "forbidden udiv i128 in IR");
    assert!(!ir_o2.contains("sdiv i128"), "forbidden sdiv i128 in IR");
    assert!(!ir_o2.contains("srem i128"), "forbidden srem i128 in IR");

    // Hot-path arithmetic (`% 17`, `t32(...)`, `- 9`, `ratio(..., 8)`) completely eliminated from runtime calls
    assert!(!ir_o2.contains("call void @aod_rt_int_rem_floor"));
    assert!(!ir_o2.contains("call void @aod_rt_int_sub"));
    assert!(!ir_o2.contains("call i64 @aod_rt_int_to_t32"));
}

#[test]
fn ac06_all_10_frozen_v1_examples_parity_vm_aot_jit() {
    let v1_dir = repo_root().join("tests/fixtures/v1_examples");
    let manifest = std::fs::read_to_string(v1_dir.join("MANIFEST.txt")).unwrap();
    let tmp = temp_dir("v1_parity");

    let mut count = 0usize;
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let name = line.split_whitespace().next().unwrap();
        let src_path = v1_dir.join(name);
        let expected = std::fs::read_to_string(src_path.with_extension("stdout")).unwrap();
        let stem = name.strip_suffix(".dec").unwrap();
        assert_5mode_file_parity(&src_path, CompatMode::V1, &expected, &tmp, stem);
        count += 1;
    }
    assert_eq!(count, 10);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac07_all_3_v2_smoke_fixtures_parity_vm_aot_jit() {
    let smoke_dir = repo_root().join("tests/fixtures/v2_smoke");
    let tmp = temp_dir("v2_smoke_parity");

    for name in [
        "opposing_336_chamber.dec",
        "m3_wavelet_recipient.dec",
        "qext_post_freeze.dec",
        "hw_register_primitives.dec",
    ] {
        let path = smoke_dir.join(name);
        let expected = run_file(&path, CompatMode::V2).unwrap().to_stdout_string();
        let stem = name.strip_suffix(".dec").unwrap();
        assert_5mode_file_parity(&path, CompatMode::V2, &expected, &tmp, stem);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_full_dec_v2_language_and_deep_copy_aliasing_parity() {
    let src = r#"
struct Node { id: int, weight: ratio }
enum Route { Closed, Exo(int), Open }

Dec classify(r: Route) -> int:
    match r:
        Route::Closed:
            return 0
        Route::Exo(k) if k > 5:
            return k * 10
        Route::Exo(k):
            return k
        Route::Open:
            return -1

Dec pat_dispatch(r, x) -> out:
    (Route::Closed, 0) -> int out:
        out = 77
    (Route::Exo(k), int x) if k > 0 -> int out:
        out = k + x
    (_, int x) -> int out:
        out = -x

Dec main() -> int:
    # P0-3 Deep-copy / copy-on-write aliasing verification for list, map, and struct!
    mut a = [1, 2, 3]
    mut b = a
    b[0] = 99
    assert a[0] == 1 and b[0] == 99

    mut m1: map[str, int] = {"zeta": 3, "alpha": 1}
    mut m2 = m1
    m2["alpha"] = 88
    assert m1["alpha"] == 1 and m2["alpha"] == 88
    assert len(m1) == 2 and contains(m1, "zeta")

    mut s1 = Node { id: 10, weight: 1/2r }
    mut s2 = s1
    s2.id = 55
    assert s1.id == 10 and s2.id == 55

    # Enum + match + multi-arm pattern dispatch + short-circuit `and`/`or`
    assert classify(Route::Closed) == 0
    assert classify(Route::Exo(3)) == 3
    assert classify(Route::Exo(8)) == 80
    assert pat_dispatch(Route::Closed, 0) == 77
    assert pat_dispatch(Route::Exo(4), 6) == 10
    assert pat_dispatch(Route::Open, 5) == -5

    # Short-circuit guard: RHS division by zero must NOT execute!
    let sc1 = (false and (1 // 0 == 0))
    let sc2 = (true or (1 // 0 == 0))
    assert sc1 == false and sc2 == true

    for (k, v) in m1:
        emit k
        emit v
    emit a[0]
    emit b[0]
    emit s1.id
    emit s2.id
    return 0
"#;
    let expected = run_source(src, CompatMode::V2).unwrap().to_stdout_string();
    assert_eq!(expected, "alpha\n1\nzeta\n3\n1\n99\n10\n55\n0\n");

    let tmp = temp_dir("v2_lang_parity");
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("lang_{tag}"));
        build_aot_source(src, CompatMode::V2, opt, &exe).unwrap();
        let out = Command::new(&exe).output().unwrap();
        assert!(out.status.success(), "AOT -{tag} stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected);

        let (jout, jres) = jit_source_captured(src, CompatMode::V2, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).unwrap();
        jres.unwrap();
        assert_eq!(jout, expected);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_unannotated_aggregate_and_imported_consts_parity() {
    let tmp = temp_dir("v2_agg_consts");

    let helper_mod = tmp.join("shared_consts.dec");
    std::fs::write(
        &helper_mod,
        r#"
struct Pt { x: int, y: int }
enum Kind { Zero, Tag(int) }

const MOD_LIST = [10, 20, 30]
const MOD_PT = Pt { x: 4, y: 5 }
const MOD_MAP = {"b": 2, "a": 1}
"#,
    )
    .unwrap();

    let main_file = tmp.join("main.dec");
    std::fs::write(
        &main_file,
        r#"
use shared_consts::{Pt, Kind, MOD_LIST, MOD_PT, MOD_MAP}

const C_LIST = [1, 2, 3]
const C_PT = Pt { x: 7, y: 8 }
const C_ENUM = Kind::Tag(42)
const C_MAP = {"z": 9, "a": 3}
const C_WIN = (0, 6]
const C_BOX = box([0..4, 0..4, 0..4])
const C_TUP = (1, "hi")
const C_ALIAS_LIST = MOD_LIST

Dec main() -> int:
    emit C_LIST[1]
    emit C_PT.x + C_PT.y
    match C_ENUM:
        Kind::Zero:
            emit 0
        Kind::Tag(v):
            emit v
    for (k, v) in C_MAP:
        emit k
        emit v
    emit C_WIN.len
    emit ∂C_BOX
    emit C_TUP
    emit MOD_PT.x
    emit MOD_MAP["a"]
    emit C_ALIAS_LIST[2]
    return 0
"#,
    )
    .unwrap();

    let expected = run_file(&main_file, CompatMode::V2).unwrap().to_stdout_string();
    assert_5mode_file_parity(&main_file, CompatMode::V2, &expected, &tmp, "agg_consts");
    let _ = std::fs::remove_dir_all(&tmp);
}
