use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;
use aodc::diagnostics::{CompatMode, DiagnosticCode};
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::{
    build_aot_file, build_aot_source, check_file, check_source, compile_file_to_ir,
    jit_file_captured, jit_source_captured, run_file, run_source, OptLevel,
};

static S2_TEST_SEQ: AtomicU64 = AtomicU64::new(0);

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
    let seq = S2_TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "aodc_pr6_{}_{}_{tag}",
        std::process::id(),
        seq
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn assert_portable_json(text: &str) -> JsonValue {
    let parsed = parse_strict_json(text)
        .unwrap_or_else(|e| panic!("expected strict zero-float JSON, got error {e}: {text}"));
    for forbidden in ["/home/", "/tmp/", "/var/", "/Users/", "T00:", "T01:", "T02:"] {
        assert!(
            !text.contains(forbidden),
            "non-portable host path or timestamp token {forbidden:?} leaked in JSON:\n{text}"
        );
    }
    parsed
}

fn assert_5mode_parity(path: &Path, expected_stdout: &str, tmp: &Path, stem: &str) {
    let vm = run_file(path, CompatMode::V2)
        .unwrap_or_else(|e| panic!("VM failed on {}: {e}", path.display()))
        .to_stdout_string();
    assert_eq!(vm, expected_stdout, "VM stdout mismatch on {}", path.display());

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
fn ac06_cli_verify_s2_human_and_portable_json() {
    let root = repo_root();

    // 1. Human mode exits 0 and reports 30/30 PASS + draft status
    let human = Command::new(aodc_bin())
        .current_dir(&root)
        .arg("verify-s2")
        .output()
        .unwrap();
    assert!(
        human.status.success(),
        "aodc verify-s2 failed:\n{}\n{}",
        String::from_utf8_lossy(&human.stdout),
        String::from_utf8_lossy(&human.stderr)
    );
    let h_out = String::from_utf8_lossy(&human.stdout);
    assert!(h_out.contains("S2@3122c94"));
    assert!(h_out.contains("DRAFT_CANDIDATE_UNDER_S2_REVIEW"));
    assert!(h_out.contains("30/30 passed"));

    // 2. --json mode is deterministic byte-for-byte, strict zero-float, zero host paths
    let j1 = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["verify-s2", "--json"])
        .output()
        .unwrap();
    let j2 = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["verify-s2", "--json"])
        .output()
        .unwrap();
    assert!(j1.status.success());
    assert_eq!(j1.stdout, j2.stdout, "verify-s2 --json must be byte-deterministic");
    let j_text = String::from_utf8(j1.stdout).unwrap();
    let parsed = assert_portable_json(&j_text);
    assert_eq!(parsed.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(
        parsed.get("binding_status").and_then(JsonValue::as_str),
        Some("DRAFT_CANDIDATE_UNDER_S2_REVIEW")
    );
    assert_eq!(
        parsed
            .get("passed_vectors")
            .and_then(JsonValue::as_int)
            .and_then(|v| v.to_i64()),
        Some(30)
    );
    let ssum = parsed
        .get("scientific_status_summary")
        .and_then(JsonValue::as_object)
        .unwrap();
    assert_eq!(ssum.get("AUTHENTICATED_EXACT").and_then(JsonValue::as_int).and_then(|v| v.to_i64()), Some(18));
    assert_eq!(ssum.get("CONFORMANT_DECLARED_FIXTURE").and_then(JsonValue::as_int).and_then(|v| v.to_i64()), Some(5));
    assert_eq!(ssum.get("RETAINED_DATA_CONFORMANT").and_then(JsonValue::as_int).and_then(|v| v.to_i64()), Some(6));
    assert_eq!(ssum.get("UNRESOLVED_PROVENANCE_QUARANTINED").and_then(JsonValue::as_int).and_then(|v| v.to_i64()), Some(1));

    // 3. Zero silent fallback on explicit --vectors or --binding; emits strict FAIL JSON + exit 1
    let bad_vec = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["verify-s2", "--vectors", "/tmp/does_not_exist_s2_vectors_xyz", "--json"])
        .output()
        .unwrap();
    assert!(!bad_vec.status.success());
    let bad_v_json = assert_portable_json(&String::from_utf8_lossy(&bad_vec.stdout));
    assert_eq!(bad_v_json.get("status").and_then(JsonValue::as_str), Some("FAIL"));

    let bad_bind = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["verify-s2", "--binding", "/tmp/does_not_exist_s2_binding.json", "--json"])
        .output()
        .unwrap();
    assert!(!bad_bind.status.success());
    let bad_b_json = assert_portable_json(&String::from_utf8_lossy(&bad_bind.stdout));
    assert_eq!(bad_b_json.get("status").and_then(JsonValue::as_str), Some("FAIL"));
}

#[test]
fn ac07_hostile_tamper_drift_float_and_symlink_rejection() {
    let root = repo_root();
    let orig_rc = std::fs::read(root.join("bindings/aod/s2_vectors/rational_core.jsonl")).unwrap();
    let orig_sd =
        std::fs::read(root.join("bindings/aod/s2_vectors/symbolic_and_display.jsonl")).unwrap();
    let orig_sb = std::fs::read_to_string(root.join("SOURCE_BINDING.json")).unwrap();

    // 1. 1-byte tamper in rational_core.jsonl against real SOURCE_BINDING.json -> FAIL (SHA-256 mismatch)
    let d1 = temp_dir("tamper_1byte");
    let mut rc1 = orig_rc.clone();
    rc1.push(b'\n');
    std::fs::write(d1.join("rational_core.jsonl"), &rc1).unwrap();
    std::fs::write(d1.join("symbolic_and_display.jsonl"), &orig_sd).unwrap();
    let out1 = Command::new(aodc_bin())
        .args(["verify-s2", "--vectors"])
        .arg(&d1)
        .arg("--binding")
        .arg(root.join("SOURCE_BINDING.json"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(!out1.status.success());
    let j1 = assert_portable_json(&String::from_utf8_lossy(&out1.stdout));
    assert_eq!(j1.get("status").and_then(JsonValue::as_str), Some("FAIL"));
    let _ = std::fs::remove_dir_all(&d1);

    // 2. Expected-value drift WITH updated temp SOURCE_BINDING.json (SHA-256 matches, math catches drift!)
    let d2 = temp_dir("value_drift");
    let rc_str = String::from_utf8(orig_rc.clone()).unwrap();
    let drifted_rc = rc_str.replacen("\"Z\":8", "\"Z\":9", 1);
    assert_ne!(drifted_rc, rc_str);
    let drifted_sha = Sha256::digest_hex(drifted_rc.as_bytes());
    let temp_sb = orig_sb.replace(
        "a852094458a884ec456fd712a1fbfdb8cff46f2e90095b74bafe7644ad72cc99",
        &drifted_sha,
    );
    std::fs::write(d2.join("rational_core.jsonl"), &drifted_rc).unwrap();
    std::fs::write(d2.join("symbolic_and_display.jsonl"), &orig_sd).unwrap();
    let temp_sb_path = d2.join("TEMP_BINDING.json");
    std::fs::write(&temp_sb_path, &temp_sb).unwrap();
    let out2 = Command::new(aodc_bin())
        .args(["verify-s2", "--vectors"])
        .arg(&d2)
        .arg("--binding")
        .arg(&temp_sb_path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!out2.status.success());
    let j2 = assert_portable_json(&String::from_utf8_lossy(&out2.stdout));
    assert_eq!(j2.get("status").and_then(JsonValue::as_str), Some("FAIL"));
    assert_eq!(j2.get("passed_vectors").and_then(JsonValue::as_int).and_then(|v| v.to_i64()), Some(29));
    assert_eq!(j2.get("failed_vectors").and_then(JsonValue::as_int).and_then(|v| v.to_i64()), Some(1));
    // P1-2: scientific_status_summary MUST count ONLY passing cases (RC-001 CONFORMANT_DECLARED_FIXTURE 5 -> 4!)
    let ssum2 = j2.get("scientific_status_summary").and_then(JsonValue::as_object).unwrap();
    assert_eq!(
        ssum2.get("CONFORMANT_DECLARED_FIXTURE").and_then(JsonValue::as_int).and_then(|v| v.to_i64()),
        Some(4)
    );
    let _ = std::fs::remove_dir_all(&d2);

    // 3. Injected JSON float literal in rational_core.jsonl -> FAIL
    let d3 = temp_dir("injected_float");
    let float_rc = rc_str.replacen("\"Z\":8", "\"Z\":8.0", 1);
    std::fs::write(d3.join("rational_core.jsonl"), &float_rc).unwrap();
    std::fs::write(d3.join("symbolic_and_display.jsonl"), &orig_sd).unwrap();
    let out3 = Command::new(aodc_bin())
        .args(["verify-s2", "--vectors"])
        .arg(&d3)
        .arg("--binding")
        .arg(root.join("SOURCE_BINDING.json"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(!out3.status.success());
    let j3 = assert_portable_json(&String::from_utf8_lossy(&out3.stdout));
    assert_eq!(j3.get("status").and_then(JsonValue::as_str), Some("FAIL"));
    let _ = std::fs::remove_dir_all(&d3);

    // 4. Symlinked .jsonl and symlinked --binding MUST both be rejected (P1-2)
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let d4 = temp_dir("symlink_jsonl");
        symlink(
            root.join("bindings/aod/s2_vectors/rational_core.jsonl"),
            d4.join("rational_core.jsonl"),
        )
        .unwrap();
        std::fs::write(d4.join("symbolic_and_display.jsonl"), &orig_sd).unwrap();
        let out4 = Command::new(aodc_bin())
            .args(["verify-s2", "--vectors"])
            .arg(&d4)
            .arg("--binding")
            .arg(root.join("SOURCE_BINDING.json"))
            .arg("--json")
            .output()
            .unwrap();
        assert!(!out4.status.success());
        let j4 = assert_portable_json(&String::from_utf8_lossy(&out4.stdout));
        assert_eq!(j4.get("status").and_then(JsonValue::as_str), Some("FAIL"));
        assert!(String::from_utf8_lossy(&out4.stdout).contains("symlink forbidden"));
        let _ = std::fs::remove_dir_all(&d4);

        let d5 = temp_dir("symlink_binding");
        let sym_bind = d5.join("SYMLINK_BINDING.json");
        symlink(root.join("SOURCE_BINDING.json"), &sym_bind).unwrap();
        let out5 = Command::new(aodc_bin())
            .args(["verify-s2", "--vectors"])
            .arg(root.join("bindings/aod/s2_vectors"))
            .arg("--binding")
            .arg(&sym_bind)
            .arg("--json")
            .output()
            .unwrap();
        assert!(!out5.status.success());
        let j5 = assert_portable_json(&String::from_utf8_lossy(&out5.stdout));
        assert_eq!(j5.get("status").and_then(JsonValue::as_str), Some("FAIL"));
        assert!(String::from_utf8_lossy(&out5.stdout).contains("symlink forbidden"));
        let _ = std::fs::remove_dir_all(&d5);
    }
}

#[test]
fn ac08_native_sectors_firewall_qext_builtins_and_runtime_bounds() {
    let root = repo_root();
    let tmp = temp_dir("ac08_qext");

    // 1. P0-2 Static audit on stdlib/aod/sectors.dec: ZERO MeV/GeV/LHC/PDG/Q_ext in non-comment code lines
    let sectors_src = std::fs::read_to_string(root.join("stdlib/aod/sectors.dec")).unwrap();
    for (idx, line) in sectors_src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        for forbidden in ["MeV", "GeV", "LHC", "PDG", "Q_ext", "QExt", "aod::qext", "quarantine"] {
            assert!(
                !trimmed.contains(forbidden),
                "stdlib/aod/sectors.dec:{} must stay strictly native (found {forbidden:?})",
                idx + 1
            );
        }
    }

    // 2. P0-1 5-mode parity for direct qext_round_half_even & qext_format_fixed (including ties & negative zero)
    let qext_ok = r#"
use aod::qext::{qext_from_ratio, qext_round_half_even, qext_format_fixed, qext_render}

Dec main() -> int:
    let half_5 = 5/2r
    let half_7 = 7/2r
    let tiny_neg = -1/1000000000r
    quarantine Q_ext:
        let r5 = qext_round_half_even(qext_from_ratio(half_5), 0)
        let r7 = qext_round_half_even(qext_from_ratio(half_7), 0)
        let f_tiny = qext_format_fixed(qext_from_ratio(tiny_neg), 6)
        emit qext_render(r5)
        emit qext_render(r7)
        emit f_tiny
    return 0
"#;
    let expected = "Q_ext(2/1)\nQ_ext(4/1)\nQ_ext(0.000000)\n0\n";
    let vm = run_source(qext_ok, CompatMode::V2).unwrap().to_stdout_string();
    assert_eq!(vm, expected);
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.join(format!("qext_direct_{tag}"));
        build_aot_source(qext_ok, CompatMode::V2, opt, &exe).unwrap();
        let aot = Command::new(&exe).output().unwrap();
        assert!(aot.status.success());
        assert_eq!(String::from_utf8_lossy(&aot.stdout), expected);

        let (jout, jres) = jit_source_captured(
            qext_ok,
            CompatMode::V2,
            opt,
            DEFAULT_FUEL,
            DEFAULT_MAX_DEPTH,
        )
        .unwrap();
        jres.unwrap();
        assert_eq!(jout, expected);
    }

    // 3. P0-1 Runtime bounds failure across VM, AOT -O2, and ORC LLJIT -O2 (decimals = -1 and 1025)
    for (label, bad_dec) in [("neg", -1), ("excess", 1025)] {
        for call in ["qext_round_half_even", "qext_format_fixed"] {
            let src = format!(
                r#"
use aod::qext::{{qext_from_ratio, {call}}}

Dec main() -> int:
    let x = 1/3r
    quarantine Q_ext:
        let _y = {call}(qext_from_ratio(x), {bad_dec})
    return 0
"#
            );
            assert!(
                run_source(&src, CompatMode::V2).is_err(),
                "VM expected runtime bounds error for {call}({bad_dec})"
            );
            let exe = tmp.join(format!("bounds_{call}_{label}"));
            build_aot_source(&src, CompatMode::V2, OptLevel::O2, &exe).unwrap();
            let aot = Command::new(&exe).output().unwrap();
            assert!(
                !aot.status.success(),
                "AOT expected runtime bounds error for {call}({bad_dec})"
            );
            let (_, jres) = jit_source_captured(
                &src,
                CompatMode::V2,
                OptLevel::O2,
                DEFAULT_FUEL,
                DEFAULT_MAX_DEPTH,
            )
            .unwrap();
            assert!(
                jres.is_err(),
                "JIT expected runtime bounds error for {call}({bad_dec})"
            );
        }
    }

    // 4. P0-1 / P2-2 Extended QExtLeakageForbidden controls across check, build (zero artifact), and jit
    let nc07 = root.join("manual_examples/negative_controls/nc07_display_quarantine_leak.dec");
    assert_eq!(
        check_file(&nc07, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );
    let nc07_exe = tmp.join("nc07_must_not_exist");
    assert_eq!(
        build_aot_file(&nc07, CompatMode::V2, OptLevel::O2, &nc07_exe)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );
    assert!(!nc07_exe.exists());
    assert_eq!(
        jit_file_captured(&nc07, CompatMode::V2, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
            .unwrap_err()
            .code,
        DiagnosticCode::QExtLeakageForbidden
    );

    let hostile_leaks = [
        (
            "2seg_const",
            "use stdlib::aod::metrology::DisplayQuarantine\nDec main() -> int:\n    let x = DisplayQuarantine::LHC_CMS_H4L_MASS_GEV\n    return 0\n",
        ),
        (
            "3seg_fn",
            "use stdlib::aod::metrology\nDec main() -> int:\n    let x = metrology::DisplayQuarantine::higgs_freeze_gev(61, 7)\n    return 0\n",
        ),
        (
            "direct_round",
            "use aod::qext::{qext_from_ratio, qext_round_half_even}\nDec main() -> int:\n    let x = qext_round_half_even(qext_from_ratio(1/3r), 6)\n    return 0\n",
        ),
        (
            "direct_format_3seg",
            "Dec main() -> int:\n    let x = aod::qext::qext_format_fixed(aod::qext::qext_from_ratio(1/3r), 6)\n    return 0\n",
        ),
    ];
    for (tag, src) in hostile_leaks {
        assert_eq!(
            check_source(src, CompatMode::V2).unwrap_err().code,
            DiagnosticCode::QExtLeakageForbidden,
            "expected QExtLeakageForbidden on check for {tag}"
        );
        let exe = tmp.join(format!("leak_{tag}_must_not_exist"));
        assert_eq!(
            build_aot_source(src, CompatMode::V2, OptLevel::O2, &exe)
                .unwrap_err()
                .code,
            DiagnosticCode::QExtLeakageForbidden,
            "expected QExtLeakageForbidden on build for {tag}"
        );
        assert!(!exe.exists(), "hostile leak {tag} must produce ZERO AOT artifact");
        assert_eq!(
            jit_source_captured(
                src,
                CompatMode::V2,
                OptLevel::O2,
                DEFAULT_FUEL,
                DEFAULT_MAX_DEPTH
            )
            .unwrap_err()
            .code,
            DiagnosticCode::QExtLeakageForbidden,
            "expected QExtLeakageForbidden on jit for {tag}"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac09_manual_examples_07_08_09_5mode_and_hermetic_ir_parity() {
    let root = repo_root();
    let dir = root.join("manual_examples");
    let tmp = temp_dir("ac09_s2_5mode");

    let fixtures = [
        "07_s2_sparc_and_shedding",
        "08_s2_awareness_stokes_octants",
        "09_s2_higgs_and_quarantined_lensing",
    ];

    for stem in fixtures {
        let dec_path = dir.join(format!("{stem}.dec"));
        let stdout_path = dir.join(format!("{stem}.stdout"));
        let expected = std::fs::read_to_string(&stdout_path).unwrap();

        // 5-mode exact parity within DEFAULT_FUEL = 100_000 (P2-2)
        assert_5mode_parity(&dec_path, &expected, &tmp, stem);

        // Hermetic DecIR v2 compile + exec under nonexistent AOD_STDLIB_DIR + 1-byte tamper check
        let ir_text = compile_file_to_ir(&dec_path, CompatMode::V2).unwrap();
        assert!(ir_text.contains("\"stdlib::aod::sectors\""));
        if stem == "09_s2_higgs_and_quarantined_lensing" {
            assert!(ir_text.contains("\"stdlib::aod::metrology\""));
        }

        let ir_path = tmp.join(format!("{stem}.ir.json"));
        std::fs::write(&ir_path, &ir_text).unwrap();

        let exec = Command::new(aodc_bin())
            .current_dir(&tmp)
            .env("AOD_STDLIB_DIR", "/nonexistent/hermetic/s2_stdlib")
            .arg("exec")
            .arg(&ir_path)
            .output()
            .unwrap();
        assert!(
            exec.status.success(),
            "hermetic exec failed on {stem}: {}",
            String::from_utf8_lossy(&exec.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&exec.stdout), expected);

        let tampered = ir_text.replacen("sparc_bin_eval", "sparc_bin_evam", 1);
        assert_ne!(tampered, ir_text);
        assert_eq!(
            loads_ir(&tampered).unwrap_err().code,
            DiagnosticCode::IrVerificationError
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
