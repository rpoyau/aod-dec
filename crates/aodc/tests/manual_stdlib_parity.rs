use std::path::{Path, PathBuf};
use std::process::Command;

use aodc::diagnostics::{CompatMode, DiagnosticCode};
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::{
    build_aot_file, build_aot_source, check_file, check_file_with_options, check_source,
    compile_file_to_ir, jit_file_captured, jit_source_captured, run_dec_tests, run_file,
    run_source, LoadOptions, OptLevel,
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
    let d = std::env::temp_dir().join(format!("aodc_pr3_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn assert_5mode_manual_parity(path: &Path, expected_stdout: &str, tmp: &Path, stem: &str) {
    // 1. Exact VM under DEFAULT_FUEL = 100_000
    let vm = run_file(path, CompatMode::V2)
        .unwrap_or_else(|e| panic!("VM failed on {}: {e}", path.display()))
        .to_stdout_string();
    assert_eq!(vm, expected_stdout, "VM stdout mismatch on {}", path.display());

    // 2 & 3. AOT -O0 and AOT -O2
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("{stem}_{tag}"));
        build_aot_file(path, CompatMode::V2, opt, &exe)
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
        let (stdout, res) =
            jit_file_captured(path, CompatMode::V2, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
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
fn ac02_stdlib_resolver_strict_precedence_and_no_silent_fallback() {
    let root = repo_root();
    let ex01 = root.join("manual_examples/01_dec_ledger.dec");
    let valid_stdlib = root.join("stdlib");
    let missing_stdlib = root.join("does_not_exist_stdlib_override");

    // 1. Explicit `--stdlib <nonexistent>` MUST fail with ModuleNotFound (ZERO silent fallback, P0-2!)
    let bad_opt = LoadOptions {
        stdlib_dir: Some(missing_stdlib.clone()),
    };
    assert_eq!(
        check_file_with_options(&ex01, CompatMode::V2, &bad_opt)
            .unwrap_err()
            .code,
        DiagnosticCode::ModuleNotFound
    );

    // 2. CLI `--stdlib <nonexistent>` fails, while explicit `--stdlib <valid>` overrides bad AOD_STDLIB_DIR!
    let bad_env = Command::new(aodc_bin())
        .env("AOD_STDLIB_DIR", &missing_stdlib)
        .args(["check"])
        .arg(&ex01)
        .output()
        .unwrap();
    assert!(!bad_env.status.success());
    assert!(
        String::from_utf8_lossy(&bad_env.stderr).contains("ModuleNotFound"),
        "expected ModuleNotFound on bad AOD_STDLIB_DIR, got: {}",
        String::from_utf8_lossy(&bad_env.stderr)
    );

    let cli_override = Command::new(aodc_bin())
        .env("AOD_STDLIB_DIR", &missing_stdlib)
        .args(["check", "--stdlib"])
        .arg(&valid_stdlib)
        .arg(&ex01)
        .output()
        .unwrap();
    assert!(
        cli_override.status.success(),
        "--stdlib should override bad AOD_STDLIB_DIR: {}",
        String::from_utf8_lossy(&cli_override.stderr)
    );

    // 3. Bare `use stdlib` and missing stdlib module rejected with ModuleNotFound
    assert_eq!(
        check_source("use stdlib\nDec main() -> int:\n    return 0\n", CompatMode::V2)
            .unwrap_err()
            .code,
        DiagnosticCode::ModuleNotFound
    );
    assert_eq!(
        check_source(
            "use stdlib::aod::no_such_module\nDec main() -> int:\n    return 0\n",
            CompatMode::V2
        )
        .unwrap_err()
        .code,
        DiagnosticCode::ModuleNotFound
    );

    // 4. `--stdlib` wired across run, emit-llvm, build, jit, test (P1-3)
    let tmp = temp_dir("ac02_cli_stdlib");
    let exe = tmp.join("ex01_elf");
    for args in [
        vec!["run", "--stdlib"],
        vec!["emit-llvm", "-O2", "--stdlib"],
        vec!["jit", "-O2", "--stdlib"],
        vec!["test", "--stdlib"],
    ] {
        let out = Command::new(aodc_bin())
            .env("AOD_STDLIB_DIR", &missing_stdlib)
            .args(&args)
            .arg(&valid_stdlib)
            .arg(&ex01)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "CLI {:?} --stdlib failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let build = Command::new(aodc_bin())
        .env("AOD_STDLIB_DIR", &missing_stdlib)
        .args(["build", "-O2", "--stdlib"])
        .arg(&valid_stdlib)
        .arg(&ex01)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(build.status.success());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac03_cross_module_qext_2seg_3seg_firewalls_and_topological_consts() {
    // 1. Hostile 2-segment & 3-segment Q_ext const and fn leaks MUST fail with QExtLeakageForbidden!
    let hostile_qext_cases = [
        "use stdlib::aod::metrology::SiMetrology\nDec main() -> int:\n    let x = SiMetrology::DELTA_NU_CS\n    return 0\n",
        "use stdlib::aod::metrology::SiMetrology\nDec main() -> int:\n    let x = SiMetrology::duration_seconds(1/1r)\n    return 0\n",
        "use stdlib::aod::metrology\nDec main() -> int:\n    let x = metrology::SiMetrology::DELTA_NU_CS\n    return 0\n",
        "use stdlib::aod::metrology\nDec main() -> int:\n    let x = metrology::SiMetrology::duration_seconds(1/1r)\n    return 0\n",
    ];
    for src in hostile_qext_cases {
        assert_eq!(
            check_source(src, CompatMode::V2).unwrap_err().code,
            DiagnosticCode::QExtLeakageForbidden,
            "expected QExtLeakageForbidden for:\n{src}"
        );
    }

    // 2. Positive 2-segment & 3-segment Q_ext calls inside `quarantine Q_ext:` across VM, AOT, JIT
    let ok_src = r#"
use aod::qext::qext_render
use stdlib::aod::metrology
use stdlib::aod::metrology::SiMetrology

Dec main() -> int:
    let two = 2/1r
    quarantine Q_ext:
        let a = SiMetrology::duration_seconds(two)
        let b = metrology::SiMetrology::duration_seconds(two)
        assert a == b
        assert SiMetrology::DELTA_NU_CS == metrology::SiMetrology::DELTA_NU_CS
        emit qext_render(a)
    return 0
"#;
    let expected = run_source(ok_src, CompatMode::V2).unwrap().to_stdout_string();
    assert_eq!(expected, "Q_ext(1/4596315885)\n0\n");

    let tmp = temp_dir("ac03_qext_topo");
    let exe = tmp.join("qext_ok");
    build_aot_source(ok_src, CompatMode::V2, OptLevel::O2, &exe).unwrap();
    let aot_out = Command::new(&exe).output().unwrap();
    assert!(aot_out.status.success());
    assert_eq!(String::from_utf8_lossy(&aot_out.stdout), expected);

    let (jout, jres) = jit_source_captured(
        ok_src,
        CompatMode::V2,
        OptLevel::O2,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    jres.unwrap();
    assert_eq!(jout, expected);

    // 3. P0-1 Topological `module_load_order` const test:
    // `a_mid` sorts alphabetically BEFORE `z_leaf`, yet `a_mid` depends on `z_leaf::LEAF_ARR`!
    std::fs::write(
        tmp.join("z_leaf.dec"),
        "const LEAF_ARR = [11, 22, 33]\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("a_mid.dec"),
        "use z_leaf::LEAF_ARR\nconst MID_ARR = LEAF_ARR\n",
    )
    .unwrap();
    let entry_file = tmp.join("entry.dec");
    std::fs::write(
        &entry_file,
        "use a_mid::MID_ARR\nconst TOP_ARR = MID_ARR\nDec main() -> int:\n    emit TOP_ARR[0]\n    emit TOP_ARR[2]\n    return 0\n",
    )
    .unwrap();
    let topo_expected = "11\n33\n0\n";
    assert_5mode_manual_parity(&entry_file, topo_expected, &tmp, "topo_consts");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac04_to_ac10_all_6_manual_examples_5mode_parity_against_stdout() {
    let root = repo_root();
    let dir = root.join("manual_examples");
    let tmp = temp_dir("manual_5mode");

    let fixtures = [
        "01_dec_ledger",
        "02_closure_wavelets",
        "03_short_window_boundary",
        "04_cell_measures_tesseract",
        "05_conditional_si_metrology",
        "06_unbiased_chamber_sandbox",
    ];
    for stem in fixtures {
        let dec_path = dir.join(format!("{stem}.dec"));
        let stdout_path = dir.join(format!("{stem}.stdout"));
        let expected = std::fs::read_to_string(&stdout_path)
            .unwrap_or_else(|e| panic!("missing {}: {e}", stdout_path.display()));
        assert_5mode_manual_parity(&dec_path, &expected, &tmp, stem);
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac11_hermetic_dec_ir_v2_with_stdlib_ignores_bad_env_and_rejects_tamper() {
    let root = repo_root();
    let tmp = temp_dir("ac11_hermetic_stdlib");

    for stem in ["03_short_window_boundary", "05_conditional_si_metrology"] {
        let src = root.join(format!("manual_examples/{stem}.dec"));
        let expected = std::fs::read_to_string(root.join(format!("manual_examples/{stem}.stdout")))
            .unwrap();
        let ir_text = compile_file_to_ir(&src, CompatMode::V2).unwrap();

        if stem == "03_short_window_boundary" {
            assert!(ir_text.contains("\"stdlib::aod::boundary\""));
            assert!(ir_text.contains("\"stdlib::aod::ledger\""));
        } else {
            assert!(ir_text.contains("\"stdlib::aod::metrology\""));
        }

        let ir_path = tmp.join(format!("{stem}.ir.json"));
        std::fs::write(&ir_path, &ir_text).unwrap();

        // Execute strictly hermetically from isolated temp dir with nonexistent AOD_STDLIB_DIR (`P0-2`)!
        let exec = Command::new(aodc_bin())
            .current_dir(&tmp)
            .env("AOD_STDLIB_DIR", "/nonexistent/hermetic/stdlib")
            .arg("exec")
            .arg(&ir_path)
            .output()
            .unwrap();
        assert!(
            exec.status.success(),
            "hermetic exec failed for {stem}: {}",
            String::from_utf8_lossy(&exec.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&exec.stdout), expected);

        // Tamper 1 byte in bundled stdlib source -> MUST fail `loads_ir` with IrVerificationError!
        let tampered = if stem == "03_short_window_boundary" {
            ir_text.replacen("moving_boundary_flux", "moving_boundary_fluy", 1)
        } else {
            ir_text.replacen("DELTA_NU_CS", "DELTA_NU_CT", 1)
        };
        assert_ne!(tampered, ir_text);
        assert_eq!(
            loads_ir(&tampered).unwrap_err().code,
            DiagnosticCode::IrVerificationError
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac12_ac13_negative_controls_expect_error_hardening_and_cli_test() {
    let root = repo_root();
    let nc_dir = root.join("manual_examples/negative_controls");
    let tmp = temp_dir("ac12_nc");

    // 1. Compile-time negative controls nc01..nc03 fail across check/run/build/jit with zero artifacts
    let ct_cases = [
        ("nc01_qext_delta_nu_leak", DiagnosticCode::QExtLeakageForbidden),
        (
            "nc02_observer_mutates_chamber",
            DiagnosticCode::ObserverMutationForbidden,
        ),
        (
            "nc03_odd_support_missing_yro",
            DiagnosticCode::OddSupportRequiresYro,
        ),
    ];
    for (stem, expected_code) in ct_cases {
        let p = nc_dir.join(format!("{stem}.dec"));
        assert_eq!(check_file(&p, CompatMode::V2).unwrap_err().code, expected_code);
        let out_exe = tmp.join(format!("{stem}_should_not_exist"));
        assert_eq!(
            build_aot_file(&p, CompatMode::V2, OptLevel::O2, &out_exe)
                .unwrap_err()
                .code,
            expected_code
        );
        assert!(!out_exe.exists());
        assert_eq!(
            jit_file_captured(&p, CompatMode::V2, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .unwrap_err()
                .code,
            expected_code
        );
    }

    // 2. Runtime negative controls nc04..nc06 fail across VM, AOT -O2, and ORC JIT -O2
    for stem in [
        "nc04_reversed_sweep_sign",
        "nc05_origin_j0_as_beat",
        "nc06_duad_drops_hinge_0h",
    ] {
        let p = nc_dir.join(format!("{stem}.dec"));
        assert_eq!(
            run_file(&p, CompatMode::V2).unwrap_err().code,
            DiagnosticCode::AssertionFailed
        );

        let exe = tmp.join(format!("{stem}_o2"));
        build_aot_file(&p, CompatMode::V2, OptLevel::O2, &exe).unwrap();
        let aot = Command::new(&exe).output().unwrap();
        assert!(!aot.status.success(), "AOT expected failure on {stem}");
        assert!(String::from_utf8_lossy(&aot.stderr).contains("AssertionFailed"));

        let (_, jres) =
            jit_file_captured(&p, CompatMode::V2, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .unwrap();
        assert!(
            jres.unwrap_err().contains("AssertionFailed"),
            "JIT expected AssertionFailed on {stem}"
        );
    }

    // 3. P1-1 `.expect_error` hardening: empty file, dual `.stdout`+`.expect_error`, unexpected pass ALL FAIL!
    let empty_f = tmp.join("empty_exp.dec");
    std::fs::write(&empty_f, "Dec main() -> int:\n    assert false\n    return 0\n").unwrap();
    std::fs::write(tmp.join("empty_exp.expect_error"), "   \n").unwrap();
    let r_empty = run_dec_tests(&empty_f, CompatMode::V2).unwrap();
    assert_eq!(r_empty.len(), 1);
    assert!(!r_empty[0].passed, "empty .expect_error must FAIL");

    let dual_f = tmp.join("dual_exp.dec");
    std::fs::write(&dual_f, "Dec main() -> int:\n    return 0\n").unwrap();
    std::fs::write(tmp.join("dual_exp.stdout"), "0\n").unwrap();
    std::fs::write(tmp.join("dual_exp.expect_error"), "AssertionFailed\n").unwrap();
    let r_dual = run_dec_tests(&dual_f, CompatMode::V2).unwrap();
    assert_eq!(r_dual.len(), 1);
    assert!(!r_dual[0].passed, "dual .stdout+.expect_error must FAIL");

    let unexp_ok = tmp.join("unexp_ok.dec");
    std::fs::write(&unexp_ok, "Dec main() -> int:\n    return 0\n").unwrap();
    std::fs::write(tmp.join("unexp_ok.expect_error"), "AssertionFailed\n").unwrap();
    let r_ok = run_dec_tests(&unexp_ok, CompatMode::V2).unwrap();
    assert_eq!(r_ok.len(), 1);
    assert!(!r_ok[0].passed, "unexpected Ok(_) on .expect_error must FAIL");

    // 4. AC-13 / PR 8 P1-3: `aodc test manual_examples` -> 17 passed; 0 failed; 17 total (exit 0)
    let cli = Command::new(aodc_bin())
        .arg("test")
        .arg(root.join("manual_examples"))
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "aodc test manual_examples failed:\n{}\n{}",
        String::from_utf8_lossy(&cli.stdout),
        String::from_utf8_lossy(&cli.stderr)
    );
    let stdout = String::from_utf8_lossy(&cli.stdout);
    assert!(
        stdout.contains("test result: 17 passed; 0 failed; 17 total"),
        "unexpected test summary:\n{stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
