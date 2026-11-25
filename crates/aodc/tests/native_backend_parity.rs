use std::path::{Path, PathBuf};
use std::process::Command;

use aod_llvm::verify_and_optimize_ir;
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

fn assert_5mode_file_parity(path: &Path, expected_stdout: &str, tmp: &Path, stem: &str) {
    // 1. Exact VM
    let vm = run_file(path)
        .unwrap_or_else(|e| panic!("VM failed on {}: {e}", path.display()))
        .to_stdout_string();
    assert_eq!(vm, expected_stdout, "VM stdout mismatch on {}", path.display());

    // 2 & 3. AOT -O0 and AOT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_{tag}"));
        build_aot_file(path, opt, &exe)
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
        let (stdout, res) = jit_file_captured(path, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
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
fn ac05_emit_llvm_o2_trit_boxed_swar_hot_loop_verification() {
    let bench = repo_root().join("tests/fixtures/v2_bench/unboxed_loop_bench.dec");
    let ir_o2 = emit_llvm_file(&bench, OptLevel::O2).unwrap();

    // Trit-Boxed 2-bit balanced ternary LUT + 64-bit Stein GCD present
    assert!(
        ir_o2.contains("@.pack4_lut"),
        "expected @.pack4_lut 81-byte 4-trit packing LUT in -O2 IR"
    );
    assert!(
        ir_o2.contains("@llvm.cttz.i64"),
        "expected @llvm.cttz.i64 64-bit Stein GCD in -O2 IR"
    );

    // P0-1 guarantee: ZERO i128 division/remainder instructions (no __udivti3/__umodti3 traps!)
    assert!(!ir_o2.contains("urem i128"), "forbidden urem i128 in IR");
    assert!(!ir_o2.contains("udiv i128"), "forbidden udiv i128 in IR");
    assert!(!ir_o2.contains("sdiv i128"), "forbidden sdiv i128 in IR");
    assert!(!ir_o2.contains("srem i128"), "forbidden srem i128 in IR");
}

#[test]
fn ac06_all_10_clean_break_v3_examples_parity_vm_aot_jit() {
    let v3_dir = repo_root().join("tests/fixtures/v3_examples");
    let manifest = std::fs::read_to_string(v3_dir.join("MANIFEST.txt")).unwrap();
    let tmp = temp_dir("v3_parity");

    let mut count = 0usize;
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let name = line.split_whitespace().next().unwrap();
        let src_path = v3_dir.join(name);
        let expected = std::fs::read_to_string(src_path.with_extension("stdout")).unwrap();
        let stem = name.strip_suffix(".dec").unwrap();
        assert_5mode_file_parity(&src_path, &expected, &tmp, stem);
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
    ] {
        let path = smoke_dir.join(name);
        let expected = run_file(&path).unwrap().to_stdout_string();
        let stem = name.strip_suffix(".dec").unwrap();
        assert_5mode_file_parity(&path, &expected, &tmp, stem);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_full_clean_break_language_rank_adverbs_and_trit_boxed_parity() {
    let src = r#"
struct Node:
    id: int
    weight: ratio

enum Route:
    Closed
    Exo(int)
    Open

dec classify(r: Route) -> int:
    match r:
        Route::Closed -> 0
        Route::Exo(k) where k > 5 -> k * 10
        Route::Exo(k) -> k
        Route::Open -> -1

dec pat_dispatch:
    (Route::Closed, 0: int) -> int:
        77
    (Route::Exo(k), x: int) where k > 0 -> int:
        k + x
    (_: Route, x: int) -> int:
        -x

dec emit_kv(kv: (str, int)) -> int:
    let (k, v) = kv
    emit k
    emit v
    0

dec main() -> int:
    # Functional copy-on-write value semantics for list, map, and struct
    let a = [1, 2, 3]
    let b = set_at(a, 0, 99)
    assert a[0] == 1 and b[0] == 99

    let m1: map[str, int] = map["zeta": 3, "alpha": 1]
    let m2 = map_put(m1, "alpha", 88)
    assert m1["alpha"] == 1 and m2["alpha"] == 88
    assert len(m1) == 2 and contains(m1, "zeta")

    let s1 = Node(10, 1/2r)
    let s2 = Node(55, s1.weight)
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

    # Trit-Boxed inline boundary, BigTrit spill, compact pack_ratio, rank-lifting & fusion
    let max_t32 = 926510094425920
    let big = (max_t32 + 5) * 3 - 15
    assert big // 3 == max_t32
    let pr = pack_ratio(-11/27r)
    assert unpack_ratio(pr) == -11/27r
    assert (([true, false, true, true] ?> [10, 20, 30, 40]) |> +/) == 80
    assert ((0..10) |> +/) == 45
    assert ([1, 2, 3] +: [10, 20]) == [[11, 21], [12, 22], [13, 23]]

    let _ = each(map_entries(m1), emit_kv)
    emit a[0]
    emit b[0]
    emit s1.id
    emit s2.id
    0
"#;
    let expected = run_source(src).unwrap().to_stdout_string();
    assert_eq!(expected, "alpha\n1\nzeta\n3\n1\n99\n10\n55\n0\n");

    let tmp = temp_dir("v3_lang_parity");
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("lang_{tag}"));
        build_aot_source(src, opt, &exe).unwrap();
        let out = Command::new(&exe).output().unwrap();
        assert!(out.status.success(), "AOT -{tag} stderr: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected);

        let (jout, jres) = jit_source_captured(src, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).unwrap();
        jres.unwrap();
        assert_eq!(jout, expected);
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac08_unannotated_aggregate_and_imported_consts_parity() {
    let tmp = temp_dir("v3_agg_consts");

    let helper_mod = tmp.join("shared_consts.dec");
    std::fs::write(
        &helper_mod,
        r#"
struct Pt:
    x: int
    y: int

enum Kind:
    Zero
    Tag(int)

const MOD_LIST = [10, 20, 30]
const MOD_PT = Pt(4, 5)
const MOD_MAP = map["b": 2, "a": 1]
"#,
    )
    .unwrap();

    let main_file = tmp.join("main.dec");
    std::fs::write(
        &main_file,
        r#"
use shared_consts:
    Pt, Kind, MOD_LIST, MOD_PT, MOD_MAP

const C_LIST = [1, 2, 3]
const C_PT = Pt(7, 8)
const C_ENUM = Kind::Tag(42)
const C_MAP = map["z": 9, "a": 3]
const C_WIN = (0, 6]
const C_BOX = box([0..4, 0..4, 0..4])
const C_TUP = (1, "hi")
const C_ALIAS_LIST = MOD_LIST

dec emit_kv(kv: (str, int)) -> int:
    let (k, v) = kv
    emit k
    emit v
    0

dec main() -> int:
    emit C_LIST[1]
    emit C_PT.x + C_PT.y
    let ev = match C_ENUM:
        Kind::Zero -> 0
        Kind::Tag(v) -> v
    emit ev
    let _ = each(map_entries(C_MAP), emit_kv)
    emit C_WIN.len
    emit ∂C_BOX
    emit C_TUP
    emit MOD_PT.x
    emit MOD_MAP["a"]
    emit C_ALIAS_LIST[2]
    0
"#,
    )
    .unwrap();

    let expected = run_file(&main_file).unwrap().to_stdout_string();
    assert_5mode_file_parity(&main_file, &expected, &tmp, "agg_consts");
    let _ = std::fs::remove_dir_all(&tmp);
}
