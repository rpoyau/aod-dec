use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::s4_report::validate_portable_report;
use aodc::codegen::lower_to_llvm_ir_with_plan;
use aodc::diagnostics::CompatMode;
use aodc::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::loads_ir;
use aodc::{
    build_aot_file, check_source, compile_file_to_ir, emit_llvm_source_with_plan,
    jit_file_captured, jit_source_with_plan_captured, recommend_candidates, run_file,
    validate_manual_data_output_dir, BootstrapCi, CodegenPlan, HostHardwareInfo, HwPlanHints,
    LearnDecision, OptLevel, ProfileLoadOutcome, RetainedProfile, UnrollHint, MANUAL_DATA_SCHEMA,
    POLICY_SCOPE, PROFILE_VERSION,
};

static PR9_TEST_SEQ: AtomicU64 = AtomicU64::new(0);

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
        let seq = PR9_TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aodc_pr9_{}_{}_{tag}",
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
// Pillar 1: `stdlib/aod/apparatus.dec` & `11_apparatus_opposing_sensors.dec`
// ============================================================================

#[test]
fn p1_apparatus_11_opposing_sensors_6way_parity_and_firewall() {
    let root = repo_root();
    let app_src = std::fs::read_to_string(root.join("stdlib/aod/apparatus.dec")).unwrap();
    let ex11_path = root.join("manual_examples/11_apparatus_opposing_sensors.dec");
    let ex11_src = std::fs::read_to_string(&ex11_path).unwrap();
    let expected_stdout = std::fs::read_to_string(
        root.join("manual_examples/11_apparatus_opposing_sensors.stdout"),
    )
    .unwrap();

    // 1. Strict unbiased & zero-Q_ext hygiene on apparatus module & fixture
    for (label, text) in [("apparatus.dec", &app_src), ("fixture 11", &ex11_src)] {
        assert!(
            !text.contains("quarantine Q_ext"),
            "{label} must not contain quarantine Q_ext"
        );
        for forbidden in ["GeV", "MeV", "938.272", "0.510998", "125.1"] {
            assert!(
                !text.contains(forbidden),
                "{label} must not embed hardcoded physical target {forbidden:?}"
            );
        }
    }
    assert!(expected_stdout.contains("DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"));

    // 2. Exact VM sequential parity
    let vm_out = run_file(&ex11_path, CompatMode::V2)
        .unwrap()
        .to_stdout_string();
    assert_eq!(vm_out, expected_stdout);

    // 3. Exact VM `--parallel-observers` parity
    let vm_par = Command::new(aodc_bin())
        .args(["run", "--parallel-observers", ex11_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        vm_par.status.success(),
        "aodc run --parallel-observers failed: {}",
        String::from_utf8_lossy(&vm_par.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&vm_par.stdout), expected_stdout);

    // 4. AOT -O0 and AOT -O2 parity
    let tmp = TempDirGuard::new("ex11_aot");
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let exe = tmp.path().join(format!("ex11_{tag}"));
        build_aot_file(&ex11_path, CompatMode::V2, opt, &exe).unwrap();
        let out = Command::new(&exe).output().unwrap();
        assert!(
            out.status.success(),
            "AOT -{tag} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected_stdout);
    }

    // 5. ORC LLJIT -O0, -O2, and `--parallel-observers` parity
    for (opt, tag) in [(OptLevel::O0, "o0"), (OptLevel::O2, "o2")] {
        let (stdout, res) =
            jit_file_captured(&ex11_path, CompatMode::V2, opt, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .unwrap();
        res.unwrap_or_else(|e| panic!("JIT -{tag} runtime error: {e}"));
        assert_eq!(stdout, expected_stdout);
    }
    let jit_par = Command::new(aodc_bin())
        .args(["jit", "-O2", "--parallel-observers", ex11_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        jit_par.status.success(),
        "aodc jit --parallel-observers failed: {}",
        String::from_utf8_lossy(&jit_par.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&jit_par.stdout), expected_stdout);

    // 6. Hermetic DecIR v2 parity (within DEFAULT_FUEL = 100_000)
    let ir_json = compile_file_to_ir(&ex11_path, CompatMode::V2).unwrap();
    let checked = loads_ir(&ir_json).unwrap();
    let ir_out = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .run_main()
        .unwrap()
        .to_stdout_string();
    assert_eq!(ir_out, expected_stdout);

    // 7. Compile-time observer firewall still rejects non-ReadOnly `ChamberView`
    let hostile_obs = r#"
chamber C:
    space = box([0..16, 0..16, 0..16])
    walls = open
prepare P in C:
    inject 3:3:6 at (4, 8, 8) dir +X orient 1t shells 1
observe Bad:
    tracer mutator:
        (view: ChamberView) -> Null:
            return null
dec main:
    () -> int:
        let _r = simulate P for 2 ticks with Bad
        return 0
"#;
    assert!(check_source(hostile_obs, CompatMode::V2).is_err());
}

// ============================================================================
// Pillar 2: Pure-Rust Zero-LLVM `aodc manual-data` & Dedicated Dir Guard (`P0-3`)
// ============================================================================

#[test]
fn p2_manual_data_vm_generation_check_drift_symlink_and_path_guard() {
    let root = repo_root();

    // 1. Static zero-LLVM proof on `crates/aodc/src/manual_data.rs`
    let md_src = std::fs::read_to_string(root.join("crates/aodc/src/manual_data.rs")).unwrap();
    assert!(
        !md_src.contains("aod_llvm"),
        "manual_data.rs must have zero aod_llvm imports/calls"
    );

    // 2. Live `docs/manual/data` passes `--check --json` EVEN with broken `DEC_LLVM_LIBRARY`!
    let live_check = Command::new(aodc_bin())
        .current_dir(&root)
        .env("DEC_LLVM_LIBRARY", "/nonexistent/libLLVM-99.so")
        .args(["manual-data", "--check", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        live_check.status.code(),
        Some(0),
        "live manual-data --check failed: {}",
        String::from_utf8_lossy(&live_check.stderr)
    );
    let check_json_str = String::from_utf8(live_check.stdout).unwrap();
    let check_val = parse_strict_json(check_json_str.trim()).unwrap();
    validate_portable_report(&check_val, "$").unwrap();
    assert_eq!(
        check_val.get("schema").and_then(JsonValue::as_str),
        Some(MANUAL_DATA_SCHEMA)
    );
    assert_eq!(check_val.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(check_val.get("mode").and_then(JsonValue::as_str), Some("check"));
    assert_eq!(
        check_val.get("files_checked").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(3))
    );
    assert_eq!(
        check_val.get("files_changed").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(0))
    );

    // 3. Write to outside-repo temp dir -> zero `.tmp` orphans -> `--check` PASS -> drift FAIL -> symlink FAIL
    let tmp = TempDirGuard::new("manual_data");
    let out_dir = tmp.path().join("gen_data");

    // 3a. `--check` on missing dir does NOT create dir and exits 1
    let missing_chk = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "manual-data",
            "--check",
            "--output-dir",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(missing_chk.status.code(), Some(1));
    assert!(!out_dir.exists(), "--check must never create missing --output-dir");
    let missing_val = parse_strict_json(std::str::from_utf8(&missing_chk.stdout).unwrap().trim()).unwrap();
    validate_portable_report(&missing_val, "$").unwrap();
    assert_eq!(missing_val.get("status").and_then(JsonValue::as_str), Some("FAIL"));

    // 3b. Write mode generates all 3 files atomically with zero `.tmp` orphans
    let write_run = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "manual-data",
            "--output-dir",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(write_run.status.code(), Some(0));
    let orphans: Vec<_> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp."))
        .collect();
    assert!(orphans.is_empty(), "orphan temp files found: {:?}", orphans);

    // 3c. `--check` passes byte-for-byte
    let ok_chk = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "manual-data",
            "--check",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(ok_chk.status.code(), Some(0));

    // 3d. 1-byte drift fails `--check` with exit 1 and portable JSON
    let disk_json = out_dir.join("disk_enclosure.json");
    let orig = std::fs::read_to_string(&disk_json).unwrap();
    std::fs::write(&disk_json, format!("{orig} ")).unwrap();
    let drift_chk = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "manual-data",
            "--check",
            "--output-dir",
            out_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(drift_chk.status.code(), Some(1));
    let drift_val = parse_strict_json(std::str::from_utf8(&drift_chk.stdout).unwrap().trim()).unwrap();
    validate_portable_report(&drift_val, "$").unwrap();
    assert_eq!(
        drift_val.get("files_changed").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(1))
    );

    // 3e. Symlinked data file fails `--check` with exit 1
    let real_copy = tmp.path().join("real_disk.json");
    std::fs::write(&real_copy, &orig).unwrap();
    std::fs::remove_file(&disk_json).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_copy, &disk_json).unwrap();
    let sym_chk = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "manual-data",
            "--check",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(sym_chk.status.code(), Some(1));

    // 3f. AC-04: Write mode with a symlinked target file MUST exit 2 BEFORE writing ANY sibling file!
    let sym_write_dir = tmp.path().join("sym_write_dir");
    std::fs::create_dir_all(&sym_write_dir).unwrap();
    let sym_write_target = tmp.path().join("outside_victim.json");
    std::fs::write(&sym_write_target, b"untouched").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        &sym_write_target,
        sym_write_dir.join("double_slit_delta3.json"),
    )
    .unwrap();
    let sym_write = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "manual-data",
            "--output-dir",
            sym_write_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        sym_write.status.code(),
        Some(2),
        "write mode with target symlink must exit 2, stderr: {}",
        String::from_utf8_lossy(&sym_write.stderr)
    );
    assert!(
        !sym_write_dir.join("disk_enclosure.json").exists(),
        "disk_enclosure.json must NOT be written when double_slit_delta3.json is a symlink"
    );
    assert!(
        !sym_write_dir.join("sandbox_chamber_summary.json").exists(),
        "sandbox_chamber_summary.json must NOT be written when double_slit_delta3.json is a symlink"
    );
    assert_eq!(
        std::fs::read_to_string(&sym_write_target).unwrap(),
        "untouched"
    );

    // 4. Dedicated output-dir guard (`P0-3`): disallowed in-repo dirs, symlinks, and regular files exit 2!
    let regular_file = tmp.path().join("not_a_dir.txt");
    std::fs::write(&regular_file, b"hi").unwrap();
    let dir_symlink = tmp.path().join("dir_link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&out_dir, &dir_symlink).unwrap();

    for bad_dir in [
        root.clone(),
        root.join(".git"),
        root.join("crates"),
        root.join("bindings"),
        root.join("stdlib"),
        root.join("SOURCE_BINDING.json"),
        root.join("docs/manual"),
        root.join("_build"),
        regular_file,
        dir_symlink,
    ] {
        let out = Command::new(aodc_bin())
            .current_dir(std::env::temp_dir())
            .args([
                "manual-data",
                "--output-dir",
                bad_dir.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "disallowed --output-dir {:?} must exit 2, stderr: {}",
            bad_dir,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // 5. Verify `<repo>/docs/manual/data` and `<repo>/_build/sub` pass guard validation
    assert!(validate_manual_data_output_dir(&root.join("docs/manual/data"), &[root.clone()]).is_ok());
    assert!(validate_manual_data_output_dir(&root.join("_build/pr9_sub"), &[root.clone()]).is_ok());
}

// ============================================================================
// Pillar 3A/3B: `CodegenPlan` v2, Combined `!llvm.loop`, Legacy v1 Fallback & SoA Bench
// ============================================================================

#[test]
fn p3_codegen_plan_v2_loop_metadata_legacy_v1_fallback_and_soa_bench() {
    // 1. `CodegenPlan` v2 validation & round-trip JSON
    for bad_ic in [1u8, 3, 8, 255] {
        let bad = CodegenPlan {
            interleave_count: bad_ic,
            ..CodegenPlan::baseline_o2()
        };
        assert!(bad.validate().is_err(), "interleave_count={bad_ic} must fail validate()");

        let mut bad_json_val = bad.to_json();
        if let JsonValue::Object(ref mut m) = bad_json_val {
            m.insert("interleave_count".to_string(), JsonValue::from_i64(bad_ic as i64));
        }
        assert!(
            CodegenPlan::from_json(&bad_json_val).is_err(),
            "interleave_count={bad_ic} must fail from_json()"
        );

        let checked = check_source("dec main:\n    () -> int:\n        return 0\n", CompatMode::V2).unwrap();
        assert!(
            lower_to_llvm_ir_with_plan(&checked, &bad).is_err(),
            "interleave_count={bad_ic} must fail lower_to_llvm_ir_with_plan()"
        );
    }

    for good_ic in [0u8, 2, 4] {
        let p = CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: true,
            unroll_hint: UnrollHint::Unroll4,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            vectorize_loops: true,
            interleave_count: good_ic,
            layout_prefer_soa: true,
            parallel_observers: true,
        };
        p.validate().unwrap();
        let rt = CodegenPlan::from_json(&p.to_json()).unwrap();
        assert_eq!(rt, p);
    }

    // 2. Combined single `!llvm.loop` node (`P0-4`): unroll + vectorize + interleave in ONE distinct node!
    let loop_src = r#"
use aod::core::evolve

dec add_step:
    (acc: int, i: int) -> int:
        return acc + i

dec main:
    () -> int:
        let acc = evolve(0, 0..64, add_step)
        emit acc
        return 0
"#;
    let checked = check_source(loop_src, CompatMode::V2).unwrap();

    // Baseline plan emits ZERO `!llvm.loop` metadata
    let base_ir = lower_to_llvm_ir_with_plan(&checked, &CodegenPlan::baseline_o2()).unwrap();
    assert!(!base_ir.contains("!llvm.loop"), "baseline IR must emit zero !llvm.loop metadata");

    // Combined v2 plan emits ONE distinct self-referential node containing all 3 hints and verifies in LLVM 18!
    let combined_plan = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: false,
        unroll_hint: UnrollHint::Unroll4,
        target_cpu_native: true,
        inline_ratio_fastpath: true,
        vectorize_loops: true,
        interleave_count: 4,
        layout_prefer_soa: true,
        parallel_observers: false,
    };
    let comb_ir = lower_to_llvm_ir_with_plan(&checked, &combined_plan).unwrap();
    assert!(comb_ir.contains("distinct !{!0, !1, !2, !3}"), "expected single combined loop md node:\n{comb_ir}");
    assert!(comb_ir.contains("llvm.loop.unroll.count"));
    assert!(comb_ir.contains("llvm.loop.vectorize.enable"));
    assert!(comb_ir.contains("llvm.loop.interleave.count\", i32 4"));
    // Must pass LLVMVerifyModule and optimize cleanly!
    let _opt = emit_llvm_source_with_plan(loop_src, CompatMode::V2, &combined_plan).unwrap();

    // 3. `recommend_candidates` capped at <= 6 unique validated plans distinct from baseline
    let hints = HwPlanHints {
        prefer_branchless: true,
        recommended_unroll: UnrollHint::Unroll4,
        prefer_native_cpu: true,
        has_avx2: true,
        prefer_soa: true,
        recommended_workers: 4,
    };
    let baseline = CodegenPlan::baseline_o0();
    let cands = recommend_candidates(&hints, &baseline);
    assert!(!cands.is_empty() && cands.len() <= 6);
    for c in &cands {
        assert_ne!(c, &baseline);
        c.validate().unwrap();
    }

    // 4. Legacy `"profile_version": 1` JSON yields `ProfileLoadOutcome::FallbackToBaseline` (`P0-4`)
    assert_eq!(PROFILE_VERSION, 2);
    let tmp = TempDirGuard::new("legacy_v1_prof");
    let hw = HostHardwareInfo::discover().unwrap();
    let k_sha = "a".repeat(64);
    let w_sha = "b".repeat(64);
    let base_plan = CodegenPlan::baseline_o0();
    let mut prof = RetainedProfile {
        profile_version: PROFILE_VERSION,
        policy_scope: POLICY_SCOPE.to_string(),
        cpu_sig_sha256: hw.cpu_sig_sha256.clone(),
        kernel_ir_sha256: k_sha.clone(),
        workload_sha256: w_sha.clone(),
        decision: LearnDecision::Rollback,
        reason: "v2 profile".to_string(),
        baseline_plan: base_plan,
        active_plan: base_plan,
        candidate_plan: base_plan,
        generation: 5,
        training_pairs: vec![(100, 100); 5],
        heldout_pairs: vec![(100, 100); 5],
        training_ci: BootstrapCi {
            point_gain_bps: 0,
            lower_95_bps: 0,
            upper_95_bps: 0,
            resamples: 2000,
            seed: 20261113,
        },
        heldout_ci: BootstrapCi {
            point_gain_bps: 0,
            lower_95_bps: 0,
            upper_95_bps: 0,
            resamples: 2000,
            seed: 20261113,
        },
        semantic_mismatches: 0,
        cand_compile_ns: 100,
        dispatch_overhead_ns: 10,
        measured_overhead_ns: 110,
        heldout_base_sum_ns: 500,
        heldout_cand_sum_ns: 500,
        heldout_net_savings_ns: -110,
        break_even_calls: None,
        profile_sha256: String::new(),
    };
    prof.profile_sha256 = prof.recompute_sha256();
    let v1_json = prof
        .to_json()
        .to_compact_ascii_json()
        .replacen("\"profile_version\":2", "\"profile_version\":1", 1);
    let v1_path = tmp.path().join("legacy_v1.json");
    std::fs::write(&v1_path, format!("{v1_json}\n")).unwrap();
    match RetainedProfile::load_validated(&v1_path, &hw.cpu_sig_sha256, &k_sha, &w_sha) {
        ProfileLoadOutcome::FallbackToBaseline { reason } => {
            assert!(
                reason.contains("unsupported profile_version 1"),
                "expected legacy v1 rejection, got: {reason}"
            );
        }
        ProfileLoadOutcome::Loaded(_) => panic!("legacy v1 profile must fallback to baseline"),
    }

    // 5. `soa_vs_aos_bench.dec` exact semantic parity across VM, baseline O2 JIT, and v2 vectorized JIT
    let bench_path = repo_root().join("tests/fixtures/v2_bench/soa_vs_aos_bench.dec");
    let bench_src = std::fs::read_to_string(&bench_path).unwrap();
    let vm_out = run_file(&bench_path, CompatMode::V2)
        .unwrap()
        .to_stdout_string();
    let (base_jit, base_res) = jit_source_with_plan_captured(
        &bench_src,
        CompatMode::V2,
        &CodegenPlan::baseline_o2(),
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    base_res.unwrap();
    assert_eq!(base_jit, vm_out);

    let (vec_jit, vec_res) = jit_source_with_plan_captured(
        &bench_src,
        CompatMode::V2,
        &combined_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    vec_res.unwrap();
    assert_eq!(vec_jit, vm_out);
}

// ============================================================================
// Pillar 3C: Parallel Read-Only Observers (`P0-1` & `P0-2`)
// Tight Fuel Equivalence, Heap-Spilled Module Consts, Ordered Emits & Error Precedence
// ============================================================================

#[test]
fn p3_parallel_observers_tight_fuel_consts_emits_and_error_precedence() {
    // 1. Heap-spilled 40-digit module `const` + 3 ordered emitting observers across VM, JIT, and AOT!
    let const_and_emit_src = r#"
use aod::core:
    t1, evolve

const BIG_TAG: int = 1234567890123456789012345678901234567890

dec obs_burn:
    (acc: int, i: int) -> int:
        return acc + i

chamber ParLab:
    space = box([0..32, 0..32, 0..32])
    walls = open

prepare Beam in ParLab:
    inject 3:3:6 at (10, 16, 16) dir +X orient 1t shells 1
    inject 3:3:6 at (22, 16, 16) dir -X orient t1(-1) shells 1

observe ObsA:
    tracer watch_a:
        (view: ReadOnly<ChamberView>) -> Null:
            let _acc = evolve(0, 0..4, obs_burn)
            emit BIG_TAG + view.tick * 10 + 1
            return null

observe ObsB:
    tracer watch_b:
        (view: ReadOnly<ChamberView>) -> Null:
            let _acc = evolve(0, 0..4, obs_burn)
            emit BIG_TAG + view.tick * 10 + 2
            return null

observe ObsC:
    tracer watch_c:
        (view: ReadOnly<ChamberView>) -> Null:
            let _acc = evolve(0, 0..4, obs_burn)
            emit BIG_TAG + view.tick * 10 + 3
            return null

dec main:
    () -> int:
        let r = simulate Beam for 3 ticks with ObsA, ObsB, ObsC
        emit r.final_state_hash
        return 0
"#;
    let checked = check_source(const_and_emit_src, CompatMode::V2).unwrap();

    // 1a. Full budget: VM seq == VM par == JIT seq == JIT par == AOT par
    let (vm_seq_emits, vm_seq_res) = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .with_parallel_observers(false)
        .run_main_capturing();
    vm_seq_res.unwrap();
    let (vm_par_emits, vm_par_res) = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .with_parallel_observers(true)
        .run_main_capturing();
    vm_par_res.unwrap();
    assert_eq!(vm_seq_emits, vm_par_emits);
    assert_eq!(vm_seq_emits.len(), 10); // 3 ticks * 3 observers + final_state_hash

    let seq_plan = CodegenPlan::baseline_o2();
    let par_plan = CodegenPlan {
        parallel_observers: true,
        ..CodegenPlan::baseline_o2()
    };
    let (jit_seq_out, jit_seq_res) = jit_source_with_plan_captured(
        const_and_emit_src,
        CompatMode::V2,
        &seq_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    jit_seq_res.unwrap();
    let (jit_par_out, jit_par_res) = jit_source_with_plan_captured(
        const_and_emit_src,
        CompatMode::V2,
        &par_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    jit_par_res.unwrap();
    assert_eq!(jit_seq_out, jit_par_out);

    // 1b. Standalone AOT ELF with `parallel_observers: true` verifies `emit_line` stdout capture (`P0-1`)
    let tmp = TempDirGuard::new("aot_par_obs");
    let raw_par_ir = lower_to_llvm_ir_with_plan(&checked, &par_plan).unwrap();
    assert!(raw_par_ir.contains("call i64 @aod_rt_sim_run_ex("));
    let aot_exe = tmp.path().join("par_obs_aot");
    aod_llvm::link_aot_executable_with_plan(&raw_par_ir, &par_plan, &aot_exe).unwrap();
    let aot_out = Command::new(&aot_exe).output().unwrap();
    assert!(
        aot_out.status.success(),
        "AOT parallel observer binary failed: {}",
        String::from_utf8_lossy(&aot_out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&aot_out.stdout), jit_seq_out);

    // 2. Tight-fuel sweep (`P0-2`): sequential == parallel byte-identical emits & error at EVERY fuel!
    for fuel in [5usize, 10, 15, 20, 25, 30, 40, 55, 70, 90] {
        let (v_seq_e, v_seq_r) = Evaluator::new(&checked, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .with_parallel_observers(false)
            .run_main_capturing();
        let (v_par_e, v_par_r) = Evaluator::new(&checked, fuel, DEFAULT_MAX_DEPTH)
            .unwrap()
            .with_parallel_observers(true)
            .run_main_capturing();
        assert_eq!(
            v_seq_e, v_par_e,
            "VM tight-fuel={fuel} emit mismatch (seq vs par)"
        );
        assert_eq!(
            v_seq_r.is_ok(),
            v_par_r.is_ok(),
            "VM tight-fuel={fuel} Ok/Err mismatch"
        );
        if let (Err(e1), Err(e2)) = (v_seq_r, v_par_r) {
            assert_eq!(e1.code, e2.code, "VM tight-fuel={fuel} error code mismatch");
        }

        let (j_seq_out, j_seq_r) = jit_source_with_plan_captured(
            const_and_emit_src,
            CompatMode::V2,
            &seq_plan,
            fuel,
            DEFAULT_MAX_DEPTH,
        )
        .unwrap();
        let (j_par_out, j_par_r) = jit_source_with_plan_captured(
            const_and_emit_src,
            CompatMode::V2,
            &par_plan,
            fuel,
            DEFAULT_MAX_DEPTH,
        )
        .unwrap();
        assert_eq!(
            j_seq_out, j_par_out,
            "JIT tight-fuel={fuel} stdout mismatch (seq vs par)"
        );
        assert_eq!(
            j_seq_r, j_par_r,
            "JIT tight-fuel={fuel} error mismatch (seq vs par)"
        );
    }

    // 3. Deterministic error precedence (`P0-2`): obs #2 fails, obs #3's emit MUST be discarded!
    let err_prec_src = r#"
chamber ErrLab:
    space = box([0..16, 0..16, 0..16])
    walls = open
prepare P in ErrLab:
    inject 3:3:6 at (4, 8, 8) dir +X orient 1t shells 1
observe Obs1:
    tracer t1:
        (view: ReadOnly<ChamberView>) -> Null:
            assert view.tick >= 0
            emit b"obs1_ok"
            return null
observe Obs2:
    tracer t2:
        (view: ReadOnly<ChamberView>) -> Null:
            assert view.tick >= 0
            emit b"obs2_before_fail"
            assert false
            return null
observe Obs3:
    tracer t3:
        (view: ReadOnly<ChamberView>) -> Null:
            assert view.tick >= 0
            emit b"obs3_must_be_discarded"
            return null
dec main:
    () -> int:
        let _r = simulate P for 2 ticks with Obs1, Obs2, Obs3
        return 0
"#;
    let err_checked = check_source(err_prec_src, CompatMode::V2).unwrap();
    let (ev_seq_e, ev_seq_r) = Evaluator::new(&err_checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .with_parallel_observers(false)
        .run_main_capturing();
    let (ev_par_e, ev_par_r) = Evaluator::new(&err_checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .unwrap()
        .with_parallel_observers(true)
        .run_main_capturing();
    assert!(ev_seq_r.is_err() && ev_par_r.is_err());
    assert_eq!(
        ev_seq_e,
        vec![
            "b\"obs1_ok\"".to_string(),
            "b\"obs2_before_fail\"".to_string()
        ]
    );
    assert_eq!(ev_par_e, ev_seq_e);

    let (je_seq_out, je_seq_r) = jit_source_with_plan_captured(
        err_prec_src,
        CompatMode::V2,
        &seq_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    let (je_par_out, je_par_r) = jit_source_with_plan_captured(
        err_prec_src,
        CompatMode::V2,
        &par_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    assert!(je_seq_r.is_err() && je_par_r.is_err());
    assert_eq!(je_seq_out, "b\"obs1_ok\"\nb\"obs2_before_fail\"\n");
    assert_eq!(je_par_out, je_seq_out);
    assert_eq!(je_par_r, je_seq_r);
}
