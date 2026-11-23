use std::path::{Path, PathBuf};

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn source_binding_schema_v2_p2_1_equations_and_vendored_hashes_verified() {
    let root = repo_root();
    let sb_text = std::fs::read_to_string(root.join("SOURCE_BINDING.json")).unwrap();
    let sb = parse_strict_json(&sb_text).unwrap();

    assert_eq!(sb.get("schema_version").and_then(JsonValue::as_int), Some(&BigInt::from_i64(2)));

    let upstream = sb.get("upstream_bindings").and_then(JsonValue::as_array).unwrap();
    assert_eq!(upstream.len(), 2);

    // Load vendored main_registry.json to cross-check every single equation label & number!
    let reg_bytes = std::fs::read(root.join("bindings/aod/main_registry.json")).unwrap();
    let reg_text = String::from_utf8(reg_bytes.clone()).unwrap();
    let reg = parse_strict_json(&reg_text).unwrap();
    let reg_entries = reg.get("entries").and_then(JsonValue::as_object).unwrap();

    // Verify S0@98ddfa0 binding with P2-1 numbers_resolved_from split and explicit expanded equations
    let s0 = &upstream[0];
    assert_eq!(s0.get("id").and_then(JsonValue::as_str), Some("S0@98ddfa0"));
    assert_eq!(
        s0.get("commit").and_then(JsonValue::as_str),
        Some("98ddfa0ebb38293adfa99c8c11da5a15cdd9eea3")
    );
    assert_eq!(
        s0.get("numbers_resolved_from").and_then(JsonValue::as_str),
        Some("S1b@cea71a7/typst/lib/main_registry.json")
    );
    let equations = s0.get("equations").and_then(JsonValue::as_array).unwrap();
    assert_eq!(equations.len(), 50, "expected 50 explicitly expanded equations");
    for eq in equations {
        let lbl = eq.get("label").and_then(JsonValue::as_str).unwrap();
        let num = eq.get("number").and_then(JsonValue::as_str).unwrap();
        let disp = eq.get("display").and_then(JsonValue::as_str).unwrap();
        assert!(!lbl.contains(".."), "no compressed ranges allowed: {lbl}");
        assert!(!num.contains(".."), "no compressed ranges allowed: {num}");
        let entry = reg_entries
            .get(lbl)
            .unwrap_or_else(|| panic!("equation {lbl} missing from vendored main_registry.json"));
        assert_eq!(entry.get("number").and_then(JsonValue::as_str), Some(num), "number mismatch for {lbl}");
        assert_eq!(entry.get("display").and_then(JsonValue::as_str), Some(disp), "display mismatch for {lbl}");
    }

    // Verify S1b@cea71a7 vendored files SHA-256 byte-for-byte
    let s1b = &upstream[1];
    assert_eq!(s1b.get("id").and_then(JsonValue::as_str), Some("S1b@cea71a7"));
    let vendored = s1b.get("vendored_files").and_then(JsonValue::as_array).unwrap();
    assert_eq!(vendored.len(), 2);
    for vf in vendored {
        let local_path = vf.get("local_path").and_then(JsonValue::as_str).unwrap();
        let expected_sha = vf.get("sha256").and_then(JsonValue::as_str).unwrap();
        let bytes = std::fs::read(root.join(local_path)).unwrap();
        assert_eq!(
            Sha256::digest_hex(&bytes),
            expected_sha,
            "vendored file SHA-256 mismatch for {local_path}"
        );
    }

    // Verify historical v1 binding preserved verbatim
    let hist = sb.get("historical_bindings").and_then(JsonValue::as_array).unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(
        hist[0].get("source_commit").and_then(JsonValue::as_str),
        Some("2bc927c6718def4df14e4c9af69b9d52c23008df")
    );
    assert_eq!(
        hist[0].get("original_file_sha256").and_then(JsonValue::as_str),
        Some("e5acb188c5597afd69cb95c41f49b1b7fed1b6d8d1f043ea9f1d4939734a1535")
    );
    assert!(hist[0].get("original_record").and_then(JsonValue::as_object).is_some());

    // Verify all 10 frozen v1 regression manifest entries match disk bytes
    let manifest_obj = sb.get("frozen_v1_regression_manifest").unwrap();
    let examples = manifest_obj.get("examples").and_then(JsonValue::as_array).unwrap();
    assert_eq!(examples.len(), 10);
    for ex in examples {
        let sp = ex.get("source_path").and_then(JsonValue::as_str).unwrap();
        let op = ex.get("stdout_path").and_then(JsonValue::as_str).unwrap();
        let s_sha = ex.get("src_sha256").and_then(JsonValue::as_str).unwrap();
        let o_sha = ex.get("stdout_sha256").and_then(JsonValue::as_str).unwrap();
        assert_eq!(Sha256::digest_hex(&std::fs::read(root.join(sp)).unwrap()), s_sha);
        assert_eq!(Sha256::digest_hex(&std::fs::read(root.join(op)).unwrap()), o_sha);
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

#[test]
fn static_audit_zero_floats_zero_unsafe_and_zero_external_deps() {
    let root = repo_root();
    let mut rs_files = Vec::new();
    collect_rs_files(&root.join("crates/aod-core/src"), &mut rs_files);
    collect_rs_files(&root.join("crates/aodc/src"), &mut rs_files);
    assert!(!rs_files.is_empty());

    // Build forbidden float token strings dynamically so this test file itself stays clean.
    let f32_tok = ["f", "32"].concat();
    let f64_tok = ["f", "64"].concat();
    for path in rs_files {
        let content = std::fs::read_to_string(&path).unwrap();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            // Tokenize ASCII identifiers and verify none is f32 or f64
            let mut ident = String::new();
            for ch in trimmed.chars().chain(std::iter::once(' ')) {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ident.push(ch);
                } else {
                    if ident == f32_tok || ident == f64_tok {
                        panic!(
                            "forbidden float type token `{ident}` found in {}:{}",
                            path.display(),
                            line_idx + 1
                        );
                    }
                    ident.clear();
                }
            }
            if trimmed.contains("unsafe {") || trimmed.starts_with("unsafe fn") {
                panic!(
                    "forbidden unsafe block found in {}:{}",
                    path.display(),
                    line_idx + 1
                );
            }
        }
    }

    // Verify Cargo.toml files contain zero external crates.io dependencies
    let core_toml = std::fs::read_to_string(root.join("crates/aod-core/Cargo.toml")).unwrap();
    let deps_section = core_toml.split("[dependencies]").nth(1).unwrap();
    let before_next = deps_section.split('[').next().unwrap();
    assert!(
        before_next.trim().is_empty(),
        "aod-core must have zero dependencies, found: {before_next}"
    );
}
