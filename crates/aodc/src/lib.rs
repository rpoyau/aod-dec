#![forbid(unsafe_code)]

//! `aodc`: Clean-break Dec compiler frontend, compile-time firewalls, hermetic `DecIR` v3
//! serializer & verifier, exact reference evaluator, Trit-Boxed 2-bit balanced ternary
//! LLVM 18 IR lowering, AOT ELF / ORC LLJIT execution, hardware discovery & zero-float
//! learner, fixed-point self-hosting bootstrap verifier, and CLI.

pub mod ast;
pub mod checker;
pub mod codegen;
pub mod diagnostics;
pub mod eval;
pub mod ir;
pub mod lexer;
pub mod manual_data;
pub mod manual_verify;
pub mod modules;
pub mod parser;
pub mod pdf_audit;
pub mod report_guard;
pub mod sim;
pub mod types;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use aod_core::json::JsonValue;
use aod_core::s2_vectors::verify_s2_vectors_dir;
pub use aod_core::s2_vectors::{eval_operation, S2VerifyReport};
pub use aod_core::s4_report::{
    audit_aod_core_policy, build_s4_report, sanitize_error, serialize_s4_report,
    validate_portable_report, verify_s4_cycle_shedding_csv, CorePolicyAudit,
    CycleSheddingFixtureReport, MAX_S4_INPUT_BYTES, S4_BASE_COMMIT_BINDING, S4_BINDING_ID,
    S4_CYCLE_SHEDDING_CSV_REL, S4_REPORT_SCHEMA,
};
use aod_core::sha256::Sha256;
pub use aod_llvm::{
    recommend_candidates, run_all_hw_probes, BootstrapCi, CodegenPlan, HostHardwareInfo,
    HwPlanHints, HwProbeReport, LearnDecision, LearnSessionReport, OptLevel, OrcJitSession,
    PerfCapability, ProfileLoadOutcome, RetainedProfile, UnrollHint, POLICY_SCOPE, PROFILE_VERSION,
};
pub use crate::manual_data::{
    run_manual_data, validate_manual_data_output_dir, ManualDataOptions, ManualDataReport,
    MANUAL_DATA_FILENAMES, MANUAL_DATA_SCHEMA,
};
pub use crate::manual_verify::{verify_manual_dir, ManualVerifyOptions, ManualVerifyReport};
pub use crate::modules::LoadOptions;
pub use crate::pdf_audit::{
    adler32, audit_pdf_bytes, audit_pdf_file, inflate_deflate, inflate_zlib, InflateError,
    PdfAuditOptions, PdfAuditReport, MAX_PDF_BYTES, MAX_STREAMS, MAX_STREAM_DECOMP_BYTES,
    MAX_TOTAL_DECOMP_BYTES, PDF_AUDIT_SCHEMA,
};
pub use crate::report_guard::{
    collect_guarded_roots, read_bounded_regular_file, read_bounded_stdin,
    validate_runtime_output_path, write_report_atomic, ALLOWED_IN_REPO_OUTPUT_ROOTS,
};

use crate::checker::{check_unit, CheckedUnit};
use crate::codegen::{lower_to_llvm_ir, lower_to_llvm_ir_with_plan};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::eval::{EvalOutput, Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use crate::ir::{dumps_ir, loads_ir};
use crate::modules::load_from_source_with_options;

static BENCH_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn check_source(source: &str) -> DecResult<CheckedUnit> {
    check_source_with_options(source, &LoadOptions::default())
}

pub fn check_source_with_options(source: &str, options: &LoadOptions) -> DecResult<CheckedUnit> {
    let loaded = load_from_source_with_options(source, None, options)?;
    check_unit(loaded, true)
}

pub fn check_file(path: &Path) -> DecResult<CheckedUnit> {
    check_file_with_options(path, &LoadOptions::default())
}

pub fn check_file_with_options(path: &Path, options: &LoadOptions) -> DecResult<CheckedUnit> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("failed reading {}: {e}", path.display()),
        )
    })?;
    let base_dir = path.parent();
    let loaded = load_from_source_with_options(&source, base_dir, options)?;
    check_unit(loaded, true)
}

pub fn run_source(source: &str) -> DecResult<EvalOutput> {
    let checked = check_source(source)?;
    Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)?.run_main()
}

pub fn run_source_with_budget(
    source: &str,
    fuel: usize,
    max_depth: usize,
) -> DecResult<EvalOutput> {
    let checked = check_source(source)?;
    Evaluator::new(&checked, fuel, max_depth)?.run_main()
}

pub fn run_file(path: &Path) -> DecResult<EvalOutput> {
    let checked = check_file(path)?;
    Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)?.run_main()
}

pub fn run_file_with_budget_capturing(
    path: &Path,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(Vec<String>, DecResult<crate::eval::Value>)> {
    run_file_with_options_capturing(path, fuel, max_depth, &LoadOptions::default())
}

pub fn run_file_with_options_capturing(
    path: &Path,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
) -> DecResult<(Vec<String>, DecResult<crate::eval::Value>)> {
    run_file_with_parallel_observers_capturing(path, fuel, max_depth, options, false)
}

pub fn run_file_with_parallel_observers_capturing(
    path: &Path,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
    parallel_observers: bool,
) -> DecResult<(Vec<String>, DecResult<crate::eval::Value>)> {
    let checked = check_file_with_options(path, options)?;
    Ok(Evaluator::new(&checked, fuel, max_depth)?
        .with_parallel_observers(parallel_observers)
        .run_main_capturing())
}

pub fn compile_file_to_ir(path: &Path) -> DecResult<String> {
    compile_file_to_ir_with_options(path, &LoadOptions::default())
}

pub fn compile_file_to_ir_with_options(path: &Path, options: &LoadOptions) -> DecResult<String> {
    let checked = check_file_with_options(path, options)?;
    Ok(dumps_ir(&checked))
}

pub fn exec_ir_str(ir_text: &str) -> DecResult<EvalOutput> {
    let checked = loads_ir(ir_text)?;
    Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)?.run_main()
}

pub fn exec_ir_file(path: &Path) -> DecResult<EvalOutput> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("failed reading IR {}: {e}", path.display()),
        )
    })?;
    exec_ir_str(&text)
}

pub fn exec_ir_file_with_budget_capturing(
    path: &Path,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(Vec<String>, DecResult<crate::eval::Value>)> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("failed reading IR {}: {e}", path.display()),
        )
    })?;
    let checked = loads_ir(&text)?;
    Ok(Evaluator::new(&checked, fuel, max_depth)?.run_main_capturing())
}

// ============================================================================
// Native LLVM 18 Lowering, AOT ELF, ORC LLJIT, & Zero-Float Learner
// ============================================================================

pub fn emit_llvm_source(source: &str, opt: OptLevel) -> DecResult<String> {
    emit_llvm_source_with_plan(source, &CodegenPlan::for_opt_level(opt))
}

pub fn emit_llvm_source_with_plan(source: &str, plan: &CodegenPlan) -> DecResult<String> {
    let checked = check_source(source)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    aod_llvm::verify_and_optimize_ir_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn emit_llvm_file(path: &Path, opt: OptLevel) -> DecResult<String> {
    emit_llvm_file_with_options(path, opt, &LoadOptions::default())
}

pub fn emit_llvm_file_with_options(
    path: &Path,
    opt: OptLevel,
    options: &LoadOptions,
) -> DecResult<String> {
    emit_llvm_file_with_plan(path, &CodegenPlan::for_opt_level(opt), options)
}

pub fn emit_llvm_file_with_plan(
    path: &Path,
    plan: &CodegenPlan,
    options: &LoadOptions,
) -> DecResult<String> {
    let checked = check_file_with_options(path, options)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    aod_llvm::verify_and_optimize_ir_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn build_aot_source(source: &str, opt: OptLevel, out_exe: &Path) -> DecResult<()> {
    let checked = check_source(source)?;
    let raw_ir = lower_to_llvm_ir(&checked)?;
    aod_llvm::link_aot_executable(&raw_ir, opt, out_exe)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn build_aot_file(path: &Path, opt: OptLevel, out_exe: &Path) -> DecResult<()> {
    build_aot_file_with_options(path, opt, out_exe, &LoadOptions::default())
}

pub fn build_aot_file_with_options(
    path: &Path,
    opt: OptLevel,
    out_exe: &Path,
    options: &LoadOptions,
) -> DecResult<()> {
    let checked = check_file_with_options(path, options)?;
    let plan = CodegenPlan::for_opt_level(opt);
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, &plan)?;
    aod_llvm::link_aot_executable_with_plan(&raw_ir, &plan, out_exe)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn compile_jit_source(source: &str, opt: OptLevel) -> DecResult<OrcJitSession> {
    compile_jit_source_with_plan(source, &CodegenPlan::for_opt_level(opt))
}

pub fn compile_jit_source_with_plan(source: &str, plan: &CodegenPlan) -> DecResult<OrcJitSession> {
    let checked = check_source(source)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    OrcJitSession::compile_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn compile_jit_file(path: &Path, opt: OptLevel) -> DecResult<OrcJitSession> {
    compile_jit_file_with_options(path, opt, &LoadOptions::default())
}

pub fn compile_jit_file_with_options(
    path: &Path,
    opt: OptLevel,
    options: &LoadOptions,
) -> DecResult<OrcJitSession> {
    compile_jit_file_with_plan(path, &CodegenPlan::for_opt_level(opt), options)
}

pub fn compile_jit_file_with_plan(
    path: &Path,
    plan: &CodegenPlan,
    options: &LoadOptions,
) -> DecResult<OrcJitSession> {
    let checked = check_file_with_options(path, options)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    OrcJitSession::compile_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn jit_source_captured(
    source: &str,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(String, Result<(), String>)> {
    let session = compile_jit_source(source, opt)?;
    Ok(session.run_captured(fuel as i64, max_depth as i32))
}

pub fn jit_source_with_plan_captured(
    source: &str,
    plan: &CodegenPlan,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(String, Result<(), String>)> {
    let session = compile_jit_source_with_plan(source, plan)?;
    Ok(session.run_captured(fuel as i64, max_depth as i32))
}

pub fn jit_file_captured(
    path: &Path,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(String, Result<(), String>)> {
    jit_file_with_options_captured(path, opt, fuel, max_depth, &LoadOptions::default())
}

pub fn jit_file_with_options_captured(
    path: &Path,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
) -> DecResult<(String, Result<(), String>)> {
    let session = compile_jit_file_with_options(path, opt, options)?;
    Ok(session.run_captured(fuel as i64, max_depth as i32))
}

pub fn compute_profile_bindings(
    checked: &CheckedUnit,
    fuel: usize,
    max_depth: usize,
) -> (String, String) {
    let ir_json = dumps_ir(checked);
    let kernel_ir_sha256 = Sha256::digest_hex(ir_json.as_bytes());
    let workload_desc = format!("abi=trit_boxed_v3;fuel={fuel};max_depth={max_depth}");
    let workload_sha256 = Sha256::digest_hex(workload_desc.as_bytes());
    (kernel_ir_sha256, workload_sha256)
}

/// Execute via ORC LLJIT honoring an optional validated `RetainedProfile` (falling back safely
/// to `fallback_opt` on any corruption, tampering, or CPU/IR/workload mismatch!).
pub fn jit_file_with_profile_captured(
    path: &Path,
    fallback_opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    profile_path: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<(String, Result<(), String>, CodegenPlan, Option<String>)> {
    jit_file_with_profile_and_parallel_captured(
        path,
        fallback_opt,
        fuel,
        max_depth,
        profile_path,
        options,
        false,
    )
}

pub fn jit_file_with_profile_and_parallel_captured(
    path: &Path,
    fallback_opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    profile_path: Option<&Path>,
    options: &LoadOptions,
    parallel_observers: bool,
) -> DecResult<(String, Result<(), String>, CodegenPlan, Option<String>)> {
    let checked = check_file_with_options(path, options)?;
    let fallback_plan = CodegenPlan::for_opt_level(fallback_opt);

    let (mut chosen_plan, note) = if let Some(pp) = profile_path {
        let hw = HostHardwareInfo::discover()
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
        let (kernel_sha, workload_sha) = compute_profile_bindings(&checked, fuel, max_depth);
        match RetainedProfile::load_validated(pp, &hw.cpu_sig_sha256, &kernel_sha, &workload_sha) {
            ProfileLoadOutcome::Loaded(prof) => (prof.active_plan, None),
            ProfileLoadOutcome::FallbackToBaseline { reason } => (fallback_plan, Some(reason)),
        }
    } else {
        (fallback_plan, None)
    };
    if parallel_observers {
        chosen_plan.parallel_observers = true;
    }

    let raw_ir = lower_to_llvm_ir_with_plan(&checked, &chosen_plan)?;
    let session = OrcJitSession::compile_with_plan(&raw_ir, &chosen_plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
    let (stdout, res) = session.run_captured(fuel as i64, max_depth as i32);
    Ok((stdout, res, chosen_plan, note))
}

/// Run CPU discovery + 5 live JIT probes + semantically-guarded, honest cost-inclusive learning
/// session on `path`, optionally persisting the resulting rollback-safe `RetainedProfile` atomically.
#[allow(clippy::too_many_arguments)]
pub fn learn_file_with_options(
    path: &Path,
    baseline_opt: OptLevel,
    training_pairs: usize,
    heldout_pairs: usize,
    fuel: usize,
    max_depth: usize,
    profile_out: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<(HwProbeReport, LearnSessionReport)> {
    let checked = check_file_with_options(path, options)?;
    let hw_report = run_all_hw_probes()
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
    let (kernel_ir_sha256, workload_sha256) = compute_profile_bindings(&checked, fuel, max_depth);

    let baseline_plan = match baseline_opt {
        OptLevel::O0 => CodegenPlan::baseline_o0(),
        OptLevel::O2 => CodegenPlan::baseline_o2(),
        other => CodegenPlan::for_opt_level(other),
    };

    let report = aod_llvm::run_learn_session(
        &hw_report,
        baseline_plan,
        kernel_ir_sha256,
        workload_sha256,
        fuel as i64,
        max_depth as i32,
        training_pairs,
        heldout_pairs,
        |plan| {
            let t0 = Instant::now();
            let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan).map_err(|e| e.to_string())?;
            let sess = OrcJitSession::compile_with_plan(&raw_ir, plan).map_err(|e| e.to_string())?;
            let compile_ns = t0.elapsed().as_nanos().max(1) as u64;
            Ok((sess, compile_ns))
        },
    )
    .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e))?;

    if let Some(p) = profile_out {
        report
            .profile
            .save_atomic(p)
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e))?;
    }

    Ok((hw_report, report))
}

/// Zero-float integer timing & parity benchmark report across all 5 execution modes (`AC-15`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BenchReport {
    pub iters: usize,
    pub vm_median_us: u128,
    pub aot_o0_compile_us: u128,
    pub aot_o0_median_us: u128,
    pub aot_o2_compile_us: u128,
    pub aot_o2_median_us: u128,
    pub jit_o0_compile_us: u128,
    pub jit_o0_median_us: u128,
    pub jit_o2_compile_us: u128,
    pub jit_o2_median_us: u128,
    pub aot_o2_speedup_x100: u128,
    pub jit_o2_speedup_x100: u128,
    pub parity_ok: bool,
    pub stdout: String,
}

fn median_u128(mut samples: Vec<u128>) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn speedup_x100(vm_us: u128, native_us: u128) -> u128 {
    let denom = native_us.max(1);
    (vm_us.saturating_mul(100)) / denom
}

pub fn format_x100(v: u128) -> String {
    format!("{}.{:02}x", v / 100, v % 100)
}

pub fn bench_file(path: &Path, iters: usize, fuel: usize, max_depth: usize) -> DecResult<BenchReport> {
    bench_file_with_options(path, iters, fuel, max_depth, &LoadOptions::default())
}

pub fn bench_file_with_options(
    path: &Path,
    iters: usize,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
) -> DecResult<BenchReport> {
    let iters = iters.max(1);
    let checked = check_file_with_options(path, options)?;
    let raw_ir = lower_to_llvm_ir(&checked)?;

    let vm_out = Evaluator::new(&checked, fuel, max_depth)?
        .run_main()?
        .to_stdout_string();
    let mut vm_times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        let out = Evaluator::new(&checked, fuel, max_depth)?
            .run_main()?
            .to_stdout_string();
        vm_times.push(t0.elapsed().as_micros());
        if out != vm_out {
            return Err(Diagnostic::new(
                DiagnosticCode::RuntimeError,
                "VM non-determinism detected during benchmark",
            ));
        }
    }
    let vm_median_us = median_u128(vm_times);

    let seq = BENCH_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let exe_o0 = std::env::temp_dir().join(format!("aodc_bench_{}_{}_o0", std::process::id(), seq));
    let exe_o2 = std::env::temp_dir().join(format!("aodc_bench_{}_{}_o2", std::process::id(), seq));

    let bench_res = (|| -> DecResult<BenchReport> {
        let t0 = Instant::now();
        aod_llvm::link_aot_executable(&raw_ir, OptLevel::O0, &exe_o0)
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
        let aot_o0_compile_us = t0.elapsed().as_micros();

        let t0 = Instant::now();
        aod_llvm::link_aot_executable(&raw_ir, OptLevel::O2, &exe_o2)
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
        let aot_o2_compile_us = t0.elapsed().as_micros();

        let run_aot = |exe: &Path| -> DecResult<Vec<u128>> {
            let mut times = Vec::with_capacity(iters);
            let fuel_s = fuel.to_string();
            let depth_s = max_depth.to_string();
            let w = Command::new(exe)
                .args(["--fuel", &fuel_s, "--max-depth", &depth_s])
                .output()
                .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
            if !w.status.success() {
                return Err(Diagnostic::new(
                    DiagnosticCode::RuntimeError,
                    format!(
                        "AOT executable failed: {}",
                        String::from_utf8_lossy(&w.stderr)
                    ),
                ));
            }
            if String::from_utf8_lossy(&w.stdout) != vm_out {
                return Err(Diagnostic::new(
                    DiagnosticCode::RuntimeError,
                    "AOT stdout mismatch against Exact VM",
                ));
            }
            for _ in 0..iters {
                let t = Instant::now();
                let o = Command::new(exe)
                    .args(["--fuel", &fuel_s, "--max-depth", &depth_s])
                    .output()
                    .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
                times.push(t.elapsed().as_micros());
                if !o.status.success() || String::from_utf8_lossy(&o.stdout) != vm_out {
                    return Err(Diagnostic::new(
                        DiagnosticCode::RuntimeError,
                        "AOT stdout parity failure during timed run",
                    ));
                }
            }
            Ok(times)
        };

        let aot_o0_median_us = median_u128(run_aot(&exe_o0)?);
        let aot_o2_median_us = median_u128(run_aot(&exe_o2)?);

        let t0 = Instant::now();
        let jit_o0 = OrcJitSession::compile(&raw_ir, OptLevel::O0)
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
        let jit_o0_compile_us = t0.elapsed().as_micros();

        let t0 = Instant::now();
        let jit_o2 = OrcJitSession::compile(&raw_ir, OptLevel::O2)
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
        let jit_o2_compile_us = t0.elapsed().as_micros();

        let run_jit = |sess: &OrcJitSession| -> DecResult<Vec<u128>> {
            let (w_out, w_res) = sess.run_captured(fuel as i64, max_depth as i32);
            w_res.map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e))?;
            if w_out != vm_out {
                return Err(Diagnostic::new(
                    DiagnosticCode::RuntimeError,
                    "ORC JIT stdout mismatch against Exact VM",
                ));
            }
            let mut times = Vec::with_capacity(iters);
            for _ in 0..iters {
                let t = Instant::now();
                let (out, res) = sess.run_captured(fuel as i64, max_depth as i32);
                times.push(t.elapsed().as_micros());
                res.map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e))?;
                if out != vm_out {
                    return Err(Diagnostic::new(
                        DiagnosticCode::RuntimeError,
                        "ORC JIT stdout parity failure during timed run",
                    ));
                }
            }
            Ok(times)
        };

        let jit_o0_median_us = median_u128(run_jit(&jit_o0)?);
        let jit_o2_median_us = median_u128(run_jit(&jit_o2)?);

        Ok(BenchReport {
            iters,
            vm_median_us,
            aot_o0_compile_us,
            aot_o0_median_us,
            aot_o2_compile_us,
            aot_o2_median_us,
            jit_o0_compile_us,
            jit_o0_median_us,
            jit_o2_compile_us,
            jit_o2_median_us,
            aot_o2_speedup_x100: speedup_x100(vm_median_us, aot_o2_median_us),
            jit_o2_speedup_x100: speedup_x100(vm_median_us, jit_o2_median_us),
            parity_ok: true,
            stdout: vm_out,
        })
    })();

    let _ = std::fs::remove_file(&exe_o0);
    let _ = std::fs::remove_file(&exe_o2);
    bench_res
}

// ============================================================================
// Genuine Fixed-Point Self-Hosting Bootstrap Verifier (`Stage 0 -> 1 -> 2 -> 3`)
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapVerifyReport {
    pub self_host_path: PathBuf,
    pub stage1_bytes: usize,
    pub stage2_bytes: usize,
    pub stage3_bytes: usize,
    pub stage1_sha256: String,
    pub stage2_sha256: String,
    pub stage3_sha256: String,
    pub fixed_point_verified: bool,
    pub sample_stdout_parity_verified: bool,
}

impl BootstrapVerifyReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "fixed_point_verified".to_string(),
            JsonValue::Bool(self.fixed_point_verified),
        );
        m.insert(
            "sample_stdout_parity_verified".to_string(),
            JsonValue::Bool(self.sample_stdout_parity_verified),
        );
        m.insert(
            "schema".to_string(),
            JsonValue::from_str_val("aod.dec.bootstrap_verify.v1"),
        );
        m.insert(
            "self_host_path".to_string(),
            JsonValue::from_str_val(self.self_host_path.display().to_string()),
        );
        m.insert(
            "stage1_bytes".to_string(),
            JsonValue::from_usize(self.stage1_bytes),
        );
        m.insert(
            "stage1_sha256".to_string(),
            JsonValue::from_str_val(self.stage1_sha256.clone()),
        );
        m.insert(
            "stage2_bytes".to_string(),
            JsonValue::from_usize(self.stage2_bytes),
        );
        m.insert(
            "stage2_sha256".to_string(),
            JsonValue::from_str_val(self.stage2_sha256.clone()),
        );
        m.insert(
            "stage3_bytes".to_string(),
            JsonValue::from_usize(self.stage3_bytes),
        );
        m.insert(
            "stage3_sha256".to_string(),
            JsonValue::from_str_val(self.stage3_sha256.clone()),
        );
        m.insert("status".to_string(), JsonValue::from_str_val("PASS"));
        JsonValue::Object(m)
    }
}

struct BootstrapTmpGuard(PathBuf);

impl Drop for BootstrapTmpGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const DEFAULT_SELF_HOST_SAMPLE_DEC: &str = "\
struct Pair:
    lhs: int
    rhs: int

dec classify:
    (x: int) where x > 0 -> int:
        x * 3
    (0: int) -> int:
        108
    (x: int) -> int:
        -x + 7

dec step_sum(acc: int) -> int:
    acc + 9

dec main() -> int:
    let anchor_ok = 00_8
    let p = Pair(14, -5)
    let c1 = classify(p.lhs)
    let c2 = classify(0)
    let c3 = classify(p.rhs)
    let w = walk(0, 6, step_sum)
    let role = p.lhs <=> p.rhs
    let m = mux(role, -10, 0, 25)
    emit c1
    emit c2
    emit c3
    emit w + m
    0
";

/// Execute the full 4-stage self-hosting bootstrap verification:
/// 1. `Stage 0` (`aodc build`) compiles `self_host/aodc_self.dec` -> `stage1`.
/// 2. `stage1` compiles `self_host/aodc_self.dec` -> `stage2` (standalone libc-free x86-64 ELF64).
/// 3. `stage2` compiles `self_host/aodc_self.dec` -> `stage3`.
/// 4. Strictly asserts `SHA256(stage2) == SHA256(stage3)` (byte-for-byte fixed point).
/// 5. Compiles a clean-break `.dec` program via `stage2` and asserts exact stdout parity against
///    both `aodc run` (Exact Reference VM) and `aodc jit` (LLVM 18 ORC LLJIT).
pub fn bootstrap_verify(
    self_host_src: Option<&Path>,
    sample_prog: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<BootstrapVerifyReport> {
    let resolved_src = match self_host_src {
        Some(p) => p.to_path_buf(),
        None => discover_default_s2_root().join("self_host/aodc_self.dec"),
    };
    if !resolved_src.is_file() {
        return Err(Diagnostic::new(
            DiagnosticCode::ModuleNotFound,
            format!(
                "self-host compiler source not found at {}",
                resolved_src.display()
            ),
        ));
    }

    let seq = BENCH_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let work_dir = std::env::temp_dir().join(format!("aodc_bootstrap_{}_{}", std::process::id(), seq));
    std::fs::create_dir_all(&work_dir).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!("failed creating bootstrap workspace: {e}"),
        )
    })?;
    let _guard = BootstrapTmpGuard(work_dir.clone());

    let stage1_exe = work_dir.join("aodc_stage1");
    let stage2_exe = work_dir.join("aodc_stage2");
    let stage3_exe = work_dir.join("aodc_stage3");

    // 1. Stage 0 -> Stage 1
    build_aot_file_with_options(&resolved_src, OptLevel::O2, &stage1_exe, options)?;
    let stage1_bytes_raw = std::fs::read(&stage1_exe).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!("failed reading stage1 binary: {e}"),
        )
    })?;
    let stage1_sha256 = Sha256::digest_hex(&stage1_bytes_raw);

    // 2. Stage 1 -> Stage 2
    let s1_out = Command::new(&stage1_exe)
        .args([
            "--fuel",
            "100000000",
            "--max-depth",
            "512",
            &resolved_src.to_string_lossy(),
            &stage2_exe.to_string_lossy(),
        ])
        .output()
        .map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::RuntimeError,
                format!("failed spawning stage1 compiler: {e}"),
            )
        })?;
    if !s1_out.status.success() {
        return Err(Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!(
                "stage1 failed compiling {}: status={}, stderr={}",
                resolved_src.display(),
                s1_out.status,
                String::from_utf8_lossy(&s1_out.stderr)
            ),
        ));
    }
    let stage2_bytes_raw = std::fs::read(&stage2_exe).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!("failed reading stage2 ELF binary: {e}"),
        )
    })?;
    let stage2_sha256 = Sha256::digest_hex(&stage2_bytes_raw);

    // 3. Stage 2 -> Stage 3 (standalone self-hosted ELF64 compiling itself!)
    let s2_out = Command::new(&stage2_exe)
        .args([
            resolved_src.as_os_str(),
            stage3_exe.as_os_str(),
        ])
        .output()
        .map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::RuntimeError,
                format!("failed spawning stage2 self-hosted ELF: {e}"),
            )
        })?;
    if !s2_out.status.success() {
        return Err(Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!(
                "stage2 self-hosted compiler failed compiling {}: status={}, stderr={}",
                resolved_src.display(),
                s2_out.status,
                String::from_utf8_lossy(&s2_out.stderr)
            ),
        ));
    }
    let stage3_bytes_raw = std::fs::read(&stage3_exe).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!("failed reading stage3 ELF binary: {e}"),
        )
    })?;
    let stage3_sha256 = Sha256::digest_hex(&stage3_bytes_raw);

    // 4. Fixed-point equality gate: SHA256(stage2) == SHA256(stage3)
    if stage2_bytes_raw != stage3_bytes_raw || stage2_sha256 != stage3_sha256 {
        return Err(Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!(
                "bootstrap fixed-point mismatch: stage2 ({} bytes, sha256={stage2_sha256}) != stage3 ({} bytes, sha256={stage3_sha256})",
                stage2_bytes_raw.len(),
                stage3_bytes_raw.len()
            ),
        ));
    }

    // 5. Behavioral parity gate: compile & run a clean-break `.dec` program via `stage2`
    let sample_dec_path = match sample_prog {
        Some(p) => p.to_path_buf(),
        None => {
            let p = work_dir.join("sample_verify.dec");
            std::fs::write(&p, DEFAULT_SELF_HOST_SAMPLE_DEC).map_err(|e| {
                Diagnostic::new(
                    DiagnosticCode::RuntimeError,
                    format!("failed writing sample Dec program: {e}"),
                )
            })?;
            p
        }
    };
    let sample_elf_path = work_dir.join("sample_verify_exe");
    let sample_compile = Command::new(&stage2_exe)
        .args([sample_dec_path.as_os_str(), sample_elf_path.as_os_str()])
        .output()
        .map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::RuntimeError,
                format!("failed running stage2 on sample Dec program: {e}"),
            )
        })?;
    if !sample_compile.status.success() {
        return Err(Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!(
                "stage2 failed compiling sample {}: status={}, stderr={}",
                sample_dec_path.display(),
                sample_compile.status,
                String::from_utf8_lossy(&sample_compile.stderr)
            ),
        ));
    }
    let sample_run = Command::new(&sample_elf_path).output().map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!("failed executing stage2-emitted sample ELF: {e}"),
        )
    })?;
    if !sample_run.status.success() {
        return Err(Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!(
                "stage2-emitted sample ELF exited non-zero: {}",
                 sample_run.status
            ),
        ));
    }
    let self_stdout = String::from_utf8_lossy(&sample_run.stdout).to_string();

    let vm_out = run_file(&sample_dec_path)?.to_stdout_string();
    let (jit_out, jit_res) = jit_file_with_options_captured(
        &sample_dec_path,
        OptLevel::O2,
        DEFAULT_FUEL,
        DEFAULT_MAX_DEPTH,
        options,
    )?;
    jit_res.map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e))?;

    if self_stdout != vm_out || self_stdout != jit_out {
        return Err(Diagnostic::new(
            DiagnosticCode::RuntimeError,
            format!(
                "stage2 stdout parity mismatch: stage2={self_stdout:?}, vm={vm_out:?}, jit={jit_out:?}"
            ),
        ));
    }

    Ok(BootstrapVerifyReport {
        self_host_path: resolved_src,
        stage1_bytes: stage1_bytes_raw.len(),
        stage2_bytes: stage2_bytes_raw.len(),
        stage3_bytes: stage3_bytes_raw.len(),
        stage1_sha256,
        stage2_sha256,
        stage3_sha256,
        fixed_point_verified: true,
        sample_stdout_parity_verified: true,
    })
}

// ============================================================================
// Dec Test Runner
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCaseResult {
    pub file: PathBuf,
    pub test_name: String,
    pub passed: bool,
    pub error: Option<String>,
}

pub fn run_dec_tests(target: &Path) -> DecResult<Vec<TestCaseResult>> {
    run_dec_tests_with_options(target, &LoadOptions::default())
}

pub fn run_dec_tests_with_options(
    target: &Path,
    options: &LoadOptions,
) -> DecResult<Vec<TestCaseResult>> {
    let mut files = Vec::new();
    if target.is_file() {
        files.push(target.to_path_buf());
    } else if target.is_dir() {
        collect_dec_files_sorted(target, &mut files)?;
    } else {
        return Err(Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("test path not found: {}", target.display()),
        ));
    }

    let mut results = Vec::new();
    for file in files {
        let sibling_stdout = file.with_extension("stdout");
        let sibling_expect_error = file.with_extension("expect_error");
        let has_stdout = sibling_stdout.is_file();
        let has_expect_error = sibling_expect_error.is_file();

        if has_stdout && has_expect_error {
            results.push(TestCaseResult {
                file,
                test_name: "expect_error".to_string(),
                passed: false,
                error: Some(
                    "ambiguous fixture: both sibling .stdout and .expect_error files exist"
                        .to_string(),
                ),
            });
            continue;
        }

        if has_expect_error {
            let expected_raw = match std::fs::read_to_string(&sibling_expect_error) {
                Ok(s) => s,
                Err(e) => {
                    results.push(TestCaseResult {
                        file,
                        test_name: "expect_error".to_string(),
                        passed: false,
                        error: Some(format!("failed reading .expect_error: {e}")),
                    });
                    continue;
                }
            };
            let expected = expected_raw.trim();
            if expected.is_empty() {
                results.push(TestCaseResult {
                    file,
                    test_name: "expect_error".to_string(),
                    passed: false,
                    error: Some(
                        ".expect_error file is empty or whitespace-only (non-empty diagnostic expectation required)"
                            .to_string(),
                    ),
                });
                continue;
            }

            let outcome = std::fs::read_to_string(&file)
                .map_err(|e| Diagnostic::new(DiagnosticCode::ParseError, e.to_string()))
                .and_then(|src| load_from_source_with_options(&src, file.parent(), options))
                .and_then(|loaded| check_unit(loaded, true))
                .and_then(|checked| {
                    Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)?.run_main()
                });

            match outcome {
                Ok(_) => results.push(TestCaseResult {
                    file,
                    test_name: "expect_error".to_string(),
                    passed: false,
                    error: Some(format!(
                        "expected failure matching {expected:?}, but program succeeded"
                    )),
                }),
                Err(diag) => {
                    let rendered = diag.to_string();
                    let code_name = format!("{:?}", diag.code);
                    if rendered.contains(expected) || code_name == expected {
                        results.push(TestCaseResult {
                            file,
                            test_name: "expect_error".to_string(),
                            passed: true,
                            error: None,
                        });
                    } else {
                        results.push(TestCaseResult {
                            file,
                            test_name: "expect_error".to_string(),
                            passed: false,
                            error: Some(format!(
                                "expected error matching {expected:?}, got {rendered}"
                            )),
                        });
                    }
                }
            }
            continue;
        }

        let source = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                results.push(TestCaseResult {
                    file,
                    test_name: "<read>".to_string(),
                    passed: false,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let loaded = match load_from_source_with_options(&source, file.parent(), options) {
            Ok(u) => u,
            Err(e) => {
                results.push(TestCaseResult {
                    file,
                    test_name: "<load>".to_string(),
                    passed: false,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let checked = match check_unit(loaded, false) {
            Ok(c) => c,
            Err(e) => {
                results.push(TestCaseResult {
                    file,
                    test_name: "<check>".to_string(),
                    passed: false,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };

        if !checked.test_functions.is_empty() {
            for tname in &checked.test_functions {
                let run_res = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                    .and_then(|ev| ev.run_entry_fn(tname));
                match run_res {
                    Ok(_) => results.push(TestCaseResult {
                        file: file.clone(),
                        test_name: tname.clone(),
                        passed: true,
                        error: None,
                    }),
                    Err(e) => results.push(TestCaseResult {
                        file: file.clone(),
                        test_name: tname.clone(),
                        passed: false,
                        error: Some(e.to_string()),
                    }),
                }
            }
        } else if checked.main_return_type.is_some() {
            let run_res = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .and_then(|ev| ev.run_main());
            match run_res {
                Ok(out) => {
                    if has_stdout {
                        match std::fs::read_to_string(&sibling_stdout) {
                            Ok(expected) => {
                                let actual = out.to_stdout_string();
                                if actual == expected {
                                    results.push(TestCaseResult {
                                        file: file.clone(),
                                        test_name: "main+stdout".to_string(),
                                        passed: true,
                                        error: None,
                                    });
                                } else {
                                    results.push(TestCaseResult {
                                        file: file.clone(),
                                        test_name: "main+stdout".to_string(),
                                        passed: false,
                                        error: Some(format!(
                                            "stdout mismatch against {}",
                                            sibling_stdout.display()
                                        )),
                                    });
                                }
                            }
                            Err(e) => results.push(TestCaseResult {
                                file: file.clone(),
                                test_name: "main+stdout".to_string(),
                                passed: false,
                                error: Some(e.to_string()),
                            }),
                        }
                    } else {
                        results.push(TestCaseResult {
                            file: file.clone(),
                            test_name: "main".to_string(),
                            passed: true,
                            error: None,
                        });
                    }
                }
                Err(e) => results.push(TestCaseResult {
                    file: file.clone(),
                    test_name: "main".to_string(),
                    passed: false,
                    error: Some(e.to_string()),
                }),
            }
        } else {
            results.push(TestCaseResult {
                file,
                test_name: "<none>".to_string(),
                passed: false,
                error: Some("no test_*() or main() functions found".to_string()),
            });
        }
    }
    Ok(results)
}

fn collect_dec_files_sorted(dir: &Path, out: &mut Vec<PathBuf>) -> DecResult<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::ParseError,
                format!("failed reading directory {}: {e}", dir.display()),
            )
        })?
        .filter_map(|e| e.ok().map(|de| de.path()))
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_dec_files_sorted(&p, out)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("dec") {
            out.push(p);
        }
    }
    Ok(())
}

fn discover_default_s2_root() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur: Option<&Path> = Some(cwd.as_path());
        for _ in 0..=6 {
            if let Some(dir) = cur {
                if dir.join("SOURCE_BINDING.json").is_file()
                    && dir
                        .join("bindings/aod/s2_vectors/rational_core.jsonl")
                        .is_file()
                {
                    return dir.to_path_buf();
                }
                cur = dir.parent();
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Verify all 30 Stage 2 (`S2@3122c94`) cross-language golden vectors.
pub fn verify_s2_dir(vectors_dir: Option<&Path>, binding_path: Option<&Path>) -> S2VerifyReport {
    let default_root = if vectors_dir.is_none() || binding_path.is_none() {
        Some(discover_default_s2_root())
    } else {
        None
    };
    let vdir = match vectors_dir {
        Some(p) => p.to_path_buf(),
        None => default_root
            .as_ref()
            .expect("default_root computed when vectors_dir is None")
            .join("bindings/aod/s2_vectors"),
    };
    let bpath = match binding_path {
        Some(p) => p.to_path_buf(),
        None => default_root
            .as_ref()
            .expect("default_root computed when binding_path is None")
            .join("SOURCE_BINDING.json"),
    };
    verify_s2_vectors_dir(&vdir, &bpath)
}

/// Verify all 30 S2/S4 golden vectors + 90-tick `wavelet_shedding_simulation.csv` + `aod-core`
/// crate policy audit, returning a validated portable `aod.s4.rust.report.v1` JSON document.
pub fn verify_s4_convergence_dir(
    vectors_dir: Option<&Path>,
    binding_path: Option<&Path>,
    fixture_path: Option<&Path>,
) -> Result<(JsonValue, String, bool), String> {
    let default_root = if vectors_dir.is_none() || binding_path.is_none() || fixture_path.is_none()
    {
        Some(discover_default_s2_root())
    } else {
        None
    };

    let vdir = match vectors_dir {
        Some(p) => p.to_path_buf(),
        None => default_root
            .as_ref()
            .expect("default_root computed when vectors_dir is None")
            .join("bindings/aod/s2_vectors"),
    };
    let bpath = match binding_path {
        Some(p) => p.to_path_buf(),
        None => default_root
            .as_ref()
            .expect("default_root computed when binding_path is None")
            .join("SOURCE_BINDING.json"),
    };
    let fpath = match fixture_path {
        Some(p) => p.to_path_buf(),
        None => default_root
            .as_ref()
            .expect("default_root computed when fixture_path is None")
            .join(S4_CYCLE_SHEDDING_CSV_REL),
    };

    let crate_root = bpath
        .parent()
        .map(|p| p.join("crates/aod-core"))
        .filter(|c| c.join("Cargo.toml").is_file())
        .or_else(|| {
            let root = default_root
                .clone()
                .unwrap_or_else(discover_default_s2_root);
            let cand = root.join("crates/aod-core");
            cand.join("Cargo.toml").is_file().then_some(cand)
        });

    let vectors = verify_s2_vectors_dir(&vdir, &bpath);
    let fixture = verify_s4_cycle_shedding_csv(&fpath, &bpath);
    let policy = crate_root.as_deref().map(audit_aod_core_policy);

    let mut extra_roots: Vec<(&Path, &str)> = vec![
        (vdir.as_path(), "<VECTORS_DIR>"),
        (bpath.as_path(), "<BINDING_PATH>"),
        (fpath.as_path(), "<FIXTURE_PATH>"),
    ];
    if let Some(ref cr) = crate_root {
        extra_roots.push((cr.as_path(), "<CRATE_ROOT>"));
    }
    if let Some(bp_parent) = bpath.parent() {
        extra_roots.push((bp_parent, "<REPO_ROOT>"));
    }

    let (report_val, passed) = build_s4_report(&vectors, &fixture, policy.as_ref(), &extra_roots)
        .map_err(|e| sanitize_error(&e.to_string(), &extra_roots))?;
    let pretty = serialize_s4_report(&report_val)
        .map_err(|e| sanitize_error(&e.to_string(), &extra_roots))?;
    Ok((report_val, pretty, passed))
}
