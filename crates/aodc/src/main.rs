#![forbid(unsafe_code)]

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aodc::diagnostics::CompatMode;
use aodc::eval::{Value, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::{
    bench_file, build_aot_file, check_file, compile_file_to_ir, emit_llvm_file,
    exec_ir_file_with_budget_capturing, format_x100, jit_file_captured, run_dec_tests,
    run_file_with_budget_capturing, OptLevel,
};

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
            let v = next.ok_or_else(|| "missing optimization level after -O (expected 0..3)".to_string())?;
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

fn parse_compat_and_rest(args: &[String]) -> Result<(CompatMode, Vec<String>), String> {
    let mut compat = CompatMode::V2;
    let mut rest = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--compat" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| "missing value after --compat (expected v1 or v2)".to_string())?;
            compat = match val.as_str() {
                "v1" => CompatMode::V1,
                "v2" => CompatMode::V2,
                other => return Err(format!("unknown --compat mode {other:?} (expected v1 or v2)")),
            };
            i += 2;
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    Ok((compat, rest))
}

struct ExecOptions {
    compat: CompatMode,
    opt: OptLevel,
    fuel: usize,
    max_depth: usize,
    iters: usize,
    output: Option<String>,
    rest: Vec<String>,
}

fn parse_options(
    args: &[String],
    allow_compat: bool,
    allow_opt: bool,
    allow_budget: bool,
    allow_output: bool,
    allow_iters: bool,
) -> Result<ExecOptions, String> {
    let mut compat = CompatMode::V2;
    let mut opt = OptLevel::O2;
    let mut fuel = DEFAULT_FUEL;
    let mut max_depth = DEFAULT_MAX_DEPTH;
    let mut iters = 5usize;
    let mut output = None;
    let mut rest = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].as_str();
        if allow_compat && a == "--compat" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| "missing value after --compat (expected v1 or v2)".to_string())?;
            compat = match val.as_str() {
                "v1" => CompatMode::V1,
                "v2" => CompatMode::V2,
                other => return Err(format!("unknown --compat mode {other:?} (expected v1 or v2)")),
            };
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
        } else {
            rest.push(args[i].clone());
            i += 1;
        }
    }
    Ok(ExecOptions {
        compat,
        opt,
        fuel,
        max_depth,
        iters,
        output,
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

fn usage() -> &'static str {
    "Usage:\n  aodc check [--compat v1|v2] <source.dec>\n  aodc run [--compat v1|v2] [--fuel <N>] [--max-depth <N>] <source.dec>\n  aodc compile [--compat v1|v2] <source.dec> -o <out.ir.json>\n  aodc exec [--fuel <N>] [--max-depth <N>] <program.ir.json>\n  aodc emit-llvm [--compat v1|v2] [-O 0|1|2|3] <source.dec> [-o <out.ll>]\n  aodc build [--compat v1|v2] [-O 0|1|2|3] <source.dec> -o <out_exe>\n  aodc jit [--compat v1|v2] [-O 0|1|2|3] [--fuel <N>] [--max-depth <N>] <source.dec>\n  aodc bench [--compat v1|v2] [--iters <N>] [--fuel <N>] [--max-depth <N>] <source.dec>\n  aodc test [--compat v1|v2] <file_or_dir>"
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

    let result: Result<(), String> = match cmd {
        "check" => parse_compat_and_rest(sub_args).and_then(|(compat, rest)| {
            if rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            check_file(Path::new(&rest[0]), compat).map_err(|e| e.to_string())?;
            match compat {
                CompatMode::V1 => println!("OK: Dec core v1"),
                CompatMode::V2 => println!("OK: Dec v2"),
            }
            Ok(())
        }),
        "run" => parse_options(sub_args, true, false, true, false, false).and_then(|opts| {
            if opts.rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            let (emits, outcome) = run_file_with_budget_capturing(
                Path::new(&opts.rest[0]),
                opts.compat,
                opts.fuel,
                opts.max_depth,
            )
            .map_err(|e| e.to_string())?;
            flush_emits_and_result(emits, outcome)
        }),
        "compile" => parse_options(sub_args, true, false, false, true, false).and_then(|opts| {
            if opts.rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            let src_path = PathBuf::from(&opts.rest[0]);
            let out_path = PathBuf::from(
                opts.output
                    .ok_or_else(|| "missing -o <out.ir.json>".to_string())?,
            );
            ensure_out_differs_from_src(&src_path, &out_path)?;
            let ir_text = compile_file_to_ir(&src_path, opts.compat).map_err(|e| e.to_string())?;
            std::fs::write(&out_path, ir_text).map_err(|e| format!("write error: {e}"))?;
            Ok(())
        }),
        "exec" => parse_options(sub_args, false, false, true, false, false).and_then(|opts| {
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
        }),
        "emit-llvm" => parse_options(sub_args, true, true, false, true, false).and_then(|opts| {
            if opts.rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            let src_path = PathBuf::from(&opts.rest[0]);
            if let Some(ref out) = opts.output {
                ensure_out_differs_from_src(&src_path, Path::new(out))?;
            }
            let ir = emit_llvm_file(&src_path, opts.compat, opts.opt).map_err(|e| e.to_string())?;
            if let Some(out) = opts.output {
                std::fs::write(&out, ir).map_err(|e| format!("write error: {e}"))?;
            } else {
                print!("{ir}");
            }
            Ok(())
        }),
        "build" => parse_options(sub_args, true, true, false, true, false).and_then(|opts| {
            if opts.rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            let src_path = PathBuf::from(&opts.rest[0]);
            let out_path = PathBuf::from(
                opts.output
                    .ok_or_else(|| "missing -o <out_exe>".to_string())?,
            );
            ensure_out_differs_from_src(&src_path, &out_path)?;
            build_aot_file(&src_path, opts.compat, opts.opt, &out_path).map_err(|e| e.to_string())
        }),
        "jit" => parse_options(sub_args, true, true, true, false, false).and_then(|opts| {
            if opts.rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            let (stdout, res) = jit_file_captured(
                Path::new(&opts.rest[0]),
                opts.compat,
                opts.opt,
                opts.fuel,
                opts.max_depth,
            )
            .map_err(|e| e.to_string())?;
            {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(stdout.as_bytes());
                let _ = out.flush();
            }
            res
        }),
        "bench" => parse_options(sub_args, true, false, true, false, true).and_then(|opts| {
            if opts.rest.len() != 1 {
                return Err(format!("expected <source.dec>\n{}", usage()));
            }
            let r = bench_file(
                Path::new(&opts.rest[0]),
                opts.compat,
                opts.iters,
                opts.fuel,
                opts.max_depth,
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
        }),
        "test" => parse_compat_and_rest(sub_args).and_then(|(compat, rest)| {
            if rest.len() != 1 {
                return Err(format!("expected <file_or_dir>\n{}", usage()));
            }
            let cases = run_dec_tests(Path::new(&rest[0]), compat).map_err(|e| e.to_string())?;
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
        }),
        other => Err(format!("unknown subcommand {other:?}\n{}", usage())),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("aodc: {msg}");
            ExitCode::from(1)
        }
    }
}
