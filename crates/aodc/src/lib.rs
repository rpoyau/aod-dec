#![forbid(unsafe_code)]

//! `aodc`: Dec v2 frontend, compile-time firewalls, hermetic `DecIR` v1/v2 serializer &
//! verifier, exact reference evaluator, unboxed-first safe textual LLVM 18 IR lowering,
//! AOT ELF / ORC LLJIT execution, hardware discovery & zero-float learner, and CLI.

pub mod ast;
pub mod checker;
pub mod codegen;
pub mod diagnostics;
pub mod eval;
pub mod ir;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod sim;
pub mod types;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use aod_core::sha256::Sha256;
pub use aod_llvm::{
    run_all_hw_probes, BootstrapCi, CodegenPlan, HostHardwareInfo, HwProbeReport, LearnDecision,
    LearnSessionReport, OptLevel, OrcJitSession, PerfCapability, ProfileLoadOutcome,
    RetainedProfile, UnrollHint, POLICY_SCOPE,
};
pub use crate::modules::LoadOptions;

use crate::checker::{check_unit, CheckedUnit};
use crate::codegen::{lower_to_llvm_ir, lower_to_llvm_ir_with_plan};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::eval::{EvalOutput, Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use crate::ir::{dumps_ir, loads_ir};
use crate::modules::load_from_source_with_options;

static BENCH_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn check_source(source: &str, compat: CompatMode) -> DecResult<CheckedUnit> {
    check_source_with_options(source, compat, &LoadOptions::default())
}

pub fn check_source_with_options(
    source: &str,
    compat: CompatMode,
    options: &LoadOptions,
) -> DecResult<CheckedUnit> {
    let loaded = load_from_source_with_options(source, None, compat, options)?;
    check_unit(loaded, true)
}

pub fn check_file(path: &Path, compat: CompatMode) -> DecResult<CheckedUnit> {
    check_file_with_options(path, compat, &LoadOptions::default())
}

pub fn check_file_with_options(
    path: &Path,
    compat: CompatMode,
    options: &LoadOptions,
) -> DecResult<CheckedUnit> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("failed reading {}: {e}", path.display()),
        )
    })?;
    let base_dir = path.parent();
    let loaded = load_from_source_with_options(&source, base_dir, compat, options)?;
    check_unit(loaded, true)
}

pub fn run_source(source: &str, compat: CompatMode) -> DecResult<EvalOutput> {
    let checked = check_source(source, compat)?;
    Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)?.run_main()
}

pub fn run_source_with_budget(
    source: &str,
    compat: CompatMode,
    fuel: usize,
    max_depth: usize,
) -> DecResult<EvalOutput> {
    let checked = check_source(source, compat)?;
    Evaluator::new(&checked, fuel, max_depth)?.run_main()
}

pub fn run_file(path: &Path, compat: CompatMode) -> DecResult<EvalOutput> {
    let checked = check_file(path, compat)?;
    Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)?.run_main()
}

pub fn run_file_with_budget_capturing(
    path: &Path,
    compat: CompatMode,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(Vec<String>, DecResult<crate::eval::Value>)> {
    run_file_with_options_capturing(path, compat, fuel, max_depth, &LoadOptions::default())
}

pub fn run_file_with_options_capturing(
    path: &Path,
    compat: CompatMode,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
) -> DecResult<(Vec<String>, DecResult<crate::eval::Value>)> {
    let checked = check_file_with_options(path, compat, options)?;
    Ok(Evaluator::new(&checked, fuel, max_depth)?.run_main_capturing())
}

pub fn compile_file_to_ir(path: &Path, compat: CompatMode) -> DecResult<String> {
    compile_file_to_ir_with_options(path, compat, &LoadOptions::default())
}

pub fn compile_file_to_ir_with_options(
    path: &Path,
    compat: CompatMode,
    options: &LoadOptions,
) -> DecResult<String> {
    let checked = check_file_with_options(path, compat, options)?;
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
// PR 2 + PR 3 + PR 4: Native LLVM 18 Lowering, AOT ELF, ORC LLJIT, & Learner
// ============================================================================

pub fn emit_llvm_source(source: &str, compat: CompatMode, opt: OptLevel) -> DecResult<String> {
    emit_llvm_source_with_plan(source, compat, &CodegenPlan::for_opt_level(opt))
}

pub fn emit_llvm_source_with_plan(
    source: &str,
    compat: CompatMode,
    plan: &CodegenPlan,
) -> DecResult<String> {
    let checked = check_source(source, compat)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    aod_llvm::verify_and_optimize_ir_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn emit_llvm_file(path: &Path, compat: CompatMode, opt: OptLevel) -> DecResult<String> {
    emit_llvm_file_with_options(path, compat, opt, &LoadOptions::default())
}

pub fn emit_llvm_file_with_options(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
    options: &LoadOptions,
) -> DecResult<String> {
    emit_llvm_file_with_plan(path, compat, &CodegenPlan::for_opt_level(opt), options)
}

pub fn emit_llvm_file_with_plan(
    path: &Path,
    compat: CompatMode,
    plan: &CodegenPlan,
    options: &LoadOptions,
) -> DecResult<String> {
    let checked = check_file_with_options(path, compat, options)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    aod_llvm::verify_and_optimize_ir_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn build_aot_source(
    source: &str,
    compat: CompatMode,
    opt: OptLevel,
    out_exe: &Path,
) -> DecResult<()> {
    let checked = check_source(source, compat)?;
    let raw_ir = lower_to_llvm_ir(&checked)?;
    aod_llvm::link_aot_executable(&raw_ir, opt, out_exe)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn build_aot_file(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
    out_exe: &Path,
) -> DecResult<()> {
    build_aot_file_with_options(path, compat, opt, out_exe, &LoadOptions::default())
}

pub fn build_aot_file_with_options(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
    out_exe: &Path,
    options: &LoadOptions,
) -> DecResult<()> {
    let checked = check_file_with_options(path, compat, options)?;
    let plan = CodegenPlan::for_opt_level(opt);
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, &plan)?;
    aod_llvm::link_aot_executable_with_plan(&raw_ir, &plan, out_exe)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn compile_jit_source(
    source: &str,
    compat: CompatMode,
    opt: OptLevel,
) -> DecResult<OrcJitSession> {
    compile_jit_source_with_plan(source, compat, &CodegenPlan::for_opt_level(opt))
}

pub fn compile_jit_source_with_plan(
    source: &str,
    compat: CompatMode,
    plan: &CodegenPlan,
) -> DecResult<OrcJitSession> {
    let checked = check_source(source, compat)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    OrcJitSession::compile_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn compile_jit_file(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
) -> DecResult<OrcJitSession> {
    compile_jit_file_with_options(path, compat, opt, &LoadOptions::default())
}

pub fn compile_jit_file_with_options(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
    options: &LoadOptions,
) -> DecResult<OrcJitSession> {
    compile_jit_file_with_plan(path, compat, &CodegenPlan::for_opt_level(opt), options)
}

pub fn compile_jit_file_with_plan(
    path: &Path,
    compat: CompatMode,
    plan: &CodegenPlan,
    options: &LoadOptions,
) -> DecResult<OrcJitSession> {
    let checked = check_file_with_options(path, compat, options)?;
    let raw_ir = lower_to_llvm_ir_with_plan(&checked, plan)?;
    OrcJitSession::compile_with_plan(&raw_ir, plan)
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))
}

pub fn jit_source_captured(
    source: &str,
    compat: CompatMode,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(String, Result<(), String>)> {
    let session = compile_jit_source(source, compat, opt)?;
    Ok(session.run_captured(fuel as i64, max_depth as i32))
}

pub fn jit_source_with_plan_captured(
    source: &str,
    compat: CompatMode,
    plan: &CodegenPlan,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(String, Result<(), String>)> {
    let session = compile_jit_source_with_plan(source, compat, plan)?;
    Ok(session.run_captured(fuel as i64, max_depth as i32))
}

pub fn jit_file_captured(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(String, Result<(), String>)> {
    jit_file_with_options_captured(path, compat, opt, fuel, max_depth, &LoadOptions::default())
}

pub fn jit_file_with_options_captured(
    path: &Path,
    compat: CompatMode,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
) -> DecResult<(String, Result<(), String>)> {
    let session = compile_jit_file_with_options(path, compat, opt, options)?;
    Ok(session.run_captured(fuel as i64, max_depth as i32))
}

pub fn compute_profile_bindings(
    checked: &CheckedUnit,
    compat: CompatMode,
    fuel: usize,
    max_depth: usize,
) -> (String, String) {
    let ir_json = dumps_ir(checked);
    let kernel_ir_sha256 = Sha256::digest_hex(ir_json.as_bytes());
    let workload_desc = format!("compat={compat:?};fuel={fuel};max_depth={max_depth}");
    let workload_sha256 = Sha256::digest_hex(workload_desc.as_bytes());
    (kernel_ir_sha256, workload_sha256)
}

/// Execute via ORC LLJIT honoring an optional validated `RetainedProfile` (falling back safely
/// to `fallback_opt` on any corruption, tampering, or CPU/IR/workload mismatch!).
#[allow(clippy::too_many_arguments)]
pub fn jit_file_with_profile_captured(
    path: &Path,
    compat: CompatMode,
    fallback_opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    profile_path: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<(String, Result<(), String>, CodegenPlan, Option<String>)> {
    let checked = check_file_with_options(path, compat, options)?;
    let fallback_plan = CodegenPlan::for_opt_level(fallback_opt);

    let (chosen_plan, note) = if let Some(pp) = profile_path {
        let hw = HostHardwareInfo::discover()
            .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
        let (kernel_sha, workload_sha) = compute_profile_bindings(&checked, compat, fuel, max_depth);
        match RetainedProfile::load_validated(pp, &hw.cpu_sig_sha256, &kernel_sha, &workload_sha) {
            ProfileLoadOutcome::Loaded(prof) => (prof.active_plan, None),
            ProfileLoadOutcome::FallbackToBaseline { reason } => (fallback_plan, Some(reason)),
        }
    } else {
        (fallback_plan, None)
    };

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
    compat: CompatMode,
    baseline_opt: OptLevel,
    training_pairs: usize,
    heldout_pairs: usize,
    fuel: usize,
    max_depth: usize,
    profile_out: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<(HwProbeReport, LearnSessionReport)> {
    let checked = check_file_with_options(path, compat, options)?;
    let hw_report = run_all_hw_probes()
        .map_err(|e| Diagnostic::new(DiagnosticCode::RuntimeError, e.to_string()))?;
    let (kernel_ir_sha256, workload_sha256) =
        compute_profile_bindings(&checked, compat, fuel, max_depth);

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

pub fn bench_file(
    path: &Path,
    compat: CompatMode,
    iters: usize,
    fuel: usize,
    max_depth: usize,
) -> DecResult<BenchReport> {
    bench_file_with_options(path, compat, iters, fuel, max_depth, &LoadOptions::default())
}

pub fn bench_file_with_options(
    path: &Path,
    compat: CompatMode,
    iters: usize,
    fuel: usize,
    max_depth: usize,
    options: &LoadOptions,
) -> DecResult<BenchReport> {
    let iters = iters.max(1);
    let checked = check_file_with_options(path, compat, options)?;
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
// Dec Test Runner (PR 1 `P0-3` + PR 3 `P1-1` hardened `.expect_error`!)
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCaseResult {
    pub file: PathBuf,
    pub test_name: String,
    pub passed: bool,
    pub error: Option<String>,
}

pub fn run_dec_tests(target: &Path, compat: CompatMode) -> DecResult<Vec<TestCaseResult>> {
    run_dec_tests_with_options(target, compat, &LoadOptions::default())
}

pub fn run_dec_tests_with_options(
    target: &Path,
    compat: CompatMode,
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
                .and_then(|src| {
                    load_from_source_with_options(&src, file.parent(), compat, options)
                })
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
        let loaded = match load_from_source_with_options(&source, file.parent(), compat, options) {
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
