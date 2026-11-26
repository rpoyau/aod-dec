#![forbid(unsafe_code)]

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aod_core::json::{parse_strict_json, JsonValue};
use aodc::eval::{Value, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::{
    audit_dec_file_with_ns_and_options, audit_pdf_file, bench_file_with_options,
    build_aot_file_with_ns_and_options, check_file_with_ns_and_options, check_v1_file,
    collect_guarded_roots, compile_file_to_ir_with_ns_and_options, compile_v1_file_to_ir,
    emit_llvm_file_with_ns_and_options, eval_operation, exec_ir_file_with_budget_capturing,
    format_source, format_x100, jit_file_with_ns_and_bootstrap_captured, learn_file_with_options,
    migrate_source_formatted, read_bounded_regular_file, read_bounded_stdin, run_all_hw_probes,
    run_dec_tests_with_compat, run_file_with_ns_and_bootstrap_capturing, run_manual_data,
    run_v1_file_capturing, validate_runtime_output_path, verify_manual_dir, verify_s2_dir,
    verify_s4_convergence_dir, write_report_atomic, CompatMode, LoadOptions, ManualDataOptions,
    ManualVerifyOptions, OptLevel, PdfAuditOptions, RtMountSpec, RtNamespace,
    MAX_MIGRATE_SOURCE_BYTES, MAX_S4_INPUT_BYTES,
};

struct CliErr {
    code: u8,
    msg: String,
}

impl CliErr {
    fn fail(msg: impl Into<String>) -> Self {
        Self {
            code: 1,
            msg: msg.into(),
        }
    }

    fn usage(msg: impl Into<String>) -> Self {
        Self {
            code: 2,
            msg: msg.into(),
        }
    }
}

impl From<String> for CliErr {
    fn from(msg: String) -> Self {
        Self::fail(msg)
    }
}

fn extract_compat_flag(args: &[String]) -> Result<(CompatMode, Vec<String>), String> {
    let mut compat = CompatMode::V3;
    let mut seen = false;
    let mut filtered = Vec::with_capacity(args.len());
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--compat" {
            if seen {
                return Err("duplicate flag --compat".to_string());
            }
            let val = args
                .get(i + 1)
                .ok_or_else(|| "missing mode after --compat (expected v1 or v3)".to_string())?;
            compat = CompatMode::parse_cli(val)?;
            seen = true;
            i += 2;
        } else if let Some(val) = args[i].strip_prefix("--compat=") {
            if seen {
                return Err("duplicate flag --compat".to_string());
            }
            compat = CompatMode::parse_cli(val)?;
            seen = true;
            i += 1;
        } else {
            filtered.push(args[i].clone());
            i += 1;
        }
    }
    Ok((compat, filtered))
}

fn canonicalize_allowing_new_file(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return std::fs::canonicalize(path);
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let parent = abs.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let canon_parent = std::fs::canonicalize(parent)?;
    let name = abs.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
    })?;
    Ok(canon_parent.join(name))
}

fn ensure_out_differs_from_src(src_path: &Path, out_path: &Path) -> Result<(), String> {
    let canon_src = std::fs::canonicalize(src_path).map_err(|e| format!("source error: {e}"))?;
    let canon_out =
        canonicalize_allowing_new_file(out_path).map_err(|e| format!("output path error: {e}"))?;
    if canon_src == canon_out {
        return Err("output path must differ from source path".to_string());
    }
    Ok(())
}

fn ensure_not_frozen_guarded_target(target: &Path) -> Result<(), String> {
    let canon =
        canonicalize_allowing_new_file(target).map_err(|e| format!("output path error: {e}"))?;
    for root in aodc::collect_guarded_roots(&canon, &[]) {
        for rel in [
            "tests/fixtures/v1_examples",
            "bindings/aod",
            "reference/aod-dec-dev",
            "SOURCE_BINDING.json",
        ] {
            let guarded = root.join(rel);
            if canon == guarded || canon.starts_with(&guarded) {
                return Err(format!(
                    "refusing to overwrite frozen provenance artifact under {rel}"
                ));
            }
        }
    }
    Ok(())
}

fn parse_positive_usize(flag: &str, raw: Option<&String>) -> Result<usize, String> {
    let val = raw.ok_or_else(|| format!("missing positive integer after {flag}"))?;
    let parsed: usize = val
        .parse()
        .map_err(|_| format!("invalid integer {val:?} for {flag}; expected positive integer"))?;
    if parsed == 0 {
        return Err(format!("{flag} must be a positive integer (>= 1)"));
    }
    Ok(parsed)
}

fn parse_opt_level(arg: &str, next: Option<&String>) -> Result<(OptLevel, usize), String> {
    if let Some(rest) = arg.strip_prefix("-O") {
        if rest.is_empty() {
            let v = next.ok_or_else(|| {
                "missing optimization level after -O (expected 0..3)".to_string()
            })?;
            let n: u8 = v
                .parse()
                .map_err(|_| format!("invalid optimization level {v:?} (expected 0..3)"))?;
            Ok((OptLevel::from_u8(n).map_err(|e| e.to_string())?, 2))
        } else {
            let n: u8 = rest
                .parse()
                .map_err(|_| format!("invalid optimization level -O{rest} (expected 0..3)"))?;
            Ok((OptLevel::from_u8(n).map_err(|e| e.to_string())?, 1))
        }
    } else {
        Err(format!("unexpected optimization flag {arg:?}"))
    }
}

struct ExecOptions {
    opt: OptLevel,
    baseline_opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    iters: usize,
    pairs: usize,
    heldout: usize,
    output: Option<String>,
    profile: Option<String>,
    parallel_observers: bool,
    bootstrap_ns: Option<PathBuf>,
    load: LoadOptions,
    rest: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn parse_options(
    args: &[String],
    allow_opt: bool,
    allow_budget: bool,
    allow_output: bool,
    allow_iters: bool,
    allow_stdlib: bool,
    allow_profile: bool,
    allow_learn: bool,
    allow_parallel: bool,
) -> Result<ExecOptions, String> {
    let mut opt = OptLevel::O2;
    let mut baseline_opt = OptLevel::O0;
    let mut fuel = DEFAULT_FUEL;
    let mut max_depth = DEFAULT_MAX_DEPTH;
    let mut iters = 5usize;
    let mut pairs = 5usize;
    let mut heldout = 5usize;
    let mut output = None;
    let mut profile = None;
    let mut parallel_observers = false;
    let mut stdlib_dir = None;
    let mut bootstrap_ns = None;
    let mut rest = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--compat" || a.starts_with("--compat=") {
            return Err("--compat is only supported on `check`, `run`, `compile`, and `test`".to_string());
        } else if a == "--strict-wave" || a == "--pure-wave" {
            i += 1;
        } else if allow_stdlib && a == "--stdlib" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| "missing directory path after --stdlib".to_string())?;
            stdlib_dir = Some(PathBuf::from(val));
            i += 2;
        } else if allow_stdlib && a == "--namespace" {
            if bootstrap_ns.is_some() {
                return Err("duplicate flag --namespace".to_string());
            }
            let val = args
                .get(i + 1)
                .ok_or_else(|| "missing bootstrap .dec file path after --namespace".to_string())?;
            bootstrap_ns = Some(PathBuf::from(val));
            i += 2;
        } else if allow_opt && a.starts_with("-O") {
            let (parsed, adv) = parse_opt_level(a, args.get(i + 1))?;
            opt = parsed;
            i += adv;
        } else if allow_budget && a == "--fuel" {
            fuel = parse_positive_usize("--fuel", args.get(i + 1))?;
            i += 2;
        } else if allow_budget && a == "--max-depth" {
            max_depth = parse_positive_usize("--max-depth", args.get(i + 1))?;
            i += 2;
        } else if allow_output && (a == "-o" || a == "--output") {
            let v = args
                .get(i + 1)
                .ok_or_else(|| "missing path after -o/--output".to_string())?;
            output = Some(v.clone());
            i += 2;
        } else if allow_iters && a == "--iters" {
            iters = parse_positive_usize("--iters", args.get(i + 1))?;
            i += 2;
        } else if allow_profile && a == "--profile" {
            let v = args
                .get(i + 1)
                .ok_or_else(|| "missing path after --profile".to_string())?;
            profile = Some(v.clone());
            i += 2;
        } else if allow_parallel && a == "--parallel-observers" {
            parallel_observers = true;
            i += 1;
        } else if allow_learn && a == "--baseline" {
            let v = args
                .get(i + 1)
                .ok_or_else(|| "missing value after --baseline (expected O0 or O2)".to_string())?;
            baseline_opt = match v.as_str() {
                "O0" | "0" => OptLevel::O0,
                "O2" | "2" => OptLevel::O2,
                other => {
                    return Err(format!(
                        "invalid --baseline {other:?} (expected O0 or O2)"
                    ));
                }
            };
            i += 2;
        } else if allow_learn && a == "--pairs" {
            pairs = parse_positive_usize("--pairs", args.get(i + 1))?;
            i += 2;
        } else if allow_learn && a == "--heldout" {
            heldout = parse_positive_usize("--heldout", args.get(i + 1))?;
            i += 2;
        } else if a.starts_with('-') {
            return Err(format!("unknown flag {a:?}\n{}", usage()));
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    Ok(ExecOptions {
        opt,
        baseline_opt,
        fuel,
        max_depth,
        iters,
        pairs,
        heldout,
        output,
        profile,
        parallel_observers,
        bootstrap_ns,
        load: LoadOptions { stdlib_dir },
        rest,
    })
}

fn flush_emits_and_result(
    emits: Vec<String>,
    outcome: aodc::diagnostics::DecResult<Value>,
) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    for line in emits {
        let _ = writeln!(stdout, "{line}");
    }
    let _ = stdout.flush();
    match outcome {
        Ok(val) => {
            if val != Value::Unit {
                let _ = writeln!(stdout, "{}", val.render());
                let _ = stdout.flush();
            }
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn extract_ns_and_prog_args(
    args: &[String],
) -> Result<(RtNamespace, Vec<String>, Vec<String>), String> {
    let mut before_ddash = Vec::new();
    let mut after_ddash = Vec::new();
    let mut saw_ddash = false;
    for a in args {
        if saw_ddash {
            after_ddash.push(a.clone());
        } else if a == "--" {
            saw_ddash = true;
        } else {
            before_ddash.push(a.clone());
        }
    }

    let mut specs = Vec::new();
    let mut unconfined = false;
    let mut filtered = Vec::new();
    let mut i = 0usize;
    while i < before_ddash.len() {
        match before_ddash[i].as_str() {
            "--unconfined" => {
                unconfined = true;
                i += 1;
            }
            "--mount" => {
                let raw = before_ddash
                    .get(i + 1)
                    .ok_or_else(|| "missing spec after --mount".to_string())?;
                specs.push(RtMountSpec::parse_cli(raw)?);
                i += 2;
            }
            _ => {
                filtered.push(before_ddash[i].clone());
                i += 1;
            }
        }
    }

    if unconfined && !specs.is_empty() {
        return Err("--unconfined cannot be combined with --mount".to_string());
    }
    let mut rt_ns = if unconfined {
        RtNamespace::unconfined_host()
    } else {
        RtNamespace::empty()
    };
    for spec in &specs {
        rt_ns.apply_spec(spec)?;
    }
    Ok((rt_ns, filtered, after_ddash))
}

fn build_prog_cli_args(prog_name: &str, prog_tail: &[String]) -> Vec<Vec<u8>> {
    let mut out = Vec::with_capacity(1 + prog_tail.len());
    out.push(prog_name.as_bytes().to_vec());
    for a in prog_tail {
        out.push(a.as_bytes().to_vec());
    }
    out
}

fn usage() -> &'static str {
    "Usage:\n  aodc check [--compat v1|v3] [--stdlib <dir>] [--namespace <ns.dec>] <source.dec>\n  aodc run [--compat v1|v3] [--stdlib <dir>] [--namespace <ns.dec>] [--fuel <N>] [--max-depth <N>] [--mount <spec>] [--unconfined] [--parallel-observers] <source.dec> [-- <args>...]\n  aodc compile [--compat v1|v3] [--stdlib <dir>] [--namespace <ns.dec>] <source.dec> -o <out.ir.json>\n  aodc exec [--fuel <N>] [--max-depth <N>] <program.ir.json>\n  aodc emit-llvm [--stdlib <dir>] [--namespace <ns.dec>] [-O 0|1|2|3] <source.dec> [-o <out.ll>]\n  aodc build [--stdlib <dir>] [--namespace <ns.dec>] [-O 0|1|2|3] <source.dec> -o <out_exe>\n  aodc jit [--stdlib <dir>] [--namespace <ns.dec>] [-O 0|1|2|3] [--fuel <N>] [--max-depth <N>] [--profile <p.json>] [--mount <spec>] [--unconfined] [--parallel-observers] <source.dec> [-- <args>...]\n  aodc bench [--stdlib <dir>] [--iters <N>] [--fuel <N>] [--max-depth <N>] <source.dec>\n  aodc probe-hw [--output <report.json>]\n  aodc learn [--stdlib <dir>] [--baseline O0|O2] [--pairs <N>] [--heldout <N>] [--fuel <N>] [--max-depth <N>] [--profile <p.json>] <source.dec>\n  aodc bootstrap-verify [--source <aodc_self.dec>] [--sample <prog.dec>] [--emit-stage2 <out_exe>] [--stdlib <dir>] [--json]\n  aodc manual-data [--check] [--output-dir <dir>] [--stdlib <dir>] [--json]\n  aodc manual-verify [--stdlib <dir>] [--registry <path>] [--json] <manual_dir>\n  aodc verify-s2 [--vectors <dir>] [--binding <path>] [--fixture <csv>] [--s4-report] [--report <path>] [--json]\n  aodc eval-op <op> (--input-json <json> | --input <file> | --input-stdin) [--report <path>]\n  aodc audit-pdf [--min-pages <N>] [--compare-pdf <pdf2>] [--json] <pdf>\n  aodc fmt [--check | --write | -o <out.dec>] <source.dec>\n  aodc migrate [--check | --write | -o <out.dec>] <source.dec>\n  aodc audit-dec [--stdlib <dir>] [--namespace <ns.dec>] [--report <path>] [--json] <source.dec>\n  aodc test [--compat v1|v3] [--stdlib <dir>] <file_or_dir>"
}

fn main() -> ExitCode {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    if raw_args.is_empty() || raw_args[0] == "-h" || raw_args[0] == "--help" {
        println!("{}", usage());
        return if raw_args.is_empty() {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        };
    }

    let cmd = raw_args[0].as_str();
    let sub_args = &raw_args[1..];

    let result: Result<(), CliErr> = match cmd {
        "check" => extract_compat_flag(sub_args)
            .and_then(|(compat, rem)| {
                let opts =
                    parse_options(&rem, false, false, false, false, true, false, false, false)?;
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let src_path = Path::new(&opts.rest[0]);
                match compat {
                    CompatMode::V1 => {
                        check_v1_file(src_path).map_err(|e| e.to_string())?;
                    }
                    CompatMode::V3 => {
                        check_file_with_ns_and_options(
                            src_path,
                            opts.bootstrap_ns.as_deref(),
                            &opts.load,
                        )
                        .map_err(|e| e.to_string())?;
                    }
                }
                println!("OK: Dec");
                Ok(())
            })
            .map_err(CliErr::fail),
        "run" => extract_compat_flag(sub_args)
            .and_then(|(compat, rem)| {
                let (rt_ns, rem2, prog_tail) = extract_ns_and_prog_args(&rem)?;
                let opts =
                    parse_options(&rem2, false, true, false, false, true, false, false, true)?;
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let src_path = Path::new(&opts.rest[0]);
                let cli_args = build_prog_cli_args(&opts.rest[0], &prog_tail);
                let (emits, outcome) = match compat {
                    CompatMode::V1 => run_v1_file_capturing(src_path, opts.fuel, opts.max_depth)
                        .map_err(|e| e.to_string())?,
                    CompatMode::V3 => run_file_with_ns_and_bootstrap_capturing(
                        src_path,
                        opts.bootstrap_ns.as_deref(),
                        opts.fuel,
                        opts.max_depth,
                        &opts.load,
                        opts.parallel_observers,
                        rt_ns,
                        cli_args,
                    )
                    .map_err(|e| e.to_string())?,
                };
                flush_emits_and_result(emits, outcome)
            })
            .map_err(CliErr::fail),
        "compile" => extract_compat_flag(sub_args)
            .and_then(|(compat, rem)| {
                let opts =
                    parse_options(&rem, false, false, true, false, true, false, false, false)?;
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let src_path = PathBuf::from(&opts.rest[0]);
                let out_path = PathBuf::from(
                    opts.output
                        .ok_or_else(|| "missing -o <out.ir.json>".to_string())?,
                );
                ensure_out_differs_from_src(&src_path, &out_path)?;
                let ir_text = match compat {
                    CompatMode::V1 => {
                        compile_v1_file_to_ir(&src_path).map_err(|e| e.to_string())?
                    }
                    CompatMode::V3 => compile_file_to_ir_with_ns_and_options(
                        &src_path,
                        opts.bootstrap_ns.as_deref(),
                        &opts.load,
                    )
                    .map_err(|e| e.to_string())?,
                };
                std::fs::write(&out_path, ir_text).map_err(|e| format!("write error: {e}"))?;
                Ok(())
            })
            .map_err(CliErr::fail),
        "exec" => parse_options(sub_args, false, true, false, false, false, false, false, false)
            .and_then(|opts| {
                if opts.rest.len() != 1 {
                    return Err(format!("expected <program.ir.json>\n{}", usage()));
                }
                let (emits, outcome) = exec_ir_file_with_budget_capturing(
                    Path::new(&opts.rest[0]),
                    opts.fuel,
                    opts.max_depth,
                )
                .map_err(|e| e.to_string())?;
                flush_emits_and_result(emits, outcome)
            })
            .map_err(CliErr::fail),
        "emit-llvm" | "emit-ir" => {
            parse_options(sub_args, true, false, true, false, true, false, false, false)
                .and_then(|opts| {
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let src_path = PathBuf::from(&opts.rest[0]);
                if let Some(ref out) = opts.output {
                    ensure_out_differs_from_src(&src_path, Path::new(out))?;
                }
                let ir = emit_llvm_file_with_ns_and_options(
                    &src_path,
                    opts.bootstrap_ns.as_deref(),
                    opts.opt,
                    &opts.load,
                )
                .map_err(|e| e.to_string())?;
                if let Some(out) = opts.output {
                    std::fs::write(&out, ir).map_err(|e| format!("write error: {e}"))?;
                } else {
                    print!("{ir}");
                }
                Ok(())
            })
            .map_err(CliErr::fail)
        }
        "build" => parse_options(sub_args, true, false, true, false, true, false, false, false)
            .and_then(|opts| {
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let src_path = PathBuf::from(&opts.rest[0]);
                let out_path = PathBuf::from(
                    opts.output
                        .ok_or_else(|| "missing -o <out_exe>".to_string())?,
                );
                ensure_out_differs_from_src(&src_path, &out_path)?;
                build_aot_file_with_ns_and_options(
                    &src_path,
                    opts.bootstrap_ns.as_deref(),
                    opts.opt,
                    &out_path,
                    &opts.load,
                )
                .map_err(|e| e.to_string())
            })
            .map_err(CliErr::fail),
        "jit" => extract_ns_and_prog_args(sub_args)
            .and_then(|(rt_ns, rem, prog_tail)| {
                let opts =
                    parse_options(&rem, true, true, false, false, true, true, false, true)?;
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let profile_path = opts.profile.as_deref().map(Path::new);
                let cli_args = build_prog_cli_args(&opts.rest[0], &prog_tail);
                let (stdout, res, _plan, fallback_note) = jit_file_with_ns_and_bootstrap_captured(
                    Path::new(&opts.rest[0]),
                    opts.bootstrap_ns.as_deref(),
                    opts.opt,
                    opts.fuel,
                    opts.max_depth,
                    profile_path,
                    &opts.load,
                    opts.parallel_observers,
                    rt_ns,
                    cli_args,
                )
                .map_err(|e| e.to_string())?;
                if let Some(note) = fallback_note {
                    eprintln!("aodc: profile fallback to baseline: {note}");
                }
                {
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(stdout.as_bytes());
                    let _ = out.flush();
                }
                res
            })
            .map_err(CliErr::fail),
        "bench" => parse_options(sub_args, false, true, false, true, true, false, false, false)
            .and_then(|opts| {
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let r = bench_file_with_options(
                    Path::new(&opts.rest[0]),
                    opts.iters,
                    opts.fuel,
                    opts.max_depth,
                    &opts.load,
                )
                .map_err(|e| e.to_string())?;
                println!("aodc bench ({}, iters={})", opts.rest[0], r.iters);
                println!("  parity:       5/5 byte-identical (VM == AOT-O0 == AOT-O2 == JIT-O0 == JIT-O2)");
                println!("  vm_median_us: {} us", r.vm_median_us);
                println!(
                    "  aot_o0:       compile={} us, median={} us",
                    r.aot_o0_compile_us, r.aot_o0_median_us
                );
                println!(
                    "  aot_o2:       compile={} us, median={} us (speedup {})",
                    r.aot_o2_compile_us,
                    r.aot_o2_median_us,
                    format_x100(r.aot_o2_speedup_x100)
                );
                println!(
                    "  jit_o0:       compile={} us, median={} us",
                    r.jit_o0_compile_us, r.jit_o0_median_us
                );
                println!(
                    "  jit_o2:       compile={} us, median={} us (speedup {})",
                    r.jit_o2_compile_us,
                    r.jit_o2_median_us,
                    format_x100(r.jit_o2_speedup_x100)
                );
                Ok(())
            })
            .map_err(CliErr::fail),
        "probe-hw" => parse_options(sub_args, false, false, true, false, false, false, false, false)
            .and_then(|opts| {
                if !opts.rest.is_empty() {
                    return Err(format!(
                        "unexpected positional arguments {:?}\n{}",
                        opts.rest,
                        usage()
                    ));
                }
                let report = run_all_hw_probes().map_err(|e| e.to_string())?;
                let json_text = format!("{}\n", report.to_json().to_compact_ascii_json());
                if let Some(out) = opts.output {
                    std::fs::write(&out, &json_text).map_err(|e| format!("write error: {e}"))?;
                }
                println!("aodc probe-hw [{}]", report.policy_scope);
                println!(
                    "  cpu:           {} ({}, cores={}, llvm_cpu={})",
                    report.hardware.model_name,
                    report.hardware.arch,
                    report.hardware.logical_cores,
                    report.hardware.llvm_host_cpu
                );
                println!("  cpu_sig:       {}", report.hardware.cpu_sig_sha256);
                println!(
                    "  caches:        L1d={}B, L2={}B, L3={}B, line={}B",
                    report.hardware.l1d_bytes,
                    report.hardware.l2_bytes,
                    report.hardware.l3_bytes,
                    report.hardware.cache_line_bytes
                );
                println!(
                    "  clocks:        {} (perf_event_open={}, paranoid={:?})",
                    report.clock_source,
                    report.perf_event_open.as_str(),
                    report.hardware.perf_event_paranoid
                );
                println!(
                    "  memory_tiers:  recommended_l1_working_set={}B (verified={})",
                    report.memory_tiers.recommended_l1_working_set_bytes,
                    report.memory_tiers.verified
                );
                println!(
                    "  exact_arith:   i64={}*0.01ns, trit_swar={}*0.01ns, ratio={}*0.01ns, bigint_spill={}*0.01ns (spill/fast={}x/100)",
                    report.exact_arith.i64_checked_add_ns_per_op_x100,
                    report.exact_arith.t32_checked_add_ns_per_op_x100,
                    report.exact_arith.ratio_inline_gcd_ns_per_op_x100,
                    report.exact_arith.bigint_spill_add_ns_per_op_x100,
                    report.exact_arith.spill_to_fast_ratio_x100
                );
                println!(
                    "  branch_pred:   irregular branch={}ns vs select={}ns (prefer_branchless={})",
                    report.branch_predictor.irregular_branch_ns,
                    report.branch_predictor.irregular_select_ns,
                    report.branch_predictor.prefer_branchless_irregular
                );
                println!(
                    "  unroll_layout: unroll={}, native_cpu={}, soa={} (aos={}ns vs soa={}ns)",
                    report.unroll_layout.recommended_unroll.as_str(),
                    report.unroll_layout.prefer_native_cpu,
                    report.unroll_layout.prefer_soa,
                    report.unroll_layout.aos_ns,
                    report.unroll_layout.soa_ns
                );
                println!(
                    "  multicore:     workers={} scaling={}x/100 (recommended_workers={})",
                    report.multicore.tested_workers,
                    report.multicore.scaling_x100,
                    report.multicore.recommended_workers
                );
                println!(
                    "  frg_ring_gate: E4(RD={})={} C6(RD={})={} C8(RD={}, T_sync={}): t0={} tx={} r_e={} handoffs={} (op_certified={})",
                    report.frg_ring_gate.tier0_e4.rd_cadence,
                    report.frg_ring_gate.tier0_e4.verified,
                    report.frg_ring_gate.tier1_c6.rd_cadence,
                    report.frg_ring_gate.tier1_c6.verified,
                    report.frg_ring_gate.tier2_c8.rd_cadence,
                    report.frg_ring_gate.t_sync,
                    report.frg_ring_gate.t_zero,
                    report.frg_ring_gate.t_cross,
                    report.frg_ring_gate.r_e,
                    report.frg_ring_gate.inter_thread_handoffs,
                    report.frg_ring_gate.op_certified
                );
                println!("  total_ms:      {} ms", report.total_probe_ms);
                Ok(())
            })
            .map_err(CliErr::fail),
        "learn" => parse_options(sub_args, false, true, false, false, true, true, true, false)
            .and_then(|opts| {
                if opts.rest.len() != 1 {
                    return Err(format!("expected <source.dec>\n{}", usage()));
                }
                let src_path = PathBuf::from(&opts.rest[0]);
                if let Some(ref prof_out) = opts.profile {
                    ensure_out_differs_from_src(&src_path, Path::new(prof_out))?;
                }
                let (_hw, report) = learn_file_with_options(
                    &src_path,
                    opts.baseline_opt,
                    opts.pairs,
                    opts.heldout,
                    opts.fuel,
                    opts.max_depth,
                    opts.profile.as_deref().map(Path::new),
                    &opts.load,
                )
                .map_err(|e| e.to_string())?;
                let p = &report.profile;
                println!("aodc learn ({}) [{}]", src_path.display(), p.policy_scope);
                println!("  decision:              {}", p.decision.as_str());
                println!("  reason:                {}", p.reason);
                println!("  baseline_plan:         {}", p.baseline_plan.plan_id());
                println!("  candidate_plan:        {}", p.candidate_plan.plan_id());
                println!("  active_plan:           {}", p.active_plan.plan_id());
                println!(
                    "  heldout_ci_bps:        point={} [lower95={}, upper95={}] (n={})",
                    p.heldout_ci.point_gain_bps,
                    p.heldout_ci.lower_95_bps,
                    p.heldout_ci.upper_95_bps,
                    p.heldout_pairs.len()
                );
                println!(
                    "  honest_overhead_ns:    compile={} + dispatch={} = {} ns",
                    p.cand_compile_ns, p.dispatch_overhead_ns, p.measured_overhead_ns
                );
                println!(
                    "  heldout_sums_ns:       base={} vs cand={} (net_savings={} ns, break_even_calls={:?})",
                    p.heldout_base_sum_ns,
                    p.heldout_cand_sum_ns,
                    p.heldout_net_savings_ns,
                    p.break_even_calls
                );
                println!("  semantic_mismatches:   {}", p.semantic_mismatches);
                if let Some(ref prof_out) = opts.profile {
                    println!("  profile_saved:         {prof_out}");
                }
                Ok(())
            })
            .map_err(CliErr::fail),
        "bootstrap-verify" => {
            let mut source_path: Option<PathBuf> = None;
            let mut sample_path: Option<PathBuf> = None;
            let mut emit_stage2_path: Option<PathBuf> = None;
            let mut stdlib_dir: Option<PathBuf> = None;
            let mut json_mode = false;
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--source" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            source_path = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing file path after --source".to_string());
                            break;
                        }
                    },
                    "--sample" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            sample_path = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing file path after --sample".to_string());
                            break;
                        }
                    },
                    "--emit-stage2" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            emit_stage2_path = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing file path after --emit-stage2".to_string());
                            break;
                        }
                    },
                    "--stdlib" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            stdlib_dir = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing directory path after --stdlib".to_string());
                            break;
                        }
                    },
                    "--json" => {
                        json_mode = true;
                        idx += 1;
                    }
                    other => {
                        parse_err = Some(format!(
                            "unexpected argument {other:?} for bootstrap-verify\n{}",
                            usage()
                        ));
                        break;
                    }
                }
            }
            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else {
                match aodc::bootstrap_verify_with_stage2_out(
                    source_path.as_deref(),
                    sample_path.as_deref(),
                    emit_stage2_path.as_deref(),
                    &LoadOptions { stdlib_dir },
                ) {
                    Ok(rep) => {
                        if json_mode {
                            println!("{}", rep.to_json().to_compact_ascii_json());
                        } else {
                            println!(
                                "aodc bootstrap-verify ({}) -> PASS",
                                rep.self_host_path.display()
                            );
                            println!(
                                "  stage1 (Stage0 AOT): {} bytes (sha256={})",
                                rep.stage1_bytes, rep.stage1_sha256
                            );
                            println!(
                                "  stage2 (Self ELF64): {} bytes (sha256={})",
                                rep.stage2_bytes, rep.stage2_sha256
                            );
                            println!(
                                "  stage3 (Self ELF64): {} bytes (sha256={})",
                                rep.stage3_bytes, rep.stage3_sha256
                            );
                            println!("  fixed_point:         SHA256(stage2) == SHA256(stage3) (VERIFIED)");
                            println!("  sample_parity:       stage2 == VM == ORC-LLJIT (VERIFIED)");
                        }
                        Ok(())
                    }
                    Err(e) => Err(CliErr::fail(e.to_string())),
                }
            }
        }
        "manual-data" => {
            let mut check_only = false;
            let mut output_dir: Option<PathBuf> = None;
            let mut stdlib_dir: Option<PathBuf> = None;
            let mut json_mode = false;
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--check" => {
                        check_only = true;
                        idx += 1;
                    }
                    "--json" => {
                        json_mode = true;
                        idx += 1;
                    }
                    "--output-dir" => {
                        if output_dir.is_some() {
                            parse_err = Some("duplicate flag --output-dir".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                output_dir = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing directory path after --output-dir".to_string());
                                break;
                            }
                        }
                    }
                    "--stdlib" => {
                        if stdlib_dir.is_some() {
                            parse_err = Some("duplicate flag --stdlib".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                stdlib_dir = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing directory path after --stdlib".to_string());
                                break;
                            }
                        }
                    }
                    other => {
                        parse_err = Some(format!("unexpected argument {other:?}\n{}", usage()));
                        break;
                    }
                }
            }
            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else {
                match run_manual_data(&ManualDataOptions {
                    check_only,
                    output_dir,
                    stdlib_dir,
                }) {
                    Err(usage_or_guard_err) => Err(CliErr::usage(usage_or_guard_err)),
                    Ok(report) => {
                        if json_mode {
                            println!("{}", report.to_json().to_compact_ascii_json());
                        } else {
                            println!(
                                "aodc manual-data (mode={}) -> {} (checked={}, written={}, changed={})",
                                report.mode,
                                report.status,
                                report.files_checked,
                                report.files_written,
                                report.files_changed
                            );
                            for err in &report.errors {
                                eprintln!("  ERROR: {err}");
                            }
                        }
                        if report.is_ok() {
                            Ok(())
                        } else {
                            Err(CliErr::fail(format!(
                                "manual-data failed with {} error(s)",
                                report.errors.len()
                            )))
                        }
                    }
                }
            }
        }
        "manual-verify" => {
            let mut stdlib_dir: Option<PathBuf> = None;
            let mut registry_path: Option<PathBuf> = None;
            let mut json_mode = false;
            let mut positional: Vec<String> = Vec::new();
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--stdlib" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            stdlib_dir = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing directory path after --stdlib".to_string());
                            break;
                        }
                    },
                    "--registry" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            registry_path = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing file path after --registry".to_string());
                            break;
                        }
                    },
                    "--json" => {
                        json_mode = true;
                        idx += 1;
                    }
                    other => {
                        positional.push(other.to_string());
                        idx += 1;
                    }
                }
            }
            if let Some(e) = parse_err {
                Err(CliErr::fail(e))
            } else if positional.len() != 1 {
                Err(CliErr::fail(format!("expected <manual_dir>\n{}", usage())))
            } else {
                let report = verify_manual_dir(
                    Path::new(&positional[0]),
                    &ManualVerifyOptions {
                        stdlib_dir,
                        registry_path,
                    },
                );
                if json_mode {
                    println!("{}", report.to_json().to_compact_ascii_json());
                } else {
                    println!("aodc manual-verify ({}) -> {}", positional[0], report.status);
                    println!(
                        "  files={} main_refs={} labels={} internal_refs={} citations={}",
                        report.files_scanned,
                        report.main_refs_checked,
                        report.internal_labels,
                        report.internal_refs_checked,
                        report.citations_checked
                    );
                    println!(
                        "  inline_pos={} inline_neg={} dec_files={} data_files={}",
                        report.inline_positive_checked,
                        report.inline_negative_checked,
                        report.dec_files_checked,
                        report.data_files_checked
                    );
                    println!(
                        "  raster_violations={} aod_lint_violations={} errors={}",
                        report.raster_violations,
                        report.aod_lint_violations,
                        report.errors.len()
                    );
                    for err in &report.errors {
                        eprintln!("  ERROR: {err}");
                    }
                }
                if report.is_ok() {
                    Ok(())
                } else {
                    Err(CliErr::fail(format!(
                        "manual verification failed with {} error(s)",
                        report.errors.len()
                    )))
                }
            }
        }
        "verify-s2" => {
            let mut vectors_dir: Option<PathBuf> = None;
            let mut binding_path: Option<PathBuf> = None;
            let mut fixture_path: Option<PathBuf> = None;
            let mut report_path: Option<PathBuf> = None;
            let mut s4_report = false;
            let mut json_mode = false;
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--vectors" => {
                        if vectors_dir.is_some() {
                            parse_err = Some("duplicate flag --vectors".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                vectors_dir = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing directory path after --vectors".to_string());
                                break;
                            }
                        }
                    }
                    "--binding" => {
                        if binding_path.is_some() {
                            parse_err = Some("duplicate flag --binding".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                binding_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing file path after --binding".to_string());
                                break;
                            }
                        }
                    }
                    "--fixture" => {
                        if fixture_path.is_some() {
                            parse_err = Some("duplicate flag --fixture".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                fixture_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing file path after --fixture".to_string());
                                break;
                            }
                        }
                    }
                    "--report" => {
                        if report_path.is_some() {
                            parse_err = Some("duplicate flag --report".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                report_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing file path after --report".to_string());
                                break;
                            }
                        }
                    }
                    "--s4-report" => {
                        s4_report = true;
                        idx += 1;
                    }
                    "--json" => {
                        json_mode = true;
                        idx += 1;
                    }
                    other => {
                        parse_err = Some(format!("unexpected argument {other:?}\n{}", usage()));
                        break;
                    }
                }
            }
            if parse_err.is_none() && fixture_path.is_some() && !s4_report {
                parse_err = Some("--fixture requires --s4-report".to_string());
            }

            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else {
                let validated_report = if let Some(ref raw_rp) = report_path {
                    let mut explicit: Vec<&Path> = Vec::new();
                    if let Some(ref v) = vectors_dir {
                        explicit.push(v.as_path());
                    }
                    if let Some(ref b) = binding_path {
                        explicit.push(b.as_path());
                    }
                    if let Some(ref f) = fixture_path {
                        explicit.push(f.as_path());
                    }
                    let guarded = collect_guarded_roots(raw_rp, &explicit);
                    match validate_runtime_output_path(raw_rp, &guarded) {
                        Ok(p) => Some(p),
                        Err(err) => {
                            eprintln!("aodc: {err}");
                            return ExitCode::from(2);
                        }
                    }
                } else {
                    None
                };

                if s4_report {
                    match verify_s4_convergence_dir(
                        vectors_dir.as_deref(),
                        binding_path.as_deref(),
                        fixture_path.as_deref(),
                    ) {
                        Ok((_val, pretty, passed)) => {
                            if let Some(ref rp) = validated_report {
                                if let Err(e) = write_report_atomic(rp, pretty.as_bytes()) {
                                    Err(CliErr::fail(e))
                                } else if passed {
                                    Ok(())
                                } else {
                                    Err(CliErr::fail(
                                        "S4 convergence verification failed".to_string(),
                                    ))
                                }
                            } else {
                                print!("{pretty}");
                                if passed {
                                    Ok(())
                                } else {
                                    Err(CliErr::fail(
                                        "S4 convergence verification failed".to_string(),
                                    ))
                                }
                            }
                        }
                        Err(e) => Err(CliErr::fail(e)),
                    }
                } else {
                    let report = verify_s2_dir(vectors_dir.as_deref(), binding_path.as_deref());
                    if let Some(ref rp) = validated_report {
                        let pretty = report.to_json().to_pretty_utf8_json();
                        if let Err(e) = write_report_atomic(rp, pretty.as_bytes()) {
                            Err(CliErr::fail(e))
                        } else if report.is_ok() {
                            Ok(())
                        } else {
                            Err(CliErr::fail(format!(
                                "S2 verification failed with {} failure(s)",
                                report.failures.len()
                            )))
                        }
                    } else {
                        if json_mode {
                            println!("{}", report.to_json().to_compact_ascii_json());
                        } else {
                            println!(
                                "aodc verify-s2 ({}, binding_status={}) -> {}",
                                report.binding_id, report.binding_status, report.status
                            );
                            println!(
                                "  vectors: {}/{} passed (rational_core={}, symbolic_and_display={}, failed={})",
                                report.passed_vectors,
                                report.total_vectors,
                                report.rational_core_count,
                                report.symbolic_and_display_count,
                                report.failed_vectors
                            );
                            let mut status_parts = Vec::new();
                            for (k, v) in &report.scientific_status_summary {
                                status_parts.push(format!("{k}={v}"));
                            }
                            println!("  scientific_status: {}", status_parts.join(", "));
                            for f in &report.failures {
                                eprintln!("  FAIL {}: {}", f.case_id, f.error);
                            }
                        }
                        if report.is_ok() {
                            Ok(())
                        } else {
                            Err(CliErr::fail(format!(
                                "S2 verification failed with {} failure(s)",
                                report.failures.len()
                            )))
                        }
                    }
                }
            }
        }
        "eval-op" => {
            let mut input_json: Option<String> = None;
            let mut input_file: Option<PathBuf> = None;
            let mut input_stdin = false;
            let mut report_path: Option<PathBuf> = None;
            let mut positional: Vec<String> = Vec::new();
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;

            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--input-json" => {
                        if input_json.is_some() {
                            parse_err = Some("duplicate flag --input-json".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                input_json = Some(v.clone());
                                idx += 2;
                            }
                            None => {
                                parse_err = Some("missing JSON value after --input-json".to_string());
                                break;
                            }
                        }
                    }
                    "--input" => {
                        if input_file.is_some() {
                            parse_err = Some("duplicate flag --input".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                input_file = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err = Some("missing file path after --input".to_string());
                                break;
                            }
                        }
                    }
                    "--input-stdin" => {
                        if input_stdin {
                            parse_err = Some("duplicate flag --input-stdin".to_string());
                            break;
                        }
                        input_stdin = true;
                        idx += 1;
                    }
                    "--report" => {
                        if report_path.is_some() {
                            parse_err = Some("duplicate flag --report".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                report_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err = Some("missing file path after --report".to_string());
                                break;
                            }
                        }
                    }
                    other if other.starts_with('-') => {
                        parse_err = Some(format!("unknown flag {other:?} for eval-op\n{}", usage()));
                        break;
                    }
                    other => {
                        positional.push(other.to_string());
                        idx += 1;
                    }
                }
            }

            let input_sources = usize::from(input_json.is_some())
                + usize::from(input_file.is_some())
                + usize::from(input_stdin);

            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else if positional.len() != 1 {
                Err(CliErr::usage(format!(
                    "eval-op expects exactly one <op> name\n{}",
                    usage()
                )))
            } else if input_sources != 1 {
                Err(CliErr::usage(
                    "eval-op requires exactly one of --input-json <json>, --input <file>, or --input-stdin"
                        .to_string(),
                ))
            } else {
                let validated_report = if let Some(ref raw_rp) = report_path {
                    let mut explicit: Vec<&Path> = Vec::new();
                    if let Some(ref inp) = input_file {
                        explicit.push(inp.as_path());
                    }
                    let guarded = collect_guarded_roots(raw_rp, &explicit);
                    match validate_runtime_output_path(raw_rp, &guarded) {
                        Ok(p) => Some(p),
                        Err(err) => {
                            eprintln!("aodc: {err}");
                            return ExitCode::from(2);
                        }
                    }
                } else {
                    None
                };

                let raw_json_res: Result<String, String> = if let Some(s) = input_json {
                    if s.len() > MAX_S4_INPUT_BYTES {
                        Err(format!(
                            "--input-json exceeds maximum size limit of {MAX_S4_INPUT_BYTES} bytes"
                        ))
                    } else {
                        Ok(s)
                    }
                } else if let Some(ref path) = input_file {
                    read_bounded_regular_file(path, "input", MAX_S4_INPUT_BYTES).and_then(|bytes| {
                        String::from_utf8(bytes)
                            .map_err(|_| "input: file is not valid UTF-8".to_string())
                    })
                } else {
                    read_bounded_stdin(MAX_S4_INPUT_BYTES).and_then(|bytes| {
                        String::from_utf8(bytes)
                            .map_err(|_| "stdin: input is not valid UTF-8".to_string())
                    })
                };

                let eval_res = raw_json_res
                    .and_then(|raw| parse_strict_json(&raw).map_err(|e| e.to_string()))
                    .and_then(|val| match val {
                        JsonValue::Object(m) => Ok(m),
                        _ => Err("eval-op input must be a JSON object".to_string()),
                    })
                    .and_then(|inputs| {
                        eval_operation(&positional[0], &inputs).map_err(|e| e.to_string())
                    });

                match eval_res {
                    Ok(result) => {
                        let pretty = result.to_pretty_utf8_json();
                        if let Some(ref rp) = validated_report {
                            write_report_atomic(rp, pretty.as_bytes()).map_err(CliErr::fail)
                        } else {
                            print!("{pretty}");
                            Ok(())
                        }
                    }
                    Err(e) => Err(CliErr::fail(e)),
                }
            }
        }
        "audit-pdf" => {
            let mut min_pages: usize = 1;
            let mut compare_pdf: Option<PathBuf> = None;
            let mut json_mode = false;
            let mut positional: Vec<String> = Vec::new();
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--min-pages" => {
                        match parse_positive_usize("--min-pages", sub_args.get(idx + 1)) {
                            Ok(n) => {
                                min_pages = n;
                                idx += 2;
                            }
                            Err(e) => {
                                parse_err = Some(e);
                                break;
                            }
                        }
                    }
                    "--compare-pdf" => match sub_args.get(idx + 1) {
                        Some(v) => {
                            compare_pdf = Some(PathBuf::from(v));
                            idx += 2;
                        }
                        None => {
                            parse_err = Some("missing file path after --compare-pdf".to_string());
                            break;
                        }
                    },
                    "--json" => {
                        json_mode = true;
                        idx += 1;
                    }
                    other if other.starts_with('-') => {
                        parse_err = Some(format!("unknown flag {other:?} for audit-pdf\n{}", usage()));
                        break;
                    }
                    other => {
                        positional.push(other.to_string());
                        idx += 1;
                    }
                }
            }
            if let Some(e) = parse_err {
                Err(CliErr::fail(e))
            } else if positional.len() != 1 {
                Err(CliErr::fail(format!("expected <pdf>\n{}", usage())))
            } else {
                let report = audit_pdf_file(
                    Path::new(&positional[0]),
                    &PdfAuditOptions {
                        min_pages,
                        compare_pdf,
                    },
                );
                if json_mode {
                    println!("{}", report.to_json().to_compact_ascii_json());
                } else {
                    println!(
                        "aodc audit-pdf ({}) -> {} (bytes={}, pages={}, streams={}, flate={}, rasters={}, unresolved={}, sha256={})",
                        positional[0],
                        report.status,
                        report.bytes,
                        report.pages,
                        report.streams_total,
                        report.streams_flate_inflated,
                        report.raster_violations,
                        report.unresolved_violations,
                        report.sha256
                    );
                    if let Some(ref cmp) = report.compare_sha256 {
                        println!(
                            "  compare_sha256={} deterministic_match={}",
                            cmp,
                            report.deterministic_match.unwrap_or(false)
                        );
                    }
                    for err in &report.errors {
                        eprintln!("  ERROR: {err}");
                    }
                }
                if report.is_ok() {
                    Ok(())
                } else {
                    Err(CliErr::fail(format!(
                        "PDF audit failed with {} error(s)",
                        report.errors.len()
                    )))
                }
            }
        }
        "fmt" => {
            let mut check_only = false;
            let mut write_in_place = false;
            let mut out_path: Option<PathBuf> = None;
            let mut positional: Vec<String> = Vec::new();
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--compat" => {
                        parse_err = Some(
                            "legacy --compat flag has been removed in clean-break Dec".to_string(),
                        );
                        break;
                    }
                    "--strict-wave" | "--pure-wave" => {
                        idx += 1;
                    }
                    "--check" => {
                        check_only = true;
                        idx += 1;
                    }
                    "--write" => {
                        write_in_place = true;
                        idx += 1;
                    }
                    "-o" | "--output" => {
                        if out_path.is_some() {
                            parse_err = Some("duplicate flag -o/--output".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                out_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err = Some("missing path after -o/--output".to_string());
                                break;
                            }
                        }
                    }
                    other if other.starts_with('-') => {
                        parse_err = Some(format!("unknown flag {other:?} for fmt\n{}", usage()));
                        break;
                    }
                    other => {
                        positional.push(other.to_string());
                        idx += 1;
                    }
                }
            }
            let mode_count = usize::from(check_only)
                + usize::from(write_in_place)
                + usize::from(out_path.is_some());
            if parse_err.is_none() && mode_count > 1 {
                parse_err = Some(
                    "--check, --write, and -o/--output are mutually exclusive".to_string(),
                );
            }
            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else if positional.len() != 1 {
                Err(CliErr::usage(format!(
                    "expected exactly one <source.dec>\n{}",
                    usage()
                )))
            } else {
                let src_path = Path::new(&positional[0]);
                // Pre-execution symlink checks on `--write` and `-o` (exit 2 without touching target!).
                if write_in_place {
                    match std::fs::symlink_metadata(src_path) {
                        Ok(meta) if meta.file_type().is_symlink() => {
                            eprintln!("aodc: fmt --write target must not be a symlink");
                            return ExitCode::from(2);
                        }
                        _ => {}
                    }
                }
                if let Some(ref op) = out_path {
                    if let Ok(meta) = std::fs::symlink_metadata(op) {
                        if meta.file_type().is_symlink() {
                            eprintln!("aodc: fmt -o/--output target must not be a symlink");
                            return ExitCode::from(2);
                        }
                        if meta.is_dir() {
                            eprintln!("aodc: fmt -o/--output target is a directory");
                            return ExitCode::from(2);
                        }
                    }
                    if let Err(e) = ensure_out_differs_from_src(src_path, op) {
                        eprintln!("aodc: {e}");
                        return ExitCode::from(2);
                    }
                }

                let raw_bytes = match read_bounded_regular_file(
                    src_path,
                    "source",
                    aodc::parser::MAX_SOURCE_BYTES,
                ) {
                    Ok(b) => b,
                    Err(e) if e.contains("symlink rejected") => {
                        eprintln!("aodc: {e}");
                        return ExitCode::from(2);
                    }
                    Err(e) => {
                        eprintln!("aodc: {e}");
                        return ExitCode::from(1);
                    }
                };
                let src_res = String::from_utf8(raw_bytes)
                    .map_err(|_| CliErr::fail("source: file is not valid UTF-8".to_string()));
                src_res.and_then(|src| {
                    let formatted =
                        format_source(&src).map_err(|e| CliErr::fail(e.to_string()))?;
                    if check_only {
                        if src == formatted {
                            Ok(())
                        } else {
                            Err(CliErr::fail(format!(
                                "{} is not canonically formatted",
                                src_path.display()
                            )))
                        }
                    } else if let Some(ref op) = out_path {
                        write_report_atomic(op, formatted.as_bytes()).map_err(CliErr::fail)
                    } else if write_in_place {
                        write_report_atomic(src_path, formatted.as_bytes()).map_err(CliErr::fail)
                    } else {
                        print!("{formatted}");
                        Ok(())
                    }
                })
            }
        }
        "migrate" => {
            let mut check_only = false;
            let mut write_in_place = false;
            let mut out_path: Option<PathBuf> = None;
            let mut positional: Vec<String> = Vec::new();
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--check" => {
                        check_only = true;
                        idx += 1;
                    }
                    "--write" | "-w" => {
                        write_in_place = true;
                        idx += 1;
                    }
                    "-o" | "--output" => {
                        if out_path.is_some() {
                            parse_err = Some("duplicate flag -o/--output".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                out_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err = Some("missing path after -o/--output".to_string());
                                break;
                            }
                        }
                    }
                    other if other.starts_with('-') => {
                        parse_err =
                            Some(format!("unknown flag {other:?} for migrate\n{}", usage()));
                        break;
                    }
                    other => {
                        positional.push(other.to_string());
                        idx += 1;
                    }
                }
            }
            let mode_count = usize::from(check_only)
                + usize::from(write_in_place)
                + usize::from(out_path.is_some());
            if parse_err.is_none() && mode_count > 1 {
                parse_err = Some(
                    "--check, --write, and -o/--output are mutually exclusive".to_string(),
                );
            }
            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else if positional.len() != 1 {
                Err(CliErr::usage(format!(
                    "expected exactly one <source.dec>\n{}",
                    usage()
                )))
            } else {
                let src_path = Path::new(&positional[0]);
                if write_in_place {
                    if let Ok(meta) = std::fs::symlink_metadata(src_path) {
                        if meta.file_type().is_symlink() {
                            eprintln!("aodc: migrate --write target must not be a symlink");
                            return ExitCode::from(2);
                        }
                    }
                    if let Err(e) = ensure_not_frozen_guarded_target(src_path) {
                        eprintln!("aodc: {e}");
                        return ExitCode::from(2);
                    }
                }
                if let Some(ref op) = out_path {
                    if let Ok(meta) = std::fs::symlink_metadata(op) {
                        if meta.file_type().is_symlink() {
                            eprintln!("aodc: migrate -o/--output target must not be a symlink");
                            return ExitCode::from(2);
                        }
                        if meta.is_dir() {
                            eprintln!("aodc: migrate -o/--output target is a directory");
                            return ExitCode::from(2);
                        }
                    }
                    if let Err(e) = ensure_out_differs_from_src(src_path, op) {
                        eprintln!("aodc: {e}");
                        return ExitCode::from(2);
                    }
                    if let Err(e) = ensure_not_frozen_guarded_target(op) {
                        eprintln!("aodc: {e}");
                        return ExitCode::from(2);
                    }
                }

                let raw_bytes =
                    match read_bounded_regular_file(src_path, "source", MAX_MIGRATE_SOURCE_BYTES) {
                        Ok(b) => b,
                        Err(e) if e.contains("symlink rejected") => {
                            eprintln!("aodc: {e}");
                            return ExitCode::from(2);
                        }
                        Err(e) => {
                            eprintln!("aodc: {e}");
                            return ExitCode::from(1);
                        }
                    };
                let src_res = String::from_utf8(raw_bytes)
                    .map_err(|_| CliErr::fail("source: file is not valid UTF-8".to_string()));
                src_res.and_then(|src| {
                    let migrated = migrate_source_formatted(&src)
                        .map_err(|e| CliErr::fail(e.to_string()))?;
                    if check_only {
                        if src == migrated {
                            Ok(())
                        } else {
                            Err(CliErr::fail(format!(
                                "{} requires v3 migration",
                                src_path.display()
                            )))
                        }
                    } else if let Some(ref op) = out_path {
                        write_report_atomic(op, migrated.as_bytes()).map_err(CliErr::fail)
                    } else if write_in_place {
                        write_report_atomic(src_path, migrated.as_bytes()).map_err(CliErr::fail)
                    } else {
                        print!("{migrated}");
                        Ok(())
                    }
                })
            }
        }
        "audit-dec" | "audit" => {
            let mut stdlib_dir: Option<PathBuf> = None;
            let mut bootstrap_ns: Option<PathBuf> = None;
            let mut report_path: Option<PathBuf> = None;
            let mut json_mode = false;
            let mut positional: Vec<String> = Vec::new();
            let mut idx = 0usize;
            let mut parse_err: Option<String> = None;
            while idx < sub_args.len() {
                match sub_args[idx].as_str() {
                    "--compat" => {
                        parse_err = Some(
                            "--compat is not supported on audit-dec (pure-wave v3 only)".to_string(),
                        );
                        break;
                    }
                    "--strict-wave" | "--pure-wave" => {
                        idx += 1;
                    }
                    "--stdlib" => {
                        if stdlib_dir.is_some() {
                            parse_err = Some("duplicate flag --stdlib".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                stdlib_dir = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing directory path after --stdlib".to_string());
                                break;
                            }
                        }
                    }
                    "--namespace" => {
                        if bootstrap_ns.is_some() {
                            parse_err = Some("duplicate flag --namespace".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                bootstrap_ns = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err =
                                    Some("missing file path after --namespace".to_string());
                                break;
                            }
                        }
                    }
                    "--report" => {
                        if report_path.is_some() {
                            parse_err = Some("duplicate flag --report".to_string());
                            break;
                        }
                        match sub_args.get(idx + 1) {
                            Some(v) => {
                                report_path = Some(PathBuf::from(v));
                                idx += 2;
                            }
                            None => {
                                parse_err = Some("missing file path after --report".to_string());
                                break;
                            }
                        }
                    }
                    "--json" => {
                        json_mode = true;
                        idx += 1;
                    }
                    other if other.starts_with('-') => {
                        parse_err =
                            Some(format!("unknown flag {other:?} for audit\n{}", usage()));
                        break;
                    }
                    other => {
                        positional.push(other.to_string());
                        idx += 1;
                    }
                }
            }
            if let Some(e) = parse_err {
                Err(CliErr::usage(e))
            } else if positional.len() != 1 {
                Err(CliErr::usage(format!(
                    "expected exactly one <source.dec>\n{}",
                    usage()
                )))
            } else {
                let src_path = Path::new(&positional[0]);
                // Pre-execution `--report` validation BEFORE reading `<source.dec>`!
                let validated_report = if let Some(ref raw_rp) = report_path {
                    let mut explicit: Vec<&Path> = vec![src_path];
                    if let Some(ref sd) = stdlib_dir {
                        explicit.push(sd.as_path());
                    }
                    if let Some(ref ns) = bootstrap_ns {
                        explicit.push(ns.as_path());
                    }
                    let guarded = collect_guarded_roots(raw_rp, &explicit);
                    match validate_runtime_output_path(raw_rp, &guarded) {
                        Ok(p) => Some(p),
                        Err(err) => {
                            eprintln!("aodc: {err}");
                            return ExitCode::from(2);
                        }
                    }
                } else {
                    None
                };

                let load_opts = LoadOptions { stdlib_dir };
                let report = audit_dec_file_with_ns_and_options(
                    src_path,
                    bootstrap_ns.as_deref(),
                    &load_opts,
                );
                let json_val = report.to_json();
                if let Some(ref rp) = validated_report {
                    let pretty = json_val.to_pretty_utf8_json();
                    if let Err(err) = write_report_atomic(rp, pretty.as_bytes()) {
                        eprintln!("aodc: {err}");
                        return ExitCode::from(1);
                    }
                }
                if json_mode {
                    println!("{}", json_val.to_compact_ascii_json());
                } else {
                    println!(
                        "aodc audit-dec ({}) -> {} ({}/6 hooks, anchor={} -> {}, crossings={}, dwell={})",
                        report.source_name,
                        report.status,
                        report.hooks_passed(),
                        report.live_walk.initial_anchor.render(),
                        report.live_walk.final_anchor.render(),
                        report.live_walk.committed_crossings,
                        report.live_walk.dwell_burned
                    );
                    for err in &report.errors {
                        eprintln!("  ERROR: {err}");
                    }
                }
                if report.passed() {
                    Ok(())
                } else {
                    Err(CliErr::fail(format!(
                        "audit-dec failed ({}/6 hooks passed)",
                        report.hooks_passed()
                    )))
                }
            }
        }
        "test" => extract_compat_flag(sub_args)
            .and_then(|(compat, rem)| {
                let opts =
                    parse_options(&rem, false, false, false, false, true, false, false, false)?;
                if opts.rest.len() != 1 {
                    return Err(format!("expected <file_or_dir>\n{}", usage()));
                }
                let cases = run_dec_tests_with_compat(Path::new(&opts.rest[0]), &opts.load, compat)
                    .map_err(|e| e.to_string())?;
                if cases.is_empty() {
                    return Err("no Dec test cases discovered".to_string());
                }
                let mut failed = 0usize;
                for c in &cases {
                    if c.passed {
                        println!("PASS {} :: {}", c.file.display(), c.test_name);
                    } else {
                        failed += 1;
                        eprintln!(
                            "FAIL {} :: {} -> {}",
                            c.file.display(),
                            c.test_name,
                            c.error.as_deref().unwrap_or("unknown failure")
                        );
                    }
                }
                println!(
                    "test result: {} passed; {} failed; {} total",
                    cases.len() - failed,
                    failed,
                    cases.len()
                );
                if failed > 0 {
                    Err(format!("{failed} test(s) failed"))
                } else {
                    Ok(())
                }
            })
            .map_err(CliErr::fail),
        other => Err(CliErr::fail(format!(
            "unknown subcommand {other:?}\n{}",
            usage()
        ))),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aodc: {}", err.msg);
            ExitCode::from(err.code)
        }
    }
}
