use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::ratio::Ratio;
use aod_core::s2_kernel::{
    canonical_rational_str, cycle_shedding_trace, parse_printed_rational, CycleSheddingTickInput,
    MAX_CYCLE_SHEDDING_TICKS,
};
use aod_core::s2_vectors::eval_operation;
use aod_core::s4_report::{
    sanitize_error, validate_portable_report, verify_s4_cycle_shedding_csv, FORBIDDEN_REPORT_KEYS,
    MAX_S4_INPUT_BYTES, S4_BASE_COMMIT_BINDING, S4_BINDING_ID, S4_CYCLE_SHEDDING_CSV_REL,
    S4_REPORT_SCHEMA,
};
use aodc::diagnostics::CompatMode;
use aodc::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::{
    build_aot_file, compile_file_to_ir, jit_file_captured, run_file, validate_runtime_output_path,
    verify_s4_convergence_dir, write_report_atomic, OptLevel,
};

static S4_TEST_SEQ: AtomicU64 = AtomicU64::new(0);

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

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(tag: &str) -> Self {
        let seq = S4_TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aodc_pr8_s4_{}_{}_{tag}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ============================================================================
// AC-02 / P0-2: Exact Upstream `cycle_shedding_trace` Schema & Bounds
// ============================================================================

#[test]
fn ac02_cycle_shedding_trace_kernel_and_json_schema() {
    // 1. Empty ticks -> final_burden == initial_burden, all max/totals == ["0", "1"]
    let empty_in = parse_strict_json(
        r#"{
            "initial_burden": ["3", "7"],
            "eta": "0.82",
            "gamma": ["11", "20"],
            "lambda_reclose": "0.38",
            "ticks": []
        }"#,
    )
    .unwrap();
    let empty_out = eval_operation("cycle_shedding_trace", empty_in.as_object().unwrap()).unwrap();
    let obj = empty_out.as_object().unwrap();
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec![
            "final_burden",
            "max_burden",
            "max_excess",
            "max_span",
            "ticks",
            "total_local",
            "total_outward",
        ]
    );
    assert_eq!(
        obj.get("final_burden").unwrap().to_compact_ascii_json(),
        r#"["3","7"]"#
    );
    assert_eq!(
        obj.get("total_outward").unwrap().to_compact_ascii_json(),
        r#"["0","1"]"#
    );
    assert_eq!(obj.get("ticks").unwrap().as_array().unwrap().len(), 0);

    // 2. Mixed canonical pairs & printed decimal strings -> matches RC-014 step
    let mixed_in = parse_strict_json(
        r#"{
            "initial_burden": ["1", "2"],
            "eta": "0.82",
            "gamma": "0.55",
            "lambda_reclose": ["19", "50"],
            "ticks": [
                {"lead": "0.8", "lag": ["1", "5"], "compatibility": "0.55"}
            ]
        }"#,
    )
    .unwrap();
    let mixed_out = eval_operation("cycle_shedding_trace", mixed_in.as_object().unwrap()).unwrap();
    let tick0 = &mixed_out.get("ticks").unwrap().as_array().unwrap()[0];
    let tkeys: Vec<&str> = tick0.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        tkeys,
        vec![
            "before_clipping",
            "local_reclosure",
            "outward_shedding",
            "padar_burden",
            "remainder_sum",
            "shed_excess",
            "span",
        ]
    );
    assert_eq!(
        tick0.get("outward_shedding").unwrap().to_compact_ascii_json(),
        r#"["589","5000"]"#
    );
    assert_eq!(
        tick0.get("local_reclosure").unwrap().to_compact_ascii_json(),
        r#"["361","5000"]"#
    );
    assert_eq!(
        mixed_out.get("final_burden").unwrap().to_compact_ascii_json(),
        r#"["11","20"]"#
    );

    // 3. Guard `ticks.len() > MAX_CYCLE_SHEDDING_TICKS` rejected cleanly
    let zero = Ratio::zero();
    let too_many = vec![
        CycleSheddingTickInput {
            lead: Ratio::zero(),
            lag: Ratio::zero(),
            compatibility: Ratio::one(),
        };
        MAX_CYCLE_SHEDDING_TICKS + 1
    ];
    assert!(cycle_shedding_trace(&zero, &too_many, &zero, &zero, &zero).is_err());
}

// ============================================================================
// AC-03 / P0-4: Strict 90-Tick CSV Verifier & Hostile Negative Controls
// ============================================================================

#[test]
fn ac03_verify_s4_cycle_shedding_csv_and_hostile_tamper_controls() {
    let root = repo_root();
    let csv_path = root.join(S4_CYCLE_SHEDDING_CSV_REL);
    let binding_path = root.join("SOURCE_BINDING.json");

    // 1. Live vendored CRLF CSV passes every exact identity & tolerance check
    let rep = verify_s4_cycle_shedding_csv(&csv_path, &binding_path);
    assert!(rep.is_ok(), "vendored S4 CSV failed: {:?}", rep.errors);
    assert_eq!(rep.binding_id, S4_BINDING_ID);
    assert_eq!(rep.binding_status, "DRAFT_CANDIDATE_UNDER_S4_REVIEW");
    assert_eq!(rep.base_commit_binding, S4_BASE_COMMIT_BINDING);
    assert_eq!(
        rep.fixture_sha256,
        "d7b0592997d47b2b550508d663afe8943a98ba9d58d087ce77e9d4d0a0c3ff40"
    );
    assert_eq!(rep.ticks_verified, 90);
    assert_eq!(rep.shedding_ticks, 11);
    assert_eq!(rep.shedding_tick_first, Some(29));
    assert_eq!(rep.shedding_tick_last, Some(39));
    assert!(rep.zero_residual_all_ticks);
    assert!(rep.within_csv_tolerance_all_ticks);
    assert!(
        rep.max_denominator_digits > 18,
        "expected BigInt denominator growth > 18 digits, got {}",
        rep.max_denominator_digits
    );

    let tmp = TempDirGuard::new("csv_hostile");
    let raw_bytes = std::fs::read(&csv_path).unwrap();
    assert_eq!(raw_bytes.len(), 11245);

    // 2. LF-stripped CSV fails SHA-256 provenance check
    let lf_csv = tmp.path().join("lf.csv");
    let lf_bytes: Vec<u8> = raw_bytes.iter().copied().filter(|&b| b != b'\r').collect();
    std::fs::write(&lf_csv, &lf_bytes).unwrap();
    let rep_lf = verify_s4_cycle_shedding_csv(&lf_csv, &binding_path);
    assert!(!rep_lf.is_ok());
    assert!(rep_lf.errors.iter().any(|e| e.contains("SHA-256 mismatch")));

    // 3. Derived-column drift (> 1/10^14) fails tolerance check even if binding matches tampered hash
    let tampered_text = String::from_utf8(raw_bytes.clone())
        .unwrap()
        .replacen("0.0049177502944412034", "0.0059177502944412034", 1);
    let tampered_csv = tmp.path().join("tampered.csv");
    std::fs::write(&tampered_csv, tampered_text.as_bytes()).unwrap();
    let rep_tampered = verify_s4_cycle_shedding_csv(&tampered_csv, &binding_path);
    assert!(!rep_tampered.is_ok());
    assert!(!rep_tampered.within_csv_tolerance_all_ticks);
    assert!(rep_tampered
        .errors
        .iter()
        .any(|e| e.contains("exceeds 1/10^14 tolerance")));

    // 4. Truncated 89-row CSV fails row count check
    let mut lines: Vec<&str> = std::str::from_utf8(&raw_bytes).unwrap().lines().collect();
    lines.pop();
    let short_csv = tmp.path().join("short.csv");
    std::fs::write(&short_csv, lines.join("\r\n") + "\r\n").unwrap();
    let rep_short = verify_s4_cycle_shedding_csv(&short_csv, &binding_path);
    assert!(!rep_short.is_ok());
    assert!(rep_short.errors.iter().any(|e| e.contains("90 CSV data rows")));

    // 5. Symlink CSV rejected
    let link_csv = tmp.path().join("link.csv");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&csv_path, &link_csv).unwrap();
    let rep_sym = verify_s4_cycle_shedding_csv(&link_csv, &binding_path);
    assert!(!rep_sym.is_ok());
    assert!(rep_sym.errors.iter().any(|e| e.contains("symlink forbidden")));

    // 6. Oversized (> 16 MiB) sparse CSV rejected without OOM
    let big_csv = tmp.path().join("big.csv");
    {
        let f = File::create(&big_csv).unwrap();
        f.set_len((MAX_S4_INPUT_BYTES as u64) + 1024).unwrap();
    }
    let rep_big = verify_s4_cycle_shedding_csv(&big_csv, &binding_path);
    assert!(!rep_big.is_ok());
    assert!(rep_big.errors.iter().any(|e| e.contains("16 MiB limit")));
}

// ============================================================================
// AC-04 / P0-4: Portable `aod.s4.rust.report.v1` Schema & Pre-Validation Sanitizer
// ============================================================================

#[test]
fn ac04_portable_s4_report_schema_20_forbidden_keys_and_pre_validation_sanitization() {
    // 1. Live report matches upstream top-level keys + preserves RC-020 quarantine status
    let (val, pretty, passed) = verify_s4_convergence_dir(None, None, None).unwrap();
    assert!(passed, "live S4 convergence report failed:\n{pretty}");
    assert!(pretty.ends_with('\n'));
    validate_portable_report(&val, "$").unwrap();

    let obj = val.as_object().unwrap();
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec![
            "base_commit_binding",
            "crate_name",
            "crate_version",
            "cycle_shedding_fixture",
            "overall_status",
            "policy_audit",
            "schema",
            "vectors_verification",
        ]
    );
    assert_eq!(val.get("schema").and_then(JsonValue::as_str), Some(S4_REPORT_SCHEMA));
    assert_eq!(
        val.get("base_commit_binding").and_then(JsonValue::as_str),
        Some(S4_BASE_COMMIT_BINDING)
    );
    assert_eq!(val.get("overall_status").and_then(JsonValue::as_str), Some("PASS"));

    let vec_sec = val.get("vectors_verification").unwrap();
    assert_eq!(vec_sec.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(
        vec_sec
            .get("scientific_status_summary")
            .and_then(|s| s.get("UNRESOLVED_PROVENANCE_QUARANTINED"))
            .and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(1))
    );

    // 2. Reject ALL 20 forbidden report keys
    assert_eq!(FORBIDDEN_REPORT_KEYS.len(), 20);
    for key in FORBIDDEN_REPORT_KEYS {
        let mut m = BTreeMap::new();
        m.insert(key.to_string(), JsonValue::from_str_val("x"));
        assert!(
            validate_portable_report(&JsonValue::Object(m), "$").is_err(),
            "forbidden report key {key:?} must be rejected"
        );
    }

    // 3. Reject host paths, temp markers, and ISO timestamps; verify sanitize_error scrubs all of them
    for bad in [
        "/home/alice/secret.rs",
        "/tmp/work/out.json",
        "C:\\Users\\alice\\x.json",
        "pytest-of-bob/run",
        "aod-anchor-scientific-replay-123",
        "failed at 2026-11-24T09:15:00Z",
    ] {
        assert!(
            validate_portable_report(&JsonValue::from_str_val(bad), "$").is_err(),
            "leak {bad:?} must be rejected"
        );
        let scrubbed = sanitize_error(bad, &[]);
        assert!(
            validate_portable_report(&JsonValue::from_str_val(scrubbed.clone()), "$").is_ok(),
            "sanitized string {scrubbed:?} still failed portable check"
        );
    }

    // 4. Failing S4 report run still emits clean portable FAIL JSON (`P0-4` pre-validation sanitization!)
    let tmp = TempDirGuard::new("fail_portable");
    let missing_csv = tmp.path().join("missing_fixture.csv");
    let out_report = tmp.path().join("fail_report.json");
    let run = Command::new(aodc_bin())
        .current_dir(repo_root())
        .args([
            "verify-s2",
            "--s4-report",
            "--fixture",
            missing_csv.to_str().unwrap(),
            "--report",
            out_report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(1), "missing fixture must exit 1");
    let text = std::fs::read_to_string(&out_report).unwrap();
    let parsed = parse_strict_json(&text).unwrap();
    validate_portable_report(&parsed, "$").unwrap();
    assert_eq!(parsed.get("overall_status").and_then(JsonValue::as_str), Some("FAIL"));
    assert_eq!(
        parsed
            .get("cycle_shedding_fixture")
            .and_then(|c| c.get("status"))
            .and_then(JsonValue::as_str),
        Some("FAIL")
    );
}

// ============================================================================
// AC-05, AC-06, P0-3, P1-2: `aodc eval-op` Modes, Exit Codes & Pre-Exec Order
// ============================================================================

#[test]
fn ac05_ac06_eval_op_cli_modes_exit_codes_and_pre_exec_report_ordering() {
    let tmp = TempDirGuard::new("eval_op_cli");
    let rc14_input = r#"{"prev_burden":["1","2"],"lead":["4","5"],"lag":["1","5"],"compatibility":["11","20"],"eta":["41","50"],"gamma":["11","20"],"lambda_reclose":["19","50"]}"#;

    // 1. `--input-json` -> exit 0, pretty JSON
    let out_inline = Command::new(aodc_bin())
        .args(["eval-op", "cycle_shedding_step", "--input-json", rc14_input])
        .output()
        .unwrap();
    assert_eq!(out_inline.status.code(), Some(0));
    let inline_text = String::from_utf8(out_inline.stdout).unwrap();
    assert!(inline_text.ends_with('\n'));
    let parsed_inline = parse_strict_json(&inline_text).unwrap();
    assert_eq!(
        parsed_inline.get("outward_shedding").unwrap().to_compact_ascii_json(),
        r#"["589","5000"]"#
    );

    // 2. `--input <file>` + `--report <path>` -> byte-identical pretty JSON written atomically
    let in_file = tmp.path().join("in.json");
    std::fs::write(&in_file, rc14_input).unwrap();
    let rep_file = tmp.path().join("nested").join("out.json");
    let out_file = Command::new(aodc_bin())
        .args([
            "eval-op",
            "cycle_shedding_step",
            "--input",
            in_file.to_str().unwrap(),
            "--report",
            rep_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out_file.status.code(), Some(0));
    assert!(out_file.stdout.is_empty());
    let file_text = std::fs::read_to_string(&rep_file).unwrap();
    assert_eq!(file_text, inline_text);

    // 3. `--input-stdin` -> byte-identical pretty JSON
    let mut child = Command::new(aodc_bin())
        .args(["eval-op", "cycle_shedding_step", "--input-stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(rc14_input.as_bytes())
        .unwrap();
    let out_stdin = child.wait_with_output().unwrap();
    assert_eq!(out_stdin.status.code(), Some(0));
    assert_eq!(String::from_utf8(out_stdin.stdout).unwrap(), inline_text);

    // 4. Eval failure (float in JSON, unknown op, symlink `--input`, >16 MiB `--input`) exits 1
    //    AND NEVER creates `--report` or its parent directory (`P0-3`)!
    let never_dir = tmp.path().join("must_not_exist_dir");
    let never_file = never_dir.join("never.json");

    let fail_float = Command::new(aodc_bin())
        .args([
            "eval-op",
            "cycle_shedding_step",
            "--input-json",
            r#"{"prev_burden": 0.5}"#,
            "--report",
            never_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(fail_float.status.code(), Some(1));
    assert!(!never_file.exists());
    assert!(!never_dir.exists());

    let fail_unknown_op = Command::new(aodc_bin())
        .args([
            "eval-op",
            "nonexistent_op",
            "--input-json",
            "{}",
            "--report",
            never_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(fail_unknown_op.status.code(), Some(1));
    assert!(!never_dir.exists());

    let sym_in = tmp.path().join("sym_in.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&in_file, &sym_in).unwrap();
    let fail_sym = Command::new(aodc_bin())
        .args(["eval-op", "cycle_shedding_step", "--input", sym_in.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(fail_sym.status.code(), Some(1));

    let big_in = tmp.path().join("big_in.json");
    {
        let f = File::create(&big_in).unwrap();
        f.set_len((MAX_S4_INPUT_BYTES as u64) + 1024).unwrap();
    }
    let fail_big = Command::new(aodc_bin())
        .args(["eval-op", "cycle_shedding_step", "--input", big_in.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(fail_big.status.code(), Some(1));

    // 5. CLI usage errors -> exact exit code 2 (`P0-3`)
    for bad_args in [
        vec!["eval-op"],
        vec!["eval-op", "cycle_shedding_step"],
        vec![
            "eval-op",
            "cycle_shedding_step",
            "--input-json",
            "{}",
            "--input-stdin",
        ],
        vec!["eval-op", "cycle_shedding_step", "--unknown", "{}"],
        vec![
            "eval-op",
            "cycle_shedding_step",
            "--input-json",
            "{}",
            "--report",
        ],
    ] {
        let out = Command::new(aodc_bin()).args(&bad_args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "expected exit code 2 for {:?}, got {:?}",
            bad_args,
            out.status.code()
        );
    }

    // 6. Pre-execution `--report` validation runs BEFORE input JSON parsing -> exit 2 even on bad JSON!
    let unsafe_target = repo_root().join("README.md");
    let pre_exec = Command::new(aodc_bin())
        .current_dir(std::env::temp_dir())
        .args([
            "eval-op",
            "nonexistent_op",
            "--input-json",
            "{invalid json",
            "--report",
            unsafe_target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        pre_exec.status.code(),
        Some(2),
        "unsafe --report must fail with exit 2 BEFORE input parsing/eval"
    );
}

// ============================================================================
// AC-07, AC-08, P1-1: Ancestor Repo-Root Discovery, Symlinks & Atomic Write
// ============================================================================

#[test]
fn ac07_ac08_report_guard_ancestor_repo_roots_symlinks_and_atomic_write() {
    let root = repo_root();
    let tmp = TempDirGuard::new("report_guard");

    // 1. Invoked from `/tmp`, targeting `<repo_root>/README.md`, `<repo_root>/crates/x.json`,
    //    `<repo_root>/_build` (1 component), or `<repo_root>/target/x.json` MUST exit 2!
    for rel in ["README.md", "crates/evil.json", "bindings/evil.json", "_build", "target/evil.json"] {
        let target = root.join(rel);
        let out = Command::new(aodc_bin())
            .current_dir(std::env::temp_dir())
            .args([
                "verify-s2",
                "--s4-report",
                "--report",
                target.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "targeting repo path {rel:?} from /tmp must exit 2, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // 2. Allowlisted in-repo `_build/<subpath>` succeeds and leaves zero orphan `.tmp` files
    let in_repo_dir = root.join("_build").join(format!("pr8_guard_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&in_repo_dir);
    let in_repo_out = in_repo_dir.join("ok.json");
    let ok_in_repo = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "eval-op",
            "cycle_shedding_trace",
            "--input-json",
            r#"{"initial_burden":"0","eta":"0.82","gamma":"0.55","lambda_reclose":"0.38","ticks":[]}"#,
            "--report",
            in_repo_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        ok_in_repo.status.code(),
        Some(0),
        "_build subpath must succeed: {}",
        String::from_utf8_lossy(&ok_in_repo.stderr)
    );
    assert!(in_repo_out.is_file());

    // 3. Parent symlink escape inside `_build/` (`_build/.../escape -> /tmp`) rejected BEFORE resolution -> exit 2!
    let escape_link = in_repo_dir.join("escape_to_tmp");
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path(), &escape_link).unwrap();
    let escape_target = escape_link.join("escaped.json");
    let escape_run = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "verify-s2",
            "--s4-report",
            "--report",
            escape_target.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        escape_run.status.code(),
        Some(2),
        "parent symlink escape inside _build must exit 2"
    );
    assert!(!tmp.path().join("escaped.json").exists());

    // Clean up `_build/pr8_guard_*` (and remove `_build` if empty)
    let _ = std::fs::remove_dir_all(&in_repo_dir);
    let _ = std::fs::remove_dir(root.join("_build"));

    // 4. Target symlink & dangling target symlink in `/tmp` rejected with exit 2
    let real_target = tmp.path().join("real.json");
    std::fs::write(&real_target, b"{}").unwrap();
    let live_link = tmp.path().join("live_link.json");
    let dangling_link = tmp.path().join("dangling_link.json");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real_target, &live_link).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("nonexistent_leaf.json"), &dangling_link)
            .unwrap();
    }
    for link in [&live_link, &dangling_link] {
        let out = Command::new(aodc_bin())
            .args([
                "verify-s2",
                "--s4-report",
                "--report",
                link.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "symlink target {:?} must exit 2", link);
    }

    // 5. Existing directory and regular-file parent rejected
    assert!(validate_runtime_output_path(tmp.path(), &[]).is_err());
    assert!(validate_runtime_output_path(&real_target.join("child.json"), &[]).is_err());

    // 6. Atomic write leaves zero orphan `.tmp` files
    let atomic_out = tmp.path().join("atomic.json");
    let validated = validate_runtime_output_path(&atomic_out, &[]).unwrap();
    write_report_atomic(&validated, b"{\"ok\":true}\n").unwrap();
    assert_eq!(std::fs::read_to_string(&atomic_out).unwrap(), "{\"ok\":true}\n");
    let tmp_orphans: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp."))
        .collect();
    assert!(tmp_orphans.is_empty(), "orphan temp files found: {:?}", tmp_orphans);
}

// ============================================================================
// AC-09 / P1-2: `verify-s2` Backward Compat & `aod.s4.rust.report.v1` Determinism
// ============================================================================

#[test]
fn ac09_verify_s2_backward_compat_and_s4_report_determinism() {
    let root = repo_root();
    let tmp = TempDirGuard::new("verify_s2_s4");

    // 1. Backward-compatible `aodc verify-s2 --json` (without `--s4-report`)
    let legacy = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["verify-s2", "--json"])
        .output()
        .unwrap();
    assert_eq!(legacy.status.code(), Some(0));
    let legacy_json = parse_strict_json(std::str::from_utf8(&legacy.stdout).unwrap()).unwrap();
    assert_eq!(
        legacy_json.get("schema").and_then(JsonValue::as_str),
        Some("aod.dec.s2.verify.v1")
    );

    // 2. `--fixture` without `--s4-report` -> exit 2
    let bad_combo = Command::new(aodc_bin())
        .current_dir(&root)
        .args(["verify-s2", "--fixture", S4_CYCLE_SHEDDING_CSV_REL])
        .output()
        .unwrap();
    assert_eq!(bad_combo.status.code(), Some(2));

    // 3. Double-run `verify-s2 --s4-report --report` produces byte-identical canonical JSON
    let r1 = tmp.path().join("r1.json");
    let r2 = tmp.path().join("r2.json");
    for rp in [&r1, &r2] {
        let out = Command::new(aodc_bin())
            .current_dir(&root)
            .args(["verify-s2", "--s4-report", "--report", rp.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "verify-s2 --s4-report failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let b1 = std::fs::read(&r1).unwrap();
    let b2 = std::fs::read(&r2).unwrap();
    assert_eq!(b1, b2, "S4 reports must be byte-for-byte deterministic");
}

// ============================================================================
// AC-10 / P1-3: Pure-Dec 90-Tick Fixture Exact CSV Parity + 5-Mode & Hermetic IR
// ============================================================================

#[test]
fn ac10_pure_dec_90tick_fixture_exact_csv_parity_and_5mode_hermetic_ir() {
    let root = repo_root();
    let csv_text = std::fs::read_to_string(root.join(S4_CYCLE_SHEDDING_CSV_REL)).unwrap();

    // 1. Compute expected 90-tick trace independently from vendored CSV via `aod_core::s2_kernel`
    let mut tick_inputs = Vec::with_capacity(90);
    for line in csv_text.lines().skip(1) {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.is_empty() {
            continue;
        }
        let cols: Vec<&str> = trimmed.split(',').collect();
        assert_eq!(cols.len(), 10);
        tick_inputs.push(CycleSheddingTickInput {
            lead: parse_printed_rational(cols[1], false, false).unwrap(),
            lag: parse_printed_rational(cols[2], false, false).unwrap(),
            compatibility: parse_printed_rational(cols[4], false, false).unwrap(),
        });
    }
    assert_eq!(tick_inputs.len(), 90);

    let eta = Ratio::from_i64(41, 50).unwrap();
    let gamma = Ratio::from_i64(11, 20).unwrap();
    let lam = Ratio::from_i64(19, 50).unwrap();
    let trace = cycle_shedding_trace(&Ratio::zero(), &tick_inputs, &eta, &gamma, &lam).unwrap();

    let mut max_span = Ratio::zero();
    let mut max_burden = Ratio::zero();
    let mut max_excess = Ratio::zero();
    let mut total_outward = Ratio::zero();
    let mut total_local = Ratio::zero();
    let mut shedding_ticks = 0usize;
    let mut first_shed: Option<usize> = None;
    let mut last_shed: Option<usize> = None;

    for (idx, step) in trace.iter().enumerate() {
        if step.span > max_span {
            max_span = step.span.clone();
        }
        if step.padar_burden > max_burden {
            max_burden = step.padar_burden.clone();
        }
        if step.shed_excess > max_excess {
            max_excess = step.shed_excess.clone();
        }
        total_outward = &total_outward + &step.outward_shedding;
        total_local = &total_local + &step.local_reclosure;
        if step.shed_excess.is_positive() {
            shedding_ticks += 1;
            if first_shed.is_none() {
                first_shed = Some(idx);
            }
            last_shed = Some(idx);
        }
    }
    let final_burden = &trace.last().unwrap().padar_burden;
    let expected_stdout = format!(
        "90\n{shedding_ticks}\n{}\n{}\ntrue\n{}\n{}\n{}\n{}\n{}\n{}\n0\n",
        first_shed.unwrap(),
        last_shed.unwrap(),
        canonical_rational_str(final_burden),
        canonical_rational_str(&max_span),
        canonical_rational_str(&max_burden),
        canonical_rational_str(&max_excess),
        canonical_rational_str(&total_outward),
        canonical_rational_str(&total_local),
    );

    let dec_path = root.join("manual_examples/10_s4_cycle_shedding_90tick.dec");
    let disk_stdout =
        std::fs::read_to_string(root.join("manual_examples/10_s4_cycle_shedding_90tick.stdout"))
            .unwrap();
    assert_eq!(
        disk_stdout, expected_stdout,
        "10_s4_cycle_shedding_90tick.stdout drifted from aod-core CSV oracle!"
    );

    // 2. Exact VM parity within DEFAULT_FUEL = 100_000
    let vm_out = run_file(&dec_path, CompatMode::V2)
        .unwrap()
        .to_stdout_string();
    assert_eq!(vm_out, expected_stdout);

    // 3. AOT -O0 and AOT -O2 parity
    let tmp = TempDirGuard::new("dec10_5mode");
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.path().join(format!("dec10_{tag}"));
        build_aot_file(&dec_path, CompatMode::V2, opt, &exe).unwrap();
        let out = Command::new(&exe).output().unwrap();
        assert!(
            out.status.success(),
            "AOT -{tag} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected_stdout);
    }

    // 4. ORC LLJIT -O0 and -O2 parity within DEFAULT_FUEL
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let (stdout, res) =
            jit_file_captured(&dec_path, CompatMode::V2, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .unwrap();
        res.unwrap_or_else(|e| panic!("JIT -{tag} runtime error: {e}"));
        assert_eq!(stdout, expected_stdout);
    }

    // 5. Hermetic DecIR v2 parity
    let ir_json = compile_file_to_ir(&dec_path, CompatMode::V2).unwrap();
    let checked = loads_ir(&ir_json).unwrap();
    let ir_out = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap()
        .to_stdout_string();
    assert_eq!(ir_out, expected_stdout);

    // 6. Verify mismatched array lengths in `cycle_shedding_trace_summary` fail cleanly
    let bad_dec = tmp.path().join("bad_len.dec");
    std::fs::write(
        &bad_dec,
        "use stdlib::aod::sectors::cycle_shedding_trace_summary\n\
         Dec main() -> int:\n\
             let _ = cycle_shedding_trace_summary(0/1r, [1/2r, 1/3r], [1/4r], [1/2r, 1/2r], 41/50r, 11/20r, 19/50r)\n\
             return 0\n",
    )
    .unwrap();
    assert!(run_file(&bad_dec, CompatMode::V2).is_err());
}
