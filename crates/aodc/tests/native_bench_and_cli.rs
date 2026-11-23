use std::path::PathBuf;
use std::process::Command;

use aodc::bench_file;
use aodc::diagnostics::CompatMode;
use aodc::eval::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH};

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
    let d = std::env::temp_dir().join(format!("aodc_cli2_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn ac11_standalone_aot_elf_ldd_isolation_cli_flags_and_overwrite_refusal() {
    let root = repo_root();
    let bench_src = root.join("tests/fixtures/v2_bench/unboxed_loop_bench.dec");
    let tmp = temp_dir("ac11_elf");

    // 1. Refuse `emit-llvm src -o src` and `build src -o src`
    let self_ll = Command::new(aodc_bin())
        .args(["emit-llvm", "-O2"])
        .arg(&bench_src)
        .arg("-o")
        .arg(&bench_src)
        .output()
        .unwrap();
    assert!(!self_ll.status.success());
    assert!(String::from_utf8_lossy(&self_ll.stderr).contains("differ"));

    let self_build = Command::new(aodc_bin())
        .args(["build", "-O2"])
        .arg(&bench_src)
        .arg("-o")
        .arg(&bench_src)
        .output()
        .unwrap();
    assert!(!self_build.status.success());
    assert!(String::from_utf8_lossy(&self_build.stderr).contains("differ"));

    // 2. Build standalone AOT ELF binary
    let exe_path = tmp.join("unboxed_bench_elf");
    let build = Command::new(aodc_bin())
        .args(["build", "-O2"])
        .arg(&bench_src)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "aodc build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(exe_path.is_file());

    // 3. `ldd` isolation audit: ZERO libLLVM, ZERO libpython, ZERO libgmp!
    let ldd = Command::new("ldd").arg(&exe_path).output().unwrap();
    assert!(ldd.status.success());
    let ldd_out = String::from_utf8_lossy(&ldd.stdout);
    let lower = ldd_out.to_ascii_lowercase();
    assert!(!lower.contains("libllvm"), "standalone ELF must not link libLLVM:\n{ldd_out}");
    assert!(!lower.contains("libpython"), "standalone ELF must not link libpython:\n{ldd_out}");
    assert!(!lower.contains("libgmp"), "standalone ELF must not link libgmp:\n{ldd_out}");

    // 4. Native ELF `--fuel` and `--max-depth` flags
    let ok_run = Command::new(&exe_path)
        .args(["--fuel", "100000", "--max-depth", "128"])
        .output()
        .unwrap();
    assert!(ok_run.status.success());

    let low_fuel = Command::new(&exe_path)
        .args(["--fuel", "5"])
        .output()
        .unwrap();
    assert!(!low_fuel.status.success());
    assert!(String::from_utf8_lossy(&low_fuel.stderr).contains("aodc:"));

    let bad_fuel = Command::new(&exe_path)
        .args(["--fuel", "0"])
        .output()
        .unwrap();
    assert!(!bad_fuel.status.success());
    assert!(String::from_utf8_lossy(&bad_fuel.stderr).contains("positive integer"));

    // 5. `aodc jit` CLI subcommand matches `aodc run` byte-for-byte
    let run_out = Command::new(aodc_bin())
        .arg("run")
        .arg(&bench_src)
        .output()
        .unwrap();
    let jit_out = Command::new(aodc_bin())
        .args(["jit", "-O2"])
        .arg(&bench_src)
        .output()
        .unwrap();
    assert!(run_out.status.success() && jit_out.status.success());
    assert_eq!(run_out.stdout, jit_out.stdout);
    assert_eq!(ok_run.stdout, run_out.stdout);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac15_aodc_bench_five_mode_parity_and_5x_speedup() {
    let bench_src = repo_root().join("tests/fixtures/v2_bench/unboxed_loop_bench.dec");

    // 1. Direct API benchmark check: 5/5 parity + >= 5.00x speedup for BOTH AOT -O2 and ORC JIT -O2
    let report = bench_file(&bench_src, CompatMode::V2, 5, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).unwrap();
    assert!(report.parity_ok);
    assert!(
        report.aot_o2_speedup_x100 >= 500,
        "expected AOT -O2 speedup >= 5.00x, got {}.{:02}x (vm={}us, aot_o2={}us)",
        report.aot_o2_speedup_x100 / 100,
        report.aot_o2_speedup_x100 % 100,
        report.vm_median_us,
        report.aot_o2_median_us
    );
    assert!(
        report.jit_o2_speedup_x100 >= 500,
        "expected ORC JIT -O2 speedup >= 5.00x, got {}.{:02}x (vm={}us, jit_o2={}us)",
        report.jit_o2_speedup_x100 / 100,
        report.jit_o2_speedup_x100 % 100,
        report.vm_median_us,
        report.jit_o2_median_us
    );

    // 2. CLI `aodc bench` invocation
    let cli = Command::new(aodc_bin())
        .args(["bench", "--iters", "3"])
        .arg(&bench_src)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "aodc bench CLI failed: {}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let stdout = String::from_utf8_lossy(&cli.stdout);
    assert!(stdout.contains("5/5 byte-identical"), "stdout:\n{stdout}");
    assert!(stdout.contains("aot_o2:"), "stdout:\n{stdout}");
    assert!(stdout.contains("jit_o2:"), "stdout:\n{stdout}");
}
