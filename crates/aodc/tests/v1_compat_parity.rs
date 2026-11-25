use std::path::PathBuf;
use std::process::Command;

use aod_core::sha256::Sha256;
use aodc::{compile_file_to_ir, exec_ir_str, jit_file_captured, run_file, OptLevel};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn all_10_frozen_v1_examples_preserve_manifest_hashes_and_reject_legacy_compat() {
    let v1_dir = repo_root().join("tests/fixtures/v1_examples");
    let manifest = std::fs::read_to_string(v1_dir.join("MANIFEST.txt")).unwrap();
    let lines: Vec<&str> = manifest.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 10, "expected exactly 10 frozen v1 examples");

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 3, "malformed MANIFEST line: {line}");
        let name = parts[0];
        let expected_src_sha = parts[1].strip_prefix("src_sha256=").unwrap();
        let expected_out_sha = parts[2].strip_prefix("stdout_sha256=").unwrap();

        let src_path = v1_dir.join(name);
        let out_path = src_path.with_extension("stdout");

        let src_bytes = std::fs::read(&src_path).unwrap();
        let out_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(
            Sha256::digest_hex(&src_bytes),
            expected_src_sha,
            "frozen source SHA-256 mismatch for {name}"
        );
        assert_eq!(
            Sha256::digest_hex(&out_bytes),
            expected_out_sha,
            "frozen stdout SHA-256 mismatch for {name}"
        );

        // Legacy uppercase `Dec` is rejected by the clean-break frontend
        let err = aodc::check_file(&src_path)
            .expect_err("clean-break frontend must reject legacy v1 syntax");
        assert!(
            err.message.contains("uppercase 'Dec' is removed")
                || err.message.contains("removed"),
            "unexpected diagnostic for {name}: {err}"
        );
    }

    // Verify `--compat` flag is rejected on CLI
    let bin = env!("CARGO_BIN_EXE_aodc");
    let out = Command::new(bin)
        .args(["check", "--compat", "v1", v1_dir.join("weights.dec").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success(), "--compat must be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown") || stderr.contains("--compat"));
}

#[test]
fn all_10_clean_break_v3_examples_match_manifest_vm_ir_and_jit() {
    let v3_dir = repo_root().join("tests/fixtures/v3_examples");
    let manifest = std::fs::read_to_string(v3_dir.join("MANIFEST.txt")).unwrap();
    let lines: Vec<&str> = manifest.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 10, "expected exactly 10 clean-break v3 examples");

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 3, "malformed MANIFEST line: {line}");
        let name = parts[0];
        let expected_src_sha = parts[1].strip_prefix("src_sha256=").unwrap();
        let expected_out_sha = parts[2].strip_prefix("stdout_sha256=").unwrap();

        let src_path = v3_dir.join(name);
        let out_path = src_path.with_extension("stdout");

        let src_bytes = std::fs::read(&src_path).unwrap();
        let out_bytes = std::fs::read(&out_path).unwrap();
        assert_eq!(
            Sha256::digest_hex(&src_bytes),
            expected_src_sha,
            "v3 source SHA-256 mismatch for {name}"
        );
        assert_eq!(
            Sha256::digest_hex(&out_bytes),
            expected_out_sha,
            "v3 stdout SHA-256 mismatch for {name}"
        );
        let expected_stdout = String::from_utf8(out_bytes).unwrap();

        // 1. Exact VM mode
        let vm_out = run_file(&src_path)
            .unwrap_or_else(|e| panic!("VM run failed for {name}: {e}"));
        assert_eq!(
            vm_out.to_stdout_string(),
            expected_stdout,
            "VM stdout mismatch for {name}"
        );

        // 2. DecIR v3 compile -> loads_ir -> exec round-trip
        let ir_v3 = compile_file_to_ir(&src_path).unwrap();
        assert!(ir_v3.contains("\"version\": 3"));
        let exec_v3 = exec_ir_str(&ir_v3).unwrap();
        assert_eq!(
            exec_v3.to_stdout_string(),
            expected_stdout,
            "DecIR v3 exec mismatch for {name}"
        );

        // 3. Native JIT-O0 & JIT-O2 parity
        for opt in [OptLevel::O0, OptLevel::O2] {
            let (jout, jres) = jit_file_captured(&src_path, opt, 10_000_000, 256)
                .unwrap_or_else(|e| panic!("JIT compile ({opt:?}) failed for {name}: {e}"));
            jres.unwrap_or_else(|e| panic!("JIT runtime error ({opt:?}) for {name}: {e}"));
            assert_eq!(
                jout,
                expected_stdout,
                "JIT stdout mismatch ({opt:?}) for {name}"
            );
        }
    }
}
