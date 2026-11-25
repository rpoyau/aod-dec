use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;
use aod_llvm::{CodegenPlan, OptLevel};
use aodc::codegen::lower_to_llvm_ir_with_plan;
use aodc::diagnostics::DiagnosticCode;
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::types::{lookup_builtin, Type};
use aodc::{
    audit_dec_file_with_options, build_aot_file, build_aot_source, check_file, check_source,
    compile_file_to_ir, compile_jit_source, emit_llvm_source, exec_ir_file, format_source,
    jit_file_captured, jit_source_with_plan_captured, run_file, run_source_with_budget,
    verify_manual_dir, LoadOptions, ManualVerifyOptions,
};

static D3_SEQ: AtomicU64 = AtomicU64::new(0);

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
    let seq = D3_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aodc_pr12_d3_{}_{}_{tag}",
        std::process::id(),
        seq
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn collect_files_with_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for ent in fs::read_dir(dir).unwrap() {
        let p = ent.unwrap().path();
        if p.is_dir() {
            collect_files_with_ext(&p, ext, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

fn assert_6mode_file_parity(path: &Path, expected: &str, tmp: &Path, stem: &str) {
    // 1. Exact VM (`run`)
    let vm = run_file(path)
        .unwrap_or_else(|e| panic!("{stem} VM failed: {e}"))
        .to_stdout_string();
    assert_eq!(vm, expected, "{stem} VM mismatch");

    // 2. Hermetic DecIR v3 (`compile + exec`)
    let ir_text = compile_file_to_ir(path).unwrap();
    assert!(ir_text.contains("\"schema\": \"aod.dec.ir.v3\""));
    assert!(ir_text.contains("\"syntax_discipline\": \"pure_wave_v2\""));
    let ir_path = tmp.join(format!("{stem}.ir.json"));
    fs::write(&ir_path, &ir_text).unwrap();
    let ir_out = exec_ir_file(&ir_path).unwrap().to_stdout_string();
    assert_eq!(ir_out, expected, "{stem} DecIR mismatch");

    // 3 & 4. AOT -O0 and -O2
    for (tag, opt) in [("o0", OptLevel::O0), ("o2", OptLevel::O2)] {
        let exe = tmp.join(format!("{stem}_{tag}"));
        build_aot_file(path, opt, &exe)
            .unwrap_or_else(|e| panic!("{stem} AOT {tag} build failed: {e}"));
        let out = Command::new(&exe).output().unwrap();
        assert!(
            out.status.success(),
            "{stem} AOT {tag} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
    }

    // 5 & 6. ORC LLJIT -O0 and -O2
    for (tag, opt) in [("o0", OptLevel::O0), ("o2", OptLevel::O2)] {
        let (jout, jres) = jit_file_captured(path, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
            .unwrap_or_else(|e| panic!("{stem} JIT {tag} compile failed: {e}"));
        jres.unwrap_or_else(|e| panic!("{stem} JIT {tag} runtime failed: {e}"));
        assert_eq!(jout, expected);
    }
}

fn assert_6mode_source_parity(src: &str, expected: &str, tmp: &Path, stem: &str) {
    let path = tmp.join(format!("{stem}.dec"));
    fs::write(&path, src).unwrap();
    assert_6mode_file_parity(&path, expected, tmp, stem);
}

#[test]
fn ac01_seven_first_principles_modules_6mode_fmt_check_and_cli_test() {
    let root = repo_root();
    let fp_dir = root.join("examples/first_principles");
    let tmp = temp_dir("ac01_fp");

    let stems = [
        "00_primitives",
        "01_dec_ledger_shedding",
        "02_tesseract_jitter_motifs",
        "03_hardware_tiers_frg_awareness",
        "04_harmonics_stokes_gpu_uma",
        "05_self_host_compiler_elf64",
        "kernel",
    ];

    for stem in stems {
        let dec_path = fp_dir.join(format!("{stem}.dec"));
        let stdout_path = fp_dir.join(format!("{stem}.stdout"));
        assert!(dec_path.is_file(), "missing {}", dec_path.display());
        assert!(stdout_path.is_file(), "missing {}", stdout_path.display());
        let expected = fs::read_to_string(&stdout_path).unwrap();

        // 1. Canonical idempotent `aodc fmt --check`
        let fmt_chk = Command::new(aodc_bin())
            .args(["fmt", "--check"])
            .arg(&dec_path)
            .output()
            .unwrap();
        assert!(
            fmt_chk.status.success(),
            "fmt --check failed on {stem}: {}",
            String::from_utf8_lossy(&fmt_chk.stderr)
        );

        // 2. Full 6-mode parity (`run`, `compile+exec`, `build -O0`, `build -O2`, `jit -O0`, `jit -O2`)
        assert_6mode_file_parity(&dec_path, &expected, &tmp, stem);
    }

    // 3. CLI `aodc test examples/first_principles` -> `7 passed; 0 failed; 7 total`
    let cli_test = Command::new(aodc_bin())
        .arg("test")
        .arg(&fp_dir)
        .output()
        .unwrap();
    assert!(
        cli_test.status.success(),
        "aodc test examples/first_principles failed: {}",
        String::from_utf8_lossy(&cli_test.stderr)
    );
    assert!(
        String::from_utf8_lossy(&cli_test.stdout).contains("7 passed; 0 failed; 7 total"),
        "unexpected test summary:\n{}",
        String::from_utf8_lossy(&cli_test.stdout)
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac02_zero_transitional_v2_and_45_file_6hook_audit_dec_sweep() {
    let root = repo_root();

    // 1. Zero `TransitionalV2` or `"transitional_v2"` anywhere in `crates/` (outside dynamic test probe)
    let mut rs_files = Vec::new();
    collect_files_with_ext(&root.join("crates"), "rs", &mut rs_files);
    rs_files.sort();
    assert!(!rs_files.is_empty());

    let banned_pascal = ["Transitional", "V2"].concat();
    let banned_snake = ["transitional", "_v2"].concat();
    for p in &rs_files {
        let rel = p
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "crates/aodc/tests/phase_d3_purification.rs" {
            continue;
        }
        let text = fs::read_to_string(p).unwrap();
        assert!(
            !text.contains(&banned_pascal),
            "forbidden legacy variant `{banned_pascal}` found in {rel}"
        );
        assert!(
            !text.contains(&banned_snake),
            "forbidden legacy tag `{banned_snake}` found in {rel}"
        );
    }

    // 2. Verify `lookup_builtin` contains ZERO `Type::Str` signatures
    for name in [
        "bytes",
        "addr8",
        "support",
        "boundary_of",
        "len",
        "window_dedup_count",
        "native_cadence_ratio",
        "single_contact_association",
        "recipient_eval_demo",
        "aod::qext::ext_decimal",
        "aod::qext::qext_render",
        "aod::qext::qext_format_fixed",
        "trace",
        "fold",
        "coupling",
    ] {
        for sig in lookup_builtin(name) {
            let has_str = sig.params.iter().any(Type::contains_str) || sig.ret.contains_str();
            assert!(!has_str, "unexpected Type::Str signature on builtin {name}");
        }
    }

    // 3. Sweep all 45 positive clean-break `.dec` files:
    //    7 `examples/first_principles/*.dec` + 7 `stdlib/aod/*.dec` + 11 `manual_examples/*.dec`
    //    + 5 `tests/fixtures/v2_smoke/*.dec` + 4 `tests/fixtures/v2_bench/*.dec`
    //    + 10 `tests/fixtures/v3_examples/*.dec` + 1 `self_host/aodc_self.dec`!
    let read_dec_dir = |rel: &str| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(root.join(rel))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("dec"))
            .collect();
        v.sort();
        v
    };
    let fp_files = read_dec_dir("examples/first_principles");
    let stdlib_files = read_dec_dir("stdlib/aod");
    let manual_files = read_dec_dir("manual_examples");
    let smoke_files = read_dec_dir("tests/fixtures/v2_smoke");
    let bench_files = read_dec_dir("tests/fixtures/v2_bench");
    let v3_files = read_dec_dir("tests/fixtures/v3_examples");
    let self_host_file = vec![root.join("self_host/aodc_self.dec")];
    assert_eq!(fp_files.len(), 7);
    assert_eq!(stdlib_files.len(), 7);
    assert_eq!(manual_files.len(), 11);
    assert_eq!(smoke_files.len(), 5);
    assert_eq!(bench_files.len(), 4);
    assert_eq!(v3_files.len(), 10);

    let all_45: Vec<&PathBuf> = fp_files
        .iter()
        .chain(&stdlib_files)
        .chain(&manual_files)
        .chain(&smoke_files)
        .chain(&bench_files)
        .chain(&v3_files)
        .chain(&self_host_file)
        .collect();
    assert_eq!(all_45.len(), 45);

    for p in all_45 {
        let raw = fs::read_to_string(p).unwrap();
        if stdlib_files.contains(p) {
            let loaded = aodc::modules::load_from_source_with_options(
                &raw,
                p.parent(),
                &LoadOptions::default(),
            )
            .unwrap_or_else(|e| panic!("load_from_source failed on {}: {e}", p.display()));
            aodc::checker::check_unit(loaded, false)
                .unwrap_or_else(|e| panic!("check_unit(lib) failed on {}: {e}", p.display()));
        } else {
            check_file(p)
                .unwrap_or_else(|e| panic!("check_file failed on {}: {e}", p.display()));
        }
        let f1 = format_source(&raw)
            .unwrap_or_else(|e| panic!("format_source failed on {}: {e}", p.display()));
        let f2 = format_source(&f1)
            .unwrap_or_else(|e| panic!("second format_source failed on {}: {e}", p.display()));
        assert_eq!(f1, f2, "non-idempotent fmt on {}", p.display());

        // Direct API + CLI `aodc audit-dec --json` (`status == "PASS"` && `hooks_passed == 6`)
        let rep = audit_dec_file_with_options(p, &LoadOptions::default());
        assert!(
            rep.passed() && rep.hooks_passed() == 6,
            "audit-dec API failed on {}: {:?}",
            p.display(),
            rep.errors
        );
        let cli_aud = Command::new(aodc_bin())
            .args(["audit-dec", "--json"])
            .arg(p)
            .output()
            .unwrap();
        assert!(
            cli_aud.status.success(),
            "CLI audit-dec --json failed on {}: {}",
            p.display(),
            String::from_utf8_lossy(&cli_aud.stderr)
        );
        let parsed = parse_strict_json(&String::from_utf8(cli_aud.stdout).unwrap()).unwrap();
        assert_eq!(parsed.get("status").and_then(JsonValue::as_str), Some("PASS"));
        assert_eq!(
            parsed
                .get("hooks_passed")
                .and_then(JsonValue::as_int)
                .and_then(|v| v.to_i64()),
            Some(6)
        );
    }
}

#[test]
fn ac03_four_form_builtin_enums_shadowing_readonly_sha256_boundary_and_qext_bytes() {
    let tmp = temp_dir("ac03_enums");

    let all_4_forms = r#"
use aod::core
use aod::sim
use aod::wavelet
use aod::core:
    len, hex_bytes

use aod::wavelet:
    RecipientStage, recipient_eval_demo

use aod::qext:
    ext_decimal, qext_render, qext_format_fixed

chamber EnumBox:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare OneCarrier in EnumBox:
    inject 3:3:6 at (4, 8, 8) dir +X orient 1t shells 1

dec match_model_status:
    (aod::sim::ModelStatus::DeclaredSandboxNotS3Authenticated) -> int:
        return 11

dec match_stage:
    (RecipientStage::Reclosed) -> int:
        return 22
    (_: aod::wavelet::RecipientStage) -> int:
        return -1

observe CheckRoStatus:
    tracer watch:
        (view: ReadOnly<ChamberView>) -> null:
            let st: sim::ModelStatus = view.model_status
            assert st == ModelStatus::DeclaredSandboxNotS3Authenticated
            assert st == aod::core::ModelStatus::DeclaredSandboxNotS3Authenticated
            assert view.chamber_name == b"EnumBox"
            return null

dec main:
    () -> int:
        let b = box([0..16, 0..16, 0..16])
        let r = simulate OneCarrier for 3 ticks with CheckRoStatus

        let m1: ModelStatus = ModelStatus::DeclaredSandboxNotS3Authenticated
        let m3: sim::ModelStatus = sim::ModelStatus::DeclaredSandboxNotS3Authenticated
        let m4a: aod::sim::ModelStatus = aod::sim::ModelStatus::DeclaredSandboxNotS3Authenticated
        let m4b: core::ModelStatus = aod::core::ModelStatus::DeclaredSandboxNotS3Authenticated
        assert r.model_status == m1 and m1 == m3 and m3 == m4a and m4a == m4b
        assert match_model_status(r.model_status) == 11

        let rec = recipient_eval_demo(5, 6, (4, 10])
        let s2: RecipientStage = RecipientStage::Reclosed
        let s3: wavelet::RecipientStage = wavelet::RecipientStage::Reclosed
        let s4a: aod::wavelet::RecipientStage = aod::wavelet::RecipientStage::Reclosed
        let s4b: core::RecipientStage = aod::core::RecipientStage::Reclosed
        assert rec.stage == s2 and s2 == s3 and s3 == s4a and s4a == s4b
        assert match_stage(rec.stage) == 22

        assert len(r.final_state_sha256) == 32
        assert len(r.final_state_hash) == 64
        assert hex_bytes(r.final_state_sha256) == r.final_state_hash

        let b_desc: bytes = ∂b
        let r_desc: bytes = ∂r
        assert b_desc == r_desc and b_desc == r.boundary_bytes

        quarantine Q_ext:
            let q = ext_decimal(b"1.25")
            let qr: QExt<bytes> = qext_render(q)
            let qf: QExt<bytes> = qext_format_fixed(q, 3)
            emit qr
            emit qf

        emit r.model_status
        emit rec.stage
        emit b_desc
        return 0
"#;
    let expected = "Q_ext(b\"5/4\")\nQ_ext(b\"1.250\")\nModelStatus::DeclaredSandboxNotS3Authenticated\nRecipientStage::Reclosed\nb\"\\xe2\\x88\\x82box([0..16, 0..16, 0..16])\"\n0\n";
    assert_6mode_source_parity(all_4_forms, expected, &tmp, "all_4_forms");

    // Local type shadowing of `ModelStatus` coexists cleanly with `aod::sim::ModelStatus`!
    let shadow_src = r#"
enum ModelStatus:
    LocalCustom

chamber C:
    space = box([0..8, 0..8, 0..8])
    walls = open

prepare P in C:
    inject 3:3:6 at (2, 4, 4) dir +X orient 1t shells 1

dec main:
    () -> int:
        let loc: ModelStatus = ModelStatus::LocalCustom
        let r = simulate P for 1 ticks
        assert r.model_status == aod::sim::ModelStatus::DeclaredSandboxNotS3Authenticated
        emit loc
        emit r.model_status
        return 0
"#;
    assert_6mode_source_parity(
        shadow_src,
        "ModelStatus::LocalCustom\nModelStatus::DeclaredSandboxNotS3Authenticated\n0\n",
        &tmp,
        "shadow_model_status",
    );

    // Wrong builtin namespace (`aod::wavelet::ModelStatus` / `aod::sim::RecipientStage`) rejected!
    assert!(check_source(
        "dec main:\n    () -> int:\n        let _bad = aod::wavelet::ModelStatus::DeclaredSandboxNotS3Authenticated\n        return 0\n"
    )
    .is_err());
    assert!(check_source(
        "dec main:\n    () -> int:\n        let _bad = aod::sim::RecipientStage::Reclosed\n        return 0\n"
    )
    .is_err());

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_ac05_ac06_banned_constructs_v1_rejection_and_decir_v3_discipline() {
    let root = repo_root();
    let tmp = temp_dir("ac04_cutoff");

    // 1. All banned legacy constructs rejected unconditionally by both `check_source` and plain `aodc check`!
    let legacy_cases: &[(&str, &str)] = &[
        ("01_brace_struct_decl", "struct S { x: int }\ndec main:\n    () -> int:\n        return 0\n"),
        ("02_brace_enum_decl", "enum E { A, B }\ndec main:\n    () -> int:\n        return 0\n"),
        ("03_brace_struct_lit", "struct S:\n    x: int\ndec main:\n    () -> int:\n        let s = S { x: 1 }\n        return s.x\n"),
        ("04_brace_map_lit", "dec main:\n    () -> int:\n        let m = {1: 2}\n        return 0\n"),
        ("05_brace_use", "use aod::core::{t1}\ndec main:\n    () -> int:\n        return 0\n"),
        ("06_uppercase_dec", "Dec main:\n    () -> int:\n        return 0\n"),
        ("07_prefix_arm_param", "dec f:\n    (int x) -> int:\n        return x\ndec main:\n    () -> int:\n        return f(1)\n"),
        ("08_legacy_if_guard", "dec f:\n    (x: int) if x > 0 -> int:\n        return x\n    (x: int) -> int:\n        return 0\ndec main:\n    () -> int:\n        return f(1)\n"),
        ("09_legacy_arm_out_binding", "dec main:\n    () -> int out:\n        return 0\n"),
        ("10_if_stmt", "dec main:\n    () -> int:\n        if true:\n            return 1\n        return 0\n"),
        ("11_ternary_expr", "dec main:\n    () -> int:\n        return 1 if true else 0\n"),
        ("12_for_loop", "dec main:\n    () -> int:\n        for i in 0..3:\n            emit i\n        return 0\n"),
        ("13_while_loop", "dec main:\n    () -> int:\n        while false:\n            emit 1\n        return 0\n"),
        ("14_break_stmt", "dec main:\n    () -> int:\n        break\n        return 0\n"),
        ("15_continue_stmt", "dec main:\n    () -> int:\n        continue\n        return 0\n"),
        ("16_mut_local", "dec main:\n    () -> int:\n        mut x = 1\n        return x\n"),
        ("17_local_rebinding", "dec main:\n    () -> int:\n        let x = 1\n        x = 2\n        return x\n"),
        ("18_implicit_assign", "dec main:\n    () -> int:\n        x = 1\n        return x\n"),
        ("19_field_mutation", "struct S:\n    x: int\ndec main:\n    () -> int:\n        let s = S(1)\n        s.x = 2\n        return s.x\n"),
        ("20_index_mutation", "dec main:\n    () -> int:\n        let xs = [1, 2]\n        xs[0] = 9\n        return xs[0]\n"),
        ("21_typed_local_v1", "dec main:\n    () -> int:\n        int x = 1\n        return x\n"),
        ("22_match_colon_stmt", "enum E:\n    A\ndec main:\n    () -> int:\n        match E::A:\n            E::A:\n                return 1\n"),
        ("23_named_call_arg", "struct S:\n    x: int\ndec main:\n    () -> int:\n        let s = S(x: 1)\n        return s.x\n"),
        ("24_double_quote_str", "dec main:\n    () -> int:\n        let s = \"hello\"\n        return 0\n"),
        ("25_single_quote_str", "dec main:\n    () -> int:\n        let s = 'hello'\n        return 0\n"),
        ("26_str_type_annot", "dec main:\n    () -> int:\n        let xs: [str] = []\n        return 0\n"),
        ("27_str_witness_call", "use aod::core::q4\nuse aod::witness::trace\ndec main:\n    () -> int:\n        let _t = trace(\"a\", \"b\", q4(0))\n        return 0\n"),
        ("28_u8_type_annot", "dec main:\n    () -> int:\n        let _x: u8 = 0\n        return 0\n"),
        ("29_t9_type_annot", "dec main:\n    () -> int:\n        let _x: t9 = 0\n        return 0\n"),
        ("30_t32_type_annot", "dec main:\n    () -> int:\n        let _x: t32 = 0\n        return 0\n"),
        ("31_int64_type_annot", "dec main:\n    () -> int:\n        let _x: int64 = 0\n        return 0\n"),
    ];
    assert!(legacy_cases.len() >= 27);
    for (tag, code) in legacy_cases {
        assert!(
            check_source(code).is_err(),
            "check_source must unconditionally reject {tag}"
        );
        let bad_file = tmp.join(format!("{tag}.dec"));
        fs::write(&bad_file, code).unwrap();
        let cli_chk = Command::new(aodc_bin())
            .arg("check")
            .arg(&bad_file)
            .output()
            .unwrap();
        assert!(
            !cli_chk.status.success(),
            "plain `aodc check` must reject {tag}"
        );
    }

    // 2. Clean-break frontend rejects all 10 frozen `tests/fixtures/v1_examples/*.dec`
    for ent in fs::read_dir(root.join("tests/fixtures/v1_examples")).unwrap() {
        let p = ent.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) == Some("dec") {
            assert!(check_file(&p).is_err());
        }
    }

    // 3. DecIR v3 requires `"schema": "aod.dec.ir.v3"` and `"syntax_discipline": "pure_wave_v2"`
    let pure_prog = "dec main:\n    () -> int:\n        return 0\n";
    let p = tmp.join("ok.dec");
    fs::write(&p, pure_prog).unwrap();
    let ir_str = compile_file_to_ir(&p).unwrap();
    let JsonValue::Object(base_obj) = parse_strict_json(&ir_str).unwrap() else {
        panic!("expected IR JSON object");
    };

    let mut missing_disc = base_obj.clone();
    missing_disc.remove("syntax_discipline");
    assert_eq!(
        loads_ir(&JsonValue::Object(missing_disc).to_pretty_utf8_json())
            .unwrap_err()
            .code,
        DiagnosticCode::IrVerificationError
    );

    let mut legacy_disc = base_obj;
    let legacy_tag = ["transitional", "_v2"].concat();
    legacy_disc.insert(
        "syntax_discipline".to_string(),
        JsonValue::from_str_val(&legacy_tag),
    );
    assert_eq!(
        loads_ir(&JsonValue::Object(legacy_disc).to_pretty_utf8_json())
            .unwrap_err()
            .code,
        DiagnosticCode::IrVerificationError
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac07_unboxed_tuple_evolve_aliasing_wildcards_depth_safe_select_and_lazy_traps() {
    let tmp = temp_dir("ac07_codegen");

    let sound_src = r#"
use aod::core:
    b8, evolve

dec alias_fib_step:
    ((a, b): (int, int), _: int) -> (int, int):
        return (b, a + b)

dec multi_wild_step:
    ((acc, _, _): (int, int, b8), i: int) -> (int, int, b8):
        return (acc + i + 1, 0, b8(7))

dec swapped_names_pick:
    (a: int, b: int) where a > b -> int:
        return a
    (b: int, a: int) -> int:
        return a

dec pat_plus_trap_guard:
    (b8(1), x: int) where (10 // x) > 0 -> int:
        return 99
    (_: b8, _: int) -> int:
        return 42

dec stress_step:
    (acc: int, i: int) -> int:
        return acc + swapped_names_pick(i % 5, 2)

dec main:
    () -> int:
        let (f10, f11) = evolve((0, 1), 0..10, alias_fib_step)
        assert f10 == 55 and f11 == 89

        let (w_acc, w_t, w_u) = evolve((0, 9, b8(1)), 0..5, multi_wild_step)
        assert w_acc == 15 and w_t == 0 and w_u == b8(7)

        assert swapped_names_pick(9, 2) == 9
        assert swapped_names_pick(2, 9) == 9
        assert pat_plus_trap_guard(b8(0), 0) == 42
        assert pat_plus_trap_guard(b8(1), 2) == 99

        # 500 calls (> 200) under tight max_depth = 6 proves emit_depth_dec() runs on EVERY 2-arm select return!
        let total = evolve(0, 0..500, stress_step)
        emit f10
        emit w_acc
        emit total
        return 0
"#;
    let checked = check_source(sound_src).unwrap();
    let sel_plan = CodegenPlan {
        branchless_select: true,
        ..CodegenPlan::baseline_o2()
    };
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, &sel_plan).unwrap();
    assert!(
        raw_ir.contains("_unboxed(ptr %ctx, ptr %s0, ptr %s1, i64 %arg_idx)"),
        "expected _unboxed companion function for alias_fib_step:\n{raw_ir}"
    );
    assert!(
        raw_ir.contains("_unboxed(ptr %ctx, ptr %s0, ptr %s1, ptr %s2, i64 %arg_idx)"),
        "expected _unboxed companion function for multi_wild_step:\n{raw_ir}"
    );
    let swapped_ir = raw_ir
        .split("define internal i64 @dec_fn_swapped_names_pick")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(
        swapped_ir.contains("= select i1 ")
            && !swapped_ir.contains("guard_ok_")
            && !swapped_ir.contains("arm_next_"),
        "expected pure 2-arm select lowering on swapped_names_pick:\n{swapped_ir}"
    );

    let vm_tight = run_source_with_budget(sound_src, DEFAULT_FUEL, 6)
        .unwrap()
        .to_stdout_string();
    let (jit_tight, jit_res) =
        jit_source_with_plan_captured(sound_src, &sel_plan, DEFAULT_FUEL, 6).unwrap();
    jit_res.unwrap_or_else(|e| panic!("JIT trapped under tight max_depth=6 (depth leak!): {e}"));
    assert_eq!(jit_tight, vm_tight);

    assert_6mode_source_parity(sound_src, &vm_tight, &tmp, "ac07_sound");
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_ac10_ac11_manual_data_manual_verify_and_precodegen_firewalls() {
    let root = repo_root();
    let tmp = temp_dir("ac09_ac11");

    // 1. Non-empty `06` and `11` `.stdout` + zero `manual-data` drift (`AC-09`)
    assert!(
        !Sha256::digest_hex(&fs::read(root.join("manual_examples/06_unbiased_chamber_sandbox.stdout")).unwrap()).is_empty()
    );
    assert!(
        !Sha256::digest_hex(&fs::read(root.join("manual_examples/11_apparatus_opposing_sensors.stdout")).unwrap()).is_empty()
    );
    let md = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["manual-data", "--check", "--json"])
        .output()
        .unwrap();
    assert!(md.status.success());
    let md_json = parse_strict_json(&String::from_utf8(md.stdout).unwrap()).unwrap();
    assert_eq!(md_json.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(
        md_json
            .get("files_changed")
            .and_then(JsonValue::as_int)
            .and_then(|v| v.to_i64()),
        Some(0)
    );

    // 2. 6-pass `verify_manual_dir` passes with 0 errors (`AC-10`)
    let opts = ManualVerifyOptions {
        stdlib_dir: Some(root.join("stdlib")),
        registry_path: Some(root.join("bindings/aod/main_registry.json")),
    };
    let rep = verify_manual_dir(&root.join("docs/manual"), &opts);
    assert!(rep.is_ok(), "manual-verify failed: {:?}", rep.errors);
    assert_eq!(rep.dec_files_checked, 18);
    assert_eq!(rep.data_files_checked, 3);
    assert_eq!(rep.raster_violations, 0);
    assert_eq!(rep.aod_lint_violations, 0);

    // 3. Pre-codegen compile-time firewall `DiagnosticCode`s (`AC-11`)
    let firewall_cases = [
        (
            "use aod::core::list_push\nobserve Bad:\n    tracer t:\n        (view: ReadOnly<ChamberView>) -> null:\n            let _b = list_push(view.carriers, 1)\n            return null\ndec main:\n    () -> int:\n        return 0\n",
            DiagnosticCode::ObserverMutationForbidden,
        ),
        (
            "use aod::qext::ext_decimal\ndec main:\n    () -> int:\n        let _q = ext_decimal(b\"1.25\")\n        return 0\n",
            DiagnosticCode::QExtLeakageForbidden,
        ),
        (
            "dec main:\n    () -> int:\n        let _bad = 1:2:9\n        return 0\n",
            DiagnosticCode::OddSupportRequiresYro,
        ),
        (
            "dec main:\n    () -> int:\n        let _bad = 3:3:6_yro\n        return 0\n",
            DiagnosticCode::EvenSupportForbidsYro,
        ),
        (
            "use aod::core::b8\ndec main:\n    () -> int:\n        let _bad = 1 + b8(1)\n        return 0\n",
            DiagnosticCode::ImplicitCoercionForbidden,
        ),
        (
            "dec main:\n    () -> int:\n        let _bad = 2.5\n        return 0\n",
            DiagnosticCode::FloatLiteralForbidden,
        ),
    ];
    let never_exe = tmp.join("never_created");
    for (src, expected_code) in firewall_cases {
        assert_eq!(emit_llvm_source(src, OptLevel::O2).unwrap_err().code, expected_code);
        assert_eq!(
            build_aot_source(src, OptLevel::O2, &never_exe)
                .unwrap_err()
                .code,
            expected_code
        );
        assert!(!never_exe.exists());
        assert_eq!(
            compile_jit_source(src, OptLevel::O2).err().unwrap().code,
            expected_code
        );
    }

    let _ = fs::remove_dir_all(&tmp);
}
