#![forbid(unsafe_code)]

//! `aodc`: Dec v2 frontend, compile-time firewalls, hermetic `DecIR` v1/v2 serializer &
//! verifier, declared deterministic sandbox chamber simulator, and exact reference evaluator.

pub mod ast;
pub mod checker;
pub mod diagnostics;
pub mod eval;
pub mod ir;
pub mod lexer;
pub mod modules;
pub mod parser;
pub mod sim;
pub mod types;

use std::path::{Path, PathBuf};

use crate::checker::{check_unit, CheckedUnit};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::eval::{EvalOutput, Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use crate::ir::{dumps_ir, loads_ir};
use crate::modules::load_from_source;

pub fn check_source(source: &str, compat: CompatMode) -> DecResult<CheckedUnit> {
    let loaded = load_from_source(source, None, compat)?;
    check_unit(loaded, true)
}

pub fn check_file(path: &Path, compat: CompatMode) -> DecResult<CheckedUnit> {
    let source = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("failed reading {}: {e}", path.display()),
        )
    })?;
    let base_dir = path.parent();
    let loaded = load_from_source(&source, base_dir, compat)?;
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
    let checked = check_file(path, compat)?;
    Ok(Evaluator::new(&checked, fuel, max_depth)?.run_main_capturing())
}

pub fn compile_file_to_ir(path: &Path, compat: CompatMode) -> DecResult<String> {
    let checked = check_file(path, compat)?;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestCaseResult {
    pub file: PathBuf,
    pub test_name: String,
    pub passed: bool,
    pub error: Option<String>,
}

/// Discover and execute Dec tests on a file or recursively across a directory (`P0-3`).
///
/// For each `.dec` file:
/// - Typecheck the file (`require_main = false` when `test_*` functions exist).
/// - Run all zero-arg `Dec test_*()` functions, or `main()` if none exist.
/// - When running `main()` and a sibling `<stem>.stdout` exists, verify exact byte-for-byte parity.
pub fn run_dec_tests(target: &Path, compat: CompatMode) -> DecResult<Vec<TestCaseResult>> {
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
        let loaded = match load_from_source(&source, file.parent(), compat) {
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
                    let sibling_stdout = file.with_extension("stdout");
                    if sibling_stdout.is_file() {
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
