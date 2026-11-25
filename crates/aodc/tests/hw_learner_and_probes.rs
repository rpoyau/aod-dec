use std::path::PathBuf;
use std::process::Command;

use aod_core::json::{parse_strict_json, JsonValue};
use aod_llvm::hw_probe::probe_branch_kernel_ir;
use aod_llvm::{
    paired_bootstrap_ci_bps, probe_perf_event_open, read_rdtscp, run_all_hw_probes,
    run_learn_session, verify_and_optimize_ir, BootstrapCi, CodegenPlan, HostHardwareInfo,
    LearnDecision, OptLevel, OrcJitSession, ProfileLoadOutcome, RetainedProfile, UnrollHint,
    POLICY_SCOPE,
};
use aodc::codegen::lower_to_llvm_ir_with_plan;
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::{
    build_aot_file, check_file, compute_profile_bindings, emit_llvm_file_with_plan,
    emit_llvm_source_with_plan, jit_file_captured, jit_file_with_profile_captured,
    jit_source_with_plan_captured, learn_file_with_options, run_file, run_source, LoadOptions,
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
    let d = std::env::temp_dir().join(format!("aodc_pr4_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn ac02_host_hardware_discovery_and_deterministic_cpu_sig() {
    let hw1 = HostHardwareInfo::discover().unwrap();
    let hw2 = HostHardwareInfo::discover().unwrap();
    assert_eq!(hw1.cpu_sig_sha256, hw2.cpu_sig_sha256);
    assert_eq!(hw1.cpu_sig_sha256.len(), 64);
    assert_eq!(hw1.policy_scope, POLICY_SCOPE);
    assert!(!hw1.llvm_host_cpu.is_empty());
    assert!(hw1.logical_cores >= 1);
    assert_eq!(
        hw1.isa_flags.get("rdtscp").copied().unwrap_or(false),
        read_rdtscp().is_some()
    );
    if cfg!(target_os = "linux") {
        assert!(hw1.l1d_bytes > 0, "expected sysfs L1d cache > 0 on Linux");
        assert!(hw1.cache_line_bytes > 0, "expected sysfs cache line > 0 on Linux");
    }
    let _cap = probe_perf_event_open();
    let raw = hw1.to_json().to_compact_ascii_json();
    let parsed = parse_strict_json(&raw).unwrap();
    assert_eq!(
        parsed.get("policy_scope").and_then(JsonValue::as_str),
        Some(POLICY_SCOPE)
    );
}

#[test]
fn ac03_to_ac07_all_5_live_self_verifying_jit_probes_and_branch_ir_proof() {
    let opt_branching =
        verify_and_optimize_ir(probe_branch_kernel_ir(true), OptLevel::O2).unwrap();
    let opt_branchless =
        verify_and_optimize_ir(probe_branch_kernel_ir(false), OptLevel::O2).unwrap();
    assert!(
        opt_branching.contains("br i1 %cond"),
        "Probe 3 branching kernel must retain conditional `br i1` after -O2:\n{opt_branching}"
    );
    assert!(
        opt_branchless.contains("select i1"),
        "Probe 3 branchless kernel must emit `select i1` after -O2:\n{opt_branchless}"
    );

    let report = run_all_hw_probes().unwrap();
    assert_eq!(report.policy_scope, POLICY_SCOPE);
    assert!(report.memory_tiers.verified);
    assert_eq!(report.memory_tiers.tiers.len(), 6);
    assert!(report.exact_arith.verified);
    assert!(report.exact_arith.spill_to_fast_ratio_x100 > 100);
    assert!(report.branch_predictor.verified);
    assert!(report.unroll_layout.verified);
    assert!(report.multicore.verified);
    assert!(report.multicore.tested_workers >= 1);

    let json_text = report.to_json().to_compact_ascii_json();
    let parsed = parse_strict_json(&json_text).unwrap();
    assert_eq!(
        parsed.get("policy_scope").and_then(JsonValue::as_str),
        Some(POLICY_SCOPE)
    );
}

#[test]
fn ac08_ac09_parameterized_codegen_plan_p0_3_safety_and_unroll_metadata() {
    // 1. P0-3 strict is_pure_select_safe guard: MUST NOT eagerly evaluate trapping div-by-0!
    let lazy_trap_src = r#"
dec main() -> int:
    let x = 0
    let r = select(x != 0, 10 // x, 42)
    let t = mux(x <=> 0, 10 // x, 7, 10 // x)
    let min_t32 = -926510094425920 - 1
    let safe_neg = select(x != 0, -min_t32, 5)
    emit r
    emit t
    emit safe_neg
    0
"#;
    let vm_out = run_source(lazy_trap_src).unwrap().to_stdout_string();
    assert_eq!(vm_out, "42\n7\n5\n0\n");

    let sel_plan = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: true,
        unroll_hint: UnrollHint::Unroll4,
        target_cpu_native: true,
        inline_ratio_fastpath: true,
        ..CodegenPlan::baseline_o2()
    };
    let (jit_out, jit_res) = jit_source_with_plan_captured(
        lazy_trap_src,
        &sel_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    jit_res.unwrap();
    assert_eq!(jit_out, vm_out);

    // 2. P0-3 unique distinct !llvm.loop metadata passes LLVMVerifyModule for Unroll4 & Disable!
    let loop_src = r#"
dec main() -> int:
    let s = (0..32) |> +/
    emit s
    0
"#;
    let checked = aodc::check_source(loop_src).unwrap();
    for (hint, expected_md) in [
        (UnrollHint::Unroll4, "llvm.loop.unroll.count"),
        (UnrollHint::Disable, "llvm.loop.unroll.disable"),
    ] {
        let p = CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: false,
            unroll_hint: hint,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            ..CodegenPlan::baseline_o2()
        };
        let raw_ir = lower_to_llvm_ir_with_plan(&checked, &p).unwrap();
        assert!(raw_ir.contains("distinct !{"), "missing distinct loop md");
        assert!(raw_ir.contains(expected_md), "missing {expected_md}");
        let _opt_ir = emit_llvm_source_with_plan(loop_src, &p).unwrap();
    }

    // 3. `inline_ratio_fastpath: false` emits direct `@aod_rt_ratio_add` and preserves exact semantics
    let ratio_src = "dec main() -> int:\n    let r = 1/3r + 1/6r\n    emit r\n    0\n";
    let no_inline_plan = CodegenPlan {
        opt_level: OptLevel::O0,
        branchless_select: false,
        unroll_hint: UnrollHint::Default,
        target_cpu_native: false,
        inline_ratio_fastpath: false,
        ..CodegenPlan::baseline_o0()
    };
    let checked_r = aodc::check_source(ratio_src).unwrap();
    let raw_r = lower_to_llvm_ir_with_plan(&checked_r, &no_inline_plan).unwrap();
    assert!(raw_r.contains("call void @aod_rt_ratio_add"));
    let (ro, rres) = jit_source_with_plan_captured(
        ratio_src,
        &no_inline_plan,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
    )
    .unwrap();
    rres.unwrap();
    assert_eq!(ro, "1/2\n0\n");
}

#[test]
fn ac10_zero_float_paired_bootstrap_determinism_and_exact_semantic_guard() {
    let fast_pairs = vec![
        (10_000, 2_000),
        (10_200, 2_100),
        (9_800, 1_950),
        (10_100, 2_050),
        (9_900, 2_000),
        (10_050, 1_980),
    ];
    let ci1 = paired_bootstrap_ci_bps(&fast_pairs, 20261113, 2000);
    let ci2 = paired_bootstrap_ci_bps(&fast_pairs, 20261113, 2000);
    assert_eq!(ci1, ci2);
    assert!(ci1.lower_95_bps > 7500 && ci1.upper_95_bps <= 8500);

    let flat_pairs = vec![(5000, 5000); 6];
    let flat_ci = paired_bootstrap_ci_bps(&flat_pairs, 20261113, 2000);
    assert_eq!(flat_ci.point_gain_bps, 0);
    assert!(flat_ci.lower_95_bps <= 0);

    let slow_pairs = vec![(5000, 7000); 6];
    let slow_ci = paired_bootstrap_ci_bps(&slow_pairs, 20261113, 2000);
    assert!(slow_ci.lower_95_bps < 0);

    // Hostile Exact Semantic Guard rejection: candidate produces wrong output -> MUST Rollback!
    let hw = run_all_hw_probes().unwrap();
    let baseline = CodegenPlan::baseline_o0();
    let ok_src = "dec main() -> int:\n    emit \"OK\"\n    0\n";
    let bad_src = "dec main() -> int:\n    emit \"CORRUPTED\"\n    0\n";
    let ok_checked = aodc::check_source(ok_src).unwrap();
    let bad_checked = aodc::check_source(bad_src).unwrap();

    let session_rep = run_learn_session(
        &hw,
        baseline,
        "a".repeat(64),
        "b".repeat(64),
        DEFAULT_FUEL as i64,
        DEFAULT_MAX_DEPTH as i32,
        5,
        5,
        |plan| {
            let checked = if *plan == baseline {
                &ok_checked
            } else {
                &bad_checked
            };
            let raw = lower_to_llvm_ir_with_plan(checked, plan).map_err(|e| e.to_string())?;
            let sess = OrcJitSession::compile_with_plan(&raw, plan).map_err(|e| e.to_string())?;
            Ok((sess, 1000))
        },
    )
    .unwrap();

    assert_eq!(session_rep.profile.decision, LearnDecision::Rollback);
    assert_eq!(session_rep.profile.active_plan, baseline);
    assert!(session_rep.profile.semantic_mismatches > 0);
}

#[test]
fn ac11_ac12_ac13_retained_profile_atomic_roundtrip_and_hostile_tamper_fallbacks() {
    let tmp = temp_dir("ac11_profile");
    let hw = HostHardwareInfo::discover().unwrap();
    let k_sha = "1".repeat(64);
    let w_sha = "2".repeat(64);

    let base_plan = CodegenPlan::baseline_o0();
    let cand_plan = CodegenPlan::baseline_o2();
    let pairs = vec![(100_000, 10_000); 6];
    let ci = BootstrapCi {
        point_gain_bps: 9000,
        lower_95_bps: 8900,
        upper_95_bps: 9100,
        resamples: 2000,
        seed: 20261113,
    };

    let mut prof = RetainedProfile {
        profile_version: aodc::PROFILE_VERSION,
        policy_scope: POLICY_SCOPE.to_string(),
        cpu_sig_sha256: hw.cpu_sig_sha256.clone(),
        kernel_ir_sha256: k_sha.clone(),
        workload_sha256: w_sha.clone(),
        decision: LearnDecision::Retain,
        reason: "valid test profile".to_string(),
        baseline_plan: base_plan,
        active_plan: cand_plan,
        candidate_plan: cand_plan,
        generation: 6,
        training_pairs: pairs.clone(),
        heldout_pairs: pairs,
        training_ci: ci,
        heldout_ci: ci,
        semantic_mismatches: 0,
        cand_compile_ns: 50_000,
        dispatch_overhead_ns: 500,
        measured_overhead_ns: 50_500,
        heldout_base_sum_ns: 600_000,
        heldout_cand_sum_ns: 60_000,
        heldout_net_savings_ns: 489_500,
        break_even_calls: Some(1),
        profile_sha256: String::new(),
    };
    prof.profile_sha256 = prof.recompute_sha256();

    let good_path = tmp.join("good_profile.json");
    prof.save_atomic(&good_path).unwrap();
    assert_eq!(
        RetainedProfile::load_validated(&good_path, &hw.cpu_sig_sha256, &k_sha, &w_sha),
        ProfileLoadOutcome::Loaded(prof.clone())
    );

    assert!(matches!(
        RetainedProfile::load_validated(&good_path, &"f".repeat(64), &k_sha, &w_sha),
        ProfileLoadOutcome::FallbackToBaseline { .. }
    ));
    assert!(matches!(
        RetainedProfile::load_validated(&good_path, &hw.cpu_sig_sha256, &"f".repeat(64), &w_sha),
        ProfileLoadOutcome::FallbackToBaseline { .. }
    ));
    assert!(matches!(
        RetainedProfile::load_validated(&good_path, &hw.cpu_sig_sha256, &k_sha, &"f".repeat(64)),
        ProfileLoadOutcome::FallbackToBaseline { .. }
    ));
    let huge_path = tmp.join("huge.json");
    std::fs::write(&huge_path, vec![b' '; 1_048_577]).unwrap();
    assert!(matches!(
        RetainedProfile::load_validated(&huge_path, &hw.cpu_sig_sha256, &k_sha, &w_sha),
        ProfileLoadOutcome::FallbackToBaseline { reason } if reason.contains("1048576")
    ));
    let float_path = tmp.join("float.json");
    std::fs::write(&float_path, "{\"gain\": 1.25}\n").unwrap();
    assert!(matches!(
        RetainedProfile::load_validated(&float_path, &hw.cpu_sig_sha256, &k_sha, &w_sha),
        ProfileLoadOutcome::FallbackToBaseline { .. }
    ));
    let tampered_path = tmp.join("tampered.json");
    let raw_good = std::fs::read_to_string(&good_path).unwrap();
    std::fs::write(&tampered_path, raw_good.replace("valid test profile", "valid test profilX"))
        .unwrap();
    assert!(matches!(
        RetainedProfile::load_validated(&tampered_path, &hw.cpu_sig_sha256, &k_sha, &w_sha),
        ProfileLoadOutcome::FallbackToBaseline { reason } if reason.contains("profile_sha256")
    ));

    let bench = repo_root().join("tests/fixtures/v2_bench/branch_irregular_bench.dec");
    let vm_out = run_file(&bench).unwrap().to_stdout_string();
    let (jit_out, jit_res, used_plan, fallback_note) = jit_file_with_profile_captured(
        &bench,
        OptLevel::O2,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
        Some(&tampered_path),
        &LoadOptions::default(),
    )
    .unwrap();
    jit_res.unwrap();
    assert_eq!(jit_out, vm_out);
    assert_eq!(used_plan, CodegenPlan::for_opt_level(OptLevel::O2));
    assert!(fallback_note.is_some());

    let dup_key_path = tmp.join("dup_key.json");
    std::fs::write(&dup_key_path, "{\"a\":1,\"a\":2}\n").unwrap();
    let cpu_drift_path = tmp.join("cpu_drift.json");
    let mut drifted = prof.clone();
    drifted.cpu_sig_sha256 = "0".repeat(64);
    drifted.profile_sha256 = drifted.recompute_sha256();
    drifted.save_atomic(&cpu_drift_path).unwrap();

    for hostile in [
        &tampered_path,
        &cpu_drift_path,
        &dup_key_path,
        &float_path,
        &huge_path,
    ] {
        let cli = Command::new(aodc_bin())
            .args(["jit", "-O2", "--profile"])
            .arg(hostile)
            .arg(&bench)
            .output()
            .unwrap();
        assert!(
            cli.status.success(),
            "aodc jit --profile {} failed: {}",
            hostile.display(),
            String::from_utf8_lossy(&cli.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&cli.stdout),
            vm_out,
            "stdout mismatch on fallback for {}",
            hostile.display()
        );
        let stderr = String::from_utf8_lossy(&cli.stderr);
        assert!(
            stderr.contains("aodc: profile fallback to baseline:"),
            "expected stderr fallback notice for {}, got: {:?}",
            hostile.display(),
            stderr
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac14_cli_probe_hw_honest_learn_amortized_retain_and_overwrite_refusal() {
    let root = repo_root();
    let tmp = temp_dir("ac14_cli");

    let probe_json = tmp.join("hw_report.json");
    let p_cli = Command::new(aodc_bin())
        .args(["probe-hw", "--output"])
        .arg(&probe_json)
        .output()
        .unwrap();
    assert!(
        p_cli.status.success(),
        "probe-hw failed: {}",
        String::from_utf8_lossy(&p_cli.stderr)
    );
    let parsed_hw = parse_strict_json(&std::fs::read_to_string(&probe_json).unwrap()).unwrap();
    assert_eq!(
        parsed_hw.get("policy_scope").and_then(JsonValue::as_str),
        Some(POLICY_SCOPE)
    );
    assert!(parsed_hw.get("memory_tiers").is_some());
    assert!(parsed_hw.get("exact_arith").is_some());
    assert!(parsed_hw.get("branch_predictor").is_some());
    assert!(parsed_hw.get("unroll_layout").is_some());
    assert!(parsed_hw.get("multicore").is_some());

    let amort_src = root.join("tests/fixtures/v2_bench/learn_amortized_bench.dec");
    let self_ov = Command::new(aodc_bin())
        .args(["learn", "--profile"])
        .arg(&amort_src)
        .arg(&amort_src)
        .output()
        .unwrap();
    assert!(!self_ov.status.success());
    assert!(String::from_utf8_lossy(&self_ov.stderr).contains("differ"));

    let amort_prof = tmp.join("amortized_profile.json");
    let learn_cli = Command::new(aodc_bin())
        .args([
            "learn",
            "--baseline",
            "O0",
            "--fuel",
            "2000000",
            "--pairs",
            "6",
            "--heldout",
            "8",
            "--profile",
        ])
        .arg(&amort_prof)
        .arg(&amort_src)
        .output()
        .unwrap();
    assert!(
        learn_cli.status.success(),
        "aodc learn failed:\n{}\n{}",
        String::from_utf8_lossy(&learn_cli.stdout),
        String::from_utf8_lossy(&learn_cli.stderr)
    );

    let hw = HostHardwareInfo::discover().unwrap();
    let checked = check_file(&amort_src).unwrap();
    let (k_sha, w_sha) = compute_profile_bindings(&checked, 2_000_000, DEFAULT_MAX_DEPTH);
    let ProfileLoadOutcome::Loaded(loaded_prof) =
        RetainedProfile::load_validated(&amort_prof, &hw.cpu_sig_sha256, &k_sha, &w_sha)
    else {
        panic!("expected valid retained profile at {}", amort_prof.display());
    };
    assert_eq!(
        loaded_prof.decision,
        LearnDecision::Retain,
        "expected honest Retain on learn_amortized_bench.dec, got: {:?}",
        loaded_prof
    );
    assert!(loaded_prof.measured_overhead_ns > 0);
    assert!(loaded_prof.heldout_ci.lower_95_bps > 500);
    assert!(
        loaded_prof.heldout_base_sum_ns
            > loaded_prof
                .heldout_cand_sum_ns
                .saturating_add(loaded_prof.measured_overhead_ns)
    );

    let jit_plain = Command::new(aodc_bin())
        .args(["jit", "-O0", "--fuel", "2000000"])
        .arg(&amort_src)
        .output()
        .unwrap();
    let jit_prof = Command::new(aodc_bin())
        .args(["jit", "-O0", "--fuel", "2000000", "--profile"])
        .arg(&amort_prof)
        .arg(&amort_src)
        .output()
        .unwrap();
    assert!(jit_plain.status.success() && jit_prof.status.success());
    assert_eq!(jit_plain.stdout, jit_prof.stdout);

    let short_src = root.join("tests/fixtures/v2_bench/unboxed_loop_bench.dec");
    let short_prof = tmp.join("short_profile.json");
    let (_, short_rep) = learn_file_with_options(
        &short_src,
        OptLevel::O0,
        5,
        5,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
        Some(&short_prof),
        &LoadOptions::default(),
    )
    .unwrap();
    assert!(short_rep.profile.measured_overhead_ns > 0);
    assert_eq!(short_rep.profile.semantic_mismatches, 0);
    if short_rep.profile.decision == LearnDecision::Retain {
        assert!(
            short_rep.profile.heldout_base_sum_ns
                > short_rep
                    .profile
                    .heldout_cand_sum_ns
                    .saturating_add(short_rep.profile.measured_overhead_ns)
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac15_branch_irregular_bench_parity_and_branchless_select_ir() {
    let root = repo_root();
    let bench = root.join("tests/fixtures/v2_bench/branch_irregular_bench.dec");
    let tmp = temp_dir("ac15_branch");

    let vm_out = run_file(&bench).unwrap().to_stdout_string();

    let checked = check_file(&bench).unwrap();
    let plan_sel = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: true,
        unroll_hint: UnrollHint::Default,
        target_cpu_native: true,
        inline_ratio_fastpath: true,
        ..CodegenPlan::baseline_o2()
    };
    let plan_br = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: false,
        unroll_hint: UnrollHint::Default,
        target_cpu_native: true,
        inline_ratio_fastpath: true,
        ..CodegenPlan::baseline_o2()
    };
    let raw_sel = lower_to_llvm_ir_with_plan(&checked, &plan_sel).unwrap();
    let raw_br = lower_to_llvm_ir_with_plan(&checked, &plan_br).unwrap();
    assert!(
        raw_sel.contains("= select i1 ") && !raw_sel.contains("sel_t_") && !raw_sel.contains("mux_n_"),
        "expected select lowering with branchless_select=true"
    );
    assert!(
        (raw_br.contains("sel_t_") || raw_br.contains("mux_n_")) && raw_br.contains("br i1 "),
        "expected br i1 lowering with branchless_select=false"
    );
    let _ = emit_llvm_file_with_plan(&bench, &plan_sel, &LoadOptions::default()).unwrap();
    let _ = emit_llvm_file_with_plan(&bench, &plan_br, &LoadOptions::default()).unwrap();

    let exe = tmp.join("branch_o2");
    build_aot_file(&bench, OptLevel::O2, &exe).unwrap();
    let aot_out = Command::new(&exe).output().unwrap();
    assert!(aot_out.status.success());
    assert_eq!(String::from_utf8_lossy(&aot_out.stdout), vm_out);

    let (jout, jres) =
        jit_file_captured(&bench, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).unwrap();
    jres.unwrap();
    assert_eq!(jout, vm_out);

    let s_sel = OrcJitSession::compile_with_plan(&raw_sel, &plan_sel).unwrap();
    let (sel_out, sel_res) = s_sel.run_captured(DEFAULT_FUEL as i64, DEFAULT_MAX_DEPTH as i32);
    sel_res.unwrap();
    assert_eq!(sel_out, vm_out);

    let (_, learn_rep) = learn_file_with_options(
        &bench,
        OptLevel::O0,
        5,
        5,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
        None,
        &LoadOptions::default(),
    )
    .unwrap();
    assert_eq!(learn_rep.baseline_stdout, vm_out);
    assert_eq!(learn_rep.profile.semantic_mismatches, 0);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn p0_nullfn_no_entry_session_returns_clean_error_and_p2_negative_json_rejected() {
    let ir = "define i64 @helper_only(i64 %x) {\nentry:\n  ret i64 %x\n}\n";
    let sess =
        OrcJitSession::compile_named_with_plan(ir, &CodegenPlan::baseline_o0(), None).unwrap();
    let (stdout, res) = sess.run_captured(1000, 32);
    assert!(stdout.is_empty());
    let err = res.expect_err("expected clean error when entry_addr == 0");
    assert!(
        err.contains("no default entry symbol"),
        "unexpected error: {err}"
    );

    let mut ctx = aod_rt::abi::OwnedRtContext::new(1000, 32, true);
    sess.execute(&mut ctx);
    assert!(ctx.failed());
    assert_eq!(
        ctx.error_message(),
        Some("OrcJitSession has no default entry symbol")
    );

    let tmp = temp_dir("p2_neg_json");
    let hw = HostHardwareInfo::discover().unwrap();
    let k_sha = "3".repeat(64);
    let w_sha = "4".repeat(64);
    let neg_path = tmp.join("neg_fields.json");
    let base_plan = CodegenPlan::baseline_o0();
    let mut prof = RetainedProfile {
        profile_version: aodc::PROFILE_VERSION,
        policy_scope: POLICY_SCOPE.to_string(),
        cpu_sig_sha256: hw.cpu_sig_sha256.clone(),
        kernel_ir_sha256: k_sha.clone(),
        workload_sha256: w_sha.clone(),
        decision: LearnDecision::Rollback,
        reason: "neg test".to_string(),
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
    let raw = prof.to_json().to_compact_ascii_json();

    for (from, to, expected_substr) in [
        ("\"semantic_mismatches\":0", "\"semantic_mismatches\":-1", "semantic_mismatches"),
        ("\"profile_version\":2", "\"profile_version\":-1", "profile_version"),
        ("\"heldout_base_sum_ns\":500", "\"heldout_base_sum_ns\":-5", "heldout_base_sum_ns"),
        ("\"resamples\":2000", "\"resamples\":-1", "resamples"),
    ] {
        let mutated = raw.replacen(from, to, 1);
        assert_ne!(mutated, raw);
        std::fs::write(&neg_path, format!("{mutated}\n")).unwrap();
        match RetainedProfile::load_validated(&neg_path, &hw.cpu_sig_sha256, &k_sha, &w_sha) {
            ProfileLoadOutcome::FallbackToBaseline { reason } => {
                assert!(
                    reason.contains(expected_substr),
                    "expected {expected_substr:?} rejection, got: {reason}"
                );
            }
            ProfileLoadOutcome::Loaded(_) => panic!("negative {from} -> {to} must be rejected!"),
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
