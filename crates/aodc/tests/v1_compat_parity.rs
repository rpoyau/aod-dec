use std::path::PathBuf;
use std::process::Command;

use aod_core::sha256::Sha256;
use aodc::{
    check_v1_file, compile_file_to_ir, compile_v1_file_to_ir, compile_v1_source_to_ir, exec_ir_str,
    jit_file_captured, run_file, run_v1_file, run_v1_source, OptLevel,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn all_10_frozen_v1_examples_preserve_manifest_hashes_and_replay_under_compat_v1() {
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
        let expected_stdout = String::from_utf8(out_bytes).unwrap();

        // 1. Default Pure-Wave v3 frontend still rejects legacy uppercase `Dec`
        let err = aodc::check_file(&src_path)
            .expect_err("default clean-break v3 frontend must reject legacy v1 syntax");
        assert!(
            err.message.contains("uppercase 'Dec' is removed")
                || err.message.contains("removed"),
            "unexpected diagnostic for {name}: {err}"
        );

        // 2. Explicit `--compat v1` check, VM run, and hermetic DecIR v1 exec match byte-for-byte
        check_v1_file(&src_path)
            .unwrap_or_else(|e| panic!("check_v1_file failed for {name}: {e}"));
        let vm_v1 = run_v1_file(&src_path, 10_000_000, 256)
            .unwrap_or_else(|e| panic!("run_v1_file failed for {name}: {e}"));
        assert_eq!(
            vm_v1.to_stdout_string(),
            expected_stdout,
            "--compat v1 VM stdout mismatch for {name}"
        );

        let ir_v1 = compile_v1_file_to_ir(&src_path)
            .unwrap_or_else(|e| panic!("compile_v1_file_to_ir failed for {name}: {e}"));
        assert!(ir_v1.contains("\"version\": 1"), "expected DecIR v1 for {name}");
        let exec_v1 = exec_ir_str(&ir_v1)
            .unwrap_or_else(|e| panic!("exec_ir_str (v1) failed for {name}: {e}"));
        assert_eq!(
            exec_v1.to_stdout_string(),
            expected_stdout,
            "DecIR v1 exec stdout mismatch for {name}"
        );
    }

    // 3. CLI `aodc test --compat v1 tests/fixtures/v1_examples` passes 10/10
    let bin = env!("CARGO_BIN_EXE_aodc");
    let ok = Command::new(bin)
        .args(["test", "--compat", "v1", v1_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "aodc test --compat v1 failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(String::from_utf8_lossy(&ok.stdout).contains("10 passed; 0 failed; 10 total"));

    // 4. Invalid `--compat` mode is rejected cleanly, and `--compat v1` rejects v3-only syntax
    let bad_mode = Command::new(bin)
        .args(["check", "--compat", "v99", v1_dir.join("weights.dec").to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!bad_mode.status.success(), "unknown --compat mode must fail");
    assert!(
        aodc::check_v1_source("dec main() -> int:\n    0\n").is_err(),
        "--compat v1 must reject v3 lowercase `dec` syntax"
    );

    // 5. Hostile invalid continuations fail identically in both v1 VM and DecIR v1:
    //    (a) Hamming-2 step `q4(0) -> q4(3)` in closure.dec
    let closure_src = std::fs::read_to_string(v1_dir.join("closure.dec")).unwrap();
    let bad_hamming = closure_src.replacen("step(t, q4(1), t1(1))", "step(t, q4(3), t1(1))", 1);
    assert!(
        run_v1_source(&bad_hamming, 10_000_000, 256).is_err(),
        "Hamming-2 step must fail in v1 VM"
    );
    let bad_hamming_ir = compile_v1_source_to_ir(&bad_hamming).unwrap();
    assert!(exec_ir_str(&bad_hamming_ir).is_err(), "Hamming-2 step must fail in DecIR v1");

    //    (b) `fuse(proposal, c)` before outer trace `c` independently closes in fusion.dec
    let fusion_src = std::fs::read_to_string(v1_dir.join("fusion.dec")).unwrap();
    let bad_fusion = fusion_src.replacen("        c = recur(c)\n", "", 1);
    assert!(
        run_v1_source(&bad_fusion, 10_000_000, 256).is_err(),
        "premature fuse must fail in v1 VM"
    );
    let bad_fusion_ir = compile_v1_source_to_ir(&bad_fusion).unwrap();
    assert!(exec_ir_str(&bad_fusion_ir).is_err(), "premature fuse must fail in DecIR v1");

    //    (c) Repeated contact at `(6, 7)` in coupling.dec
    let coupling_src = std::fs::read_to_string(v1_dir.join("coupling.dec")).unwrap();
    let bad_contact = coupling_src.replacen(
        "        pair = contact(pair, 6, 7)\n",
        "        pair = contact(pair, 6, 7)\n        pair = contact(pair, 6, 7)\n",
        1,
    );
    assert!(
        run_v1_source(&bad_contact, 10_000_000, 256).is_err(),
        "repeated contact (6, 7) must fail in v1 VM"
    );
    let bad_contact_ir = compile_v1_source_to_ir(&bad_contact).unwrap();
    assert!(exec_ir_str(&bad_contact_ir).is_err(), "repeated contact (6, 7) must fail in DecIR v1");
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
