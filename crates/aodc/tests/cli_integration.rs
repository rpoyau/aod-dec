use std::path::PathBuf;
use std::process::Command;

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
    let d = std::env::temp_dir().join(format!("aodc_cli_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn cli_check_run_compile_exec_and_self_overwrite_refusal() {
    let root = repo_root();
    let closure_dec = root.join("tests/fixtures/v1_examples/closure.dec");
    let closure_stdout =
        std::fs::read_to_string(root.join("tests/fixtures/v1_examples/closure.stdout")).unwrap();

    // `aodc check --compat v1`
    let chk = Command::new(aodc_bin())
        .args(["check", "--compat", "v1"])
        .arg(&closure_dec)
        .output()
        .unwrap();
    assert!(chk.status.success());
    assert_eq!(String::from_utf8_lossy(&chk.stdout).trim(), "OK: Dec core v1");

    // `aodc run --compat v1`
    let run = Command::new(aodc_bin())
        .args(["run", "--compat", "v1"])
        .arg(&closure_dec)
        .output()
        .unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), closure_stdout);

    // `aodc compile src -o src` refused via canonical path comparison
    let self_out = Command::new(aodc_bin())
        .args(["compile", "--compat", "v1"])
        .arg(&closure_dec)
        .arg("-o")
        .arg(&closure_dec)
        .output()
        .unwrap();
    assert!(!self_out.status.success());
    assert!(String::from_utf8_lossy(&self_out.stderr).contains("differ"));

    // `aodc compile` -> `aodc exec`, then tamper IR and verify strict rejection
    let dir = temp_dir("ir_roundtrip");
    let ir_path = dir.join("closure.ir.json");
    let comp = Command::new(aodc_bin())
        .args(["compile", "--compat", "v1"])
        .arg(&closure_dec)
        .arg("-o")
        .arg(&ir_path)
        .output()
        .unwrap();
    assert!(comp.status.success());

    let exec_ok = Command::new(aodc_bin())
        .arg("exec")
        .arg(&ir_path)
        .output()
        .unwrap();
    assert!(exec_ok.status.success());
    assert_eq!(String::from_utf8_lossy(&exec_ok.stdout), closure_stdout);

    // Tamper with IR JSON -> must reject!
    let orig_ir = std::fs::read_to_string(&ir_path).unwrap();
    let tampered = orig_ir.replacen("\"version\": 1", "\"version\": 1, \"extra\": 99", 1);
    let bad_ir_path = dir.join("tampered.ir.json");
    std::fs::write(&bad_ir_path, tampered).unwrap();
    let exec_bad = Command::new(aodc_bin())
        .arg("exec")
        .arg(&bad_ir_path)
        .output()
        .unwrap();
    assert!(!exec_bad.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac21_hermetic_multifile_dec_ir_v2_executes_after_deleting_source_directory() {
    let base = temp_dir("hermetic_v2");
    let src_dir = base.join("project_src");
    std::fs::create_dir_all(&src_dir).unwrap();

    std::fs::write(
        src_dir.join("rates.dec"),
        "const BONUS = 2/3r\n\nDec scaled(x: ratio) -> ratio:\n    return x + BONUS\n",
    )
    .unwrap();
    let entry_file = src_dir.join("entry.dec");
    std::fs::write(
        &entry_file,
        "use rates::{scaled}\n\nDec main() -> ratio:\n    let v = scaled(1/3r)\n    emit v\n    return v\n",
    )
    .unwrap();

    let ir_file = base.join("bundled.ir.json");
    let comp = Command::new(aodc_bin())
        .arg("compile")
        .arg(&entry_file)
        .arg("-o")
        .arg(&ir_file)
        .output()
        .unwrap();
    assert!(
        comp.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&comp.stderr)
    );

    // DELETE the entire original source directory before executing IR!
    std::fs::remove_dir_all(&src_dir).unwrap();
    assert!(!src_dir.exists());

    // `aodc exec` MUST succeed strictly from embedded hermetic bundle (zero ambient FS reads)
    let exec = Command::new(aodc_bin())
        .arg("exec")
        .arg(&ir_file)
        .output()
        .unwrap();
    assert!(
        exec.status.success(),
        "hermetic exec failed after source deletion: {}",
        String::from_utf8_lossy(&exec.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&exec.stdout), "1/1\n1/1\n");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cli_test_subcommand_handles_test_fns_directories_unit_return_and_failures() {
    let root = repo_root();

    // 1. Directory of v1 examples with sibling .stdout parity checks
    let v1_test = Command::new(aodc_bin())
        .args(["test", "--compat", "v1"])
        .arg(root.join("tests/fixtures/v1_examples"))
        .output()
        .unwrap();
    assert!(v1_test.status.success());
    assert!(String::from_utf8_lossy(&v1_test.stdout).contains("10 passed; 0 failed; 10 total"));

    // 2. Directory of v2 smoke fixtures
    let v2_test = Command::new(aodc_bin())
        .arg("test")
        .arg(root.join("tests/fixtures/v2_smoke"))
        .output()
        .unwrap();
    assert!(v2_test.status.success());
    assert!(String::from_utf8_lossy(&v2_test.stdout).contains("3 passed; 0 failed; 3 total"));

    // 3. Single file with multiple zero-arg `Dec test_*()` functions + unit `main()` omission
    let dir = temp_dir("unit_and_tests");
    let test_file = dir.join("my_tests.dec");
    std::fs::write(
        &test_file,
        r#"
Dec test_ratio():
    assert 1/2r + 1/2r == 1/1r

Dec test_anchor():
    assert 00_8 != 0_8
"#,
    )
    .unwrap();
    let t_out = Command::new(aodc_bin())
        .arg("test")
        .arg(&test_file)
        .output()
        .unwrap();
    assert!(t_out.status.success());
    assert!(String::from_utf8_lossy(&t_out.stdout).contains("2 passed; 0 failed; 2 total"));

    // 4. Unit return `main()` omits trailing `()` line on `aodc run`
    let unit_file = dir.join("unit_main.dec");
    std::fs::write(&unit_file, "Dec main():\n    emit 42\n").unwrap();
    let u_out = Command::new(aodc_bin())
        .arg("run")
        .arg(&unit_file)
        .output()
        .unwrap();
    assert!(u_out.status.success());
    assert_eq!(String::from_utf8_lossy(&u_out.stdout), "42\n");

    // 5. Failing `test_*()` exits with non-zero code
    let fail_file = dir.join("fail_test.dec");
    std::fs::write(&fail_file, "Dec test_boom():\n    assert 1 == 2\n").unwrap();
    let f_out = Command::new(aodc_bin())
        .arg("test")
        .arg(&fail_file)
        .output()
        .unwrap();
    assert!(!f_out.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac22_cli_fuel_max_depth_and_prior_emit_preservation_on_error() {
    let dir = temp_dir("ac22_budgets");

    // 1. `aodc run --fuel 20` preserves prior `emit` lines on stdout while exiting 1 with `aodc:` on stderr
    let fuel_src = dir.join("fuel_loop.dec");
    std::fs::write(
        &fuel_src,
        r#"
Dec main() -> int:
    emit "before-fuel-exhaust"
    mut i = 0
    while true:
        i = i + 1
    return i
"#,
    )
    .unwrap();
    let fuel_run = Command::new(aodc_bin())
        .args(["run", "--fuel", "20"])
        .arg(&fuel_src)
        .output()
        .unwrap();
    assert!(!fuel_run.status.success());
    assert_eq!(
        String::from_utf8_lossy(&fuel_run.stdout),
        "before-fuel-exhaust\n"
    );
    let fuel_err = String::from_utf8_lossy(&fuel_run.stderr);
    assert!(fuel_err.starts_with("aodc:"), "stderr={fuel_err}");
    assert!(fuel_err.contains("budget"), "stderr={fuel_err}");

    // Also verify `aodc exec --fuel 20` on compiled IR
    let fuel_ir = dir.join("fuel_loop.ir.json");
    let comp = Command::new(aodc_bin())
        .arg("compile")
        .arg(&fuel_src)
        .arg("-o")
        .arg(&fuel_ir)
        .output()
        .unwrap();
    assert!(comp.status.success());
    let fuel_exec = Command::new(aodc_bin())
        .args(["exec", "--fuel", "20"])
        .arg(&fuel_ir)
        .output()
        .unwrap();
    assert!(!fuel_exec.status.success());
    assert_eq!(
        String::from_utf8_lossy(&fuel_exec.stdout),
        "before-fuel-exhaust\n"
    );
    assert!(String::from_utf8_lossy(&fuel_exec.stderr).starts_with("aodc:"));

    // 2. `aodc run --max-depth 5` preserves prior `emit` lines on stdout while exiting 1
    let depth_src = dir.join("depth_rec.dec");
    std::fs::write(
        &depth_src,
        r#"
Dec dive(n: int) -> int:
    emit n
    return dive(n + 1)

Dec main() -> int:
    return dive(1)
"#,
    )
    .unwrap();
    let depth_run = Command::new(aodc_bin())
        .args(["run", "--max-depth", "5"])
        .arg(&depth_src)
        .output()
        .unwrap();
    assert!(!depth_run.status.success());
    let depth_stdout = String::from_utf8_lossy(&depth_run.stdout);
    assert!(
        depth_stdout.starts_with("1\n2\n3\n4\n"),
        "expected preserved prior emits, got: {depth_stdout:?}"
    );
    let depth_err = String::from_utf8_lossy(&depth_run.stderr);
    assert!(depth_err.starts_with("aodc:"), "stderr={depth_err}");
    assert!(depth_err.contains("depth"), "stderr={depth_err}");

    // 3. Assertion failure after `emit` preserves prior `emit` lines
    let assert_src = dir.join("assert_after_emit.dec");
    std::fs::write(
        &assert_src,
        "Dec main() -> int:\n    emit 77\n    assert false\n    return 0\n",
    )
    .unwrap();
    let assert_run = Command::new(aodc_bin())
        .arg("run")
        .arg(&assert_src)
        .output()
        .unwrap();
    assert!(!assert_run.status.success());
    assert_eq!(String::from_utf8_lossy(&assert_run.stdout), "77\n");
    assert!(String::from_utf8_lossy(&assert_run.stderr).starts_with("aodc:"));

    // 4. Invalid zero budget rejected cleanly
    let bad_fuel = Command::new(aodc_bin())
        .args(["run", "--fuel", "0"])
        .arg(&assert_src)
        .output()
        .unwrap();
    assert!(!bad_fuel.status.success());
    assert!(String::from_utf8_lossy(&bad_fuel.stderr).contains("positive integer"));

    let _ = std::fs::remove_dir_all(&dir);
}
