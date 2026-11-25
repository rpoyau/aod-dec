use std::path::PathBuf;

use aod_core::sha256::Sha256;
use aodc::diagnostics::CompatMode;
use aodc::{check_file, check_file_with_options, compile_file_to_ir, exec_ir_str, run_file, LoadOptions};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn all_10_frozen_v1_examples_match_manifest_and_stdout_in_v1_ir_and_reject_in_v2() {
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

        // 1. `--compat v1` mode (`LoadOptions::for_compat` AND `LoadOptions::default()`, `[P0-3]`, `AC-05`)
        check_file_with_options(&src_path, CompatMode::V1, &LoadOptions::default())
            .unwrap_or_else(|e| panic!("v1 check_file_with_options(default) failed for {name}: {e}"));
        let v1_out = run_file(&src_path, CompatMode::V1)
            .unwrap_or_else(|e| panic!("v1 compat run failed for {name}: {e}"));
        assert_eq!(
            v1_out.to_stdout_string(),
            expected_stdout,
            "v1 compat stdout mismatch for {name}"
        );

        // 2. DecIR v1 compile -> loads_ir -> exec round-trip
        let ir_v1 = compile_file_to_ir(&src_path, CompatMode::V1).unwrap();
        assert!(ir_v1.contains("\"version\": 1"));
        let exec_v1 = exec_ir_str(&ir_v1).unwrap();
        assert_eq!(
            exec_v1.to_stdout_string(),
            expected_stdout,
            "DecIR v1 exec mismatch for {name}"
        );

        // 3. Unconditional PureWaveV2 `CompatMode::V2` MUST reject legacy v1 syntax (`AC-05`)
        assert!(
            check_file(&src_path, CompatMode::V2).is_err(),
            "CompatMode::V2 must reject legacy v1 fixture {name}"
        );
    }
}

#[test]
fn compat_v1_rejects_v2_only_syntax() {
    let v2_prog = "dec main:\n    () -> int:\n        let x = 1/3r\n        return 0\n";
    assert!(aodc::check_source(v2_prog, CompatMode::V2).is_ok());
    assert!(aodc::check_source(v2_prog, CompatMode::V1).is_err());
}
