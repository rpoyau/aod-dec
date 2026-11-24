use std::io::Read;
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
    assert_eq!(upstream.len(), 4);

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

    // Verify S2@3122c94 draft-candidate binding and vendored golden-vector JSONL SHA-256 byte-for-byte
    let s2 = &upstream[2];
    assert_eq!(s2.get("id").and_then(JsonValue::as_str), Some("S2@3122c94"));
    assert_eq!(s2.get("stage").and_then(JsonValue::as_str), Some("S2"));
    assert_eq!(
        s2.get("status").and_then(JsonValue::as_str),
        Some("DRAFT_CANDIDATE_UNDER_S2_REVIEW")
    );
    assert_eq!(
        s2.get("pr_number").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(51))
    );
    assert_eq!(
        s2.get("ref_name").and_then(JsonValue::as_str),
        Some("m4-s2-reference-engine")
    );
    assert_eq!(
        s2.get("commit").and_then(JsonValue::as_str),
        Some("3122c9444596a9f479e1444c9d497e510a87f825")
    );
    assert_eq!(
        s2.get("tree").and_then(JsonValue::as_str),
        Some("d58b43ccf82b428be8f831d11203604e12d66388")
    );
    assert_eq!(
        s2.get("base_commit_binding").and_then(JsonValue::as_str),
        Some("cea71a76f26c08b603866e746867b3982da6c88e")
    );
    let s2_vendored = s2.get("vendored_files").and_then(JsonValue::as_array).unwrap();
    assert_eq!(s2_vendored.len(), 2);
    for vf in s2_vendored {
        let local_path = vf.get("local_path").and_then(JsonValue::as_str).unwrap();
        let expected_sha = vf.get("sha256").and_then(JsonValue::as_str).unwrap();
        let bytes = std::fs::read(root.join(local_path)).unwrap();
        assert_eq!(
            Sha256::digest_hex(&bytes),
            expected_sha,
            "vendored S2 file SHA-256 mismatch for {local_path}"
        );
    }

    // Verify S4@e013401 draft-candidate binding and vendored 90-tick CRLF CSV SHA-256 byte-for-byte
    let s4 = &upstream[3];
    assert_eq!(s4.get("id").and_then(JsonValue::as_str), Some("S4@e013401"));
    assert_eq!(s4.get("stage").and_then(JsonValue::as_str), Some("S4"));
    assert_eq!(
        s4.get("status").and_then(JsonValue::as_str),
        Some("DRAFT_CANDIDATE_UNDER_S4_REVIEW")
    );
    assert_eq!(
        s4.get("pr_number").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(53))
    );
    assert_eq!(
        s4.get("ref_name").and_then(JsonValue::as_str),
        Some("m4-s4-rust-exact-core")
    );
    assert_eq!(
        s4.get("commit").and_then(JsonValue::as_str),
        Some("e0134017a816c699ee71a4342044c007b63917db")
    );
    assert_eq!(
        s4.get("tree").and_then(JsonValue::as_str),
        Some("789e28b5c0c7c24c91f580b643e1bd0017a89dd6")
    );
    assert_eq!(
        s4.get("base_commit_binding").and_then(JsonValue::as_str),
        Some("3a2418defb4fa92e6fbfc8951b81990fc57ce0e6")
    );
    let s4_vendored = s4.get("vendored_files").and_then(JsonValue::as_array).unwrap();
    assert_eq!(s4_vendored.len(), 1);
    assert_eq!(
        s4_vendored[0].get("upstream_path").and_then(JsonValue::as_str),
        Some("wavelet_shedding_simulation.csv")
    );
    assert_eq!(
        s4_vendored[0].get("local_path").and_then(JsonValue::as_str),
        Some("bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv")
    );
    let s4_csv_bytes =
        std::fs::read(root.join("bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv"))
            .unwrap();
    assert_eq!(s4_csv_bytes.len(), 11245);
    assert_eq!(
        Sha256::digest_hex(&s4_csv_bytes),
        s4_vendored[0].get("sha256").and_then(JsonValue::as_str).unwrap()
    );
    assert_eq!(
        Sha256::digest_hex(&s4_csv_bytes),
        "d7b0592997d47b2b550508d663afe8943a98ba9d58d087ce77e9d4d0a0c3ff40"
    );

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
    for crate_src in [
        "crates/aod-core/src",
        "crates/aod-rt/src",
        "crates/aod-llvm/src",
        "crates/aodc/src",
    ] {
        collect_rs_files(&root.join(crate_src), &mut rs_files);
    }
    assert!(!rs_files.is_empty());

    let allowlisted_unsafe = [
        "crates/aod-rt/src/abi.rs",
        "crates/aod-llvm/src/ffi.rs",
        "crates/aod-llvm/src/aot.rs",
        "crates/aod-llvm/src/jit.rs",
        "crates/aod-llvm/src/hw_probe.rs",
    ];

    // Verify `#![forbid(unsafe_code)]` on aod-core and aodc entrypoints
    for forbid_file in [
        "crates/aod-core/src/lib.rs",
        "crates/aodc/src/lib.rs",
        "crates/aodc/src/main.rs",
    ] {
        let content = std::fs::read_to_string(root.join(forbid_file)).unwrap();
        assert!(
            content.contains("#![forbid(unsafe_code)]"),
            "{forbid_file} must declare #![forbid(unsafe_code)]"
        );
    }

    // Verify `#![deny(unsafe_code)]` on aod-rt and aod-llvm roots
    for deny_file in ["crates/aod-rt/src/lib.rs", "crates/aod-llvm/src/lib.rs"] {
        let content = std::fs::read_to_string(root.join(deny_file)).unwrap();
        assert!(
            content.contains("#![deny(unsafe_code)]"),
            "{deny_file} must declare #![deny(unsafe_code)]"
        );
    }

    // Build forbidden float token strings dynamically so this test file itself stays clean.
    let f32_tok = ["f", "32"].concat();
    let f64_tok = ["f", "64"].concat();
    let unsafe_tok = ["un", "safe"].concat();

    for path in rs_files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let is_unsafe_allowed = allowlisted_unsafe.contains(&rel.as_str());
        let content = std::fs::read_to_string(&path).unwrap();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            let mut ident = String::new();
            for ch in trimmed.chars().chain(std::iter::once(' ')) {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ident.push(ch);
                } else {
                    if ident == f32_tok || ident == f64_tok {
                        panic!(
                            "forbidden float type token `{ident}` found in {}:{}",
                            rel,
                            line_idx + 1
                        );
                    }
                    if !is_unsafe_allowed
                        && ident == unsafe_tok
                        && !trimmed.contains("unsafe_code")
                    {
                        panic!(
                            "forbidden `{unsafe_tok}` token outside allowlisted FFI files in {}:{}",
                            rel,
                            line_idx + 1
                        );
                    }
                    ident.clear();
                }
            }
        }
    }

    // Verify ALL 4 crate Cargo.toml manifests contain zero external crates.io dependencies
    let allowed_local = ["aod-core", "aod-rt", "aod-llvm"];
    for manifest in [
        "crates/aod-core/Cargo.toml",
        "crates/aod-rt/Cargo.toml",
        "crates/aod-llvm/Cargo.toml",
        "crates/aodc/Cargo.toml",
    ] {
        let text = std::fs::read_to_string(root.join(manifest)).unwrap();
        let deps_section = text.split("[dependencies]").nth(1).unwrap();
        let before_next = deps_section.split('[').next().unwrap();
        for line in before_next.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let dep_name = t.split('=').next().unwrap().trim();
            assert!(
                allowed_local.contains(&dep_name) && t.contains("path ="),
                "external dependency `{t}` forbidden in {manifest}"
            );
        }
    }
}

fn has_dot_py_token(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b".py" {
            let after = bytes.get(i + 3).copied();
            let after2 = bytes.get(i + 4).copied();
            let is_py_ext = match after {
                None => true,
                Some(b) if !(b.is_ascii_alphanumeric() || b == b'_') => true,
                Some(b'c' | b'o' | b'i') => match after2 {
                    None => true,
                    Some(b2) => !(b2.is_ascii_alphanumeric() || b2 == b'_'),
                },
                _ => false,
            };
            if is_py_ext {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn check_repo_zero_python(root: &Path, dir: &Path) {
    for entry_res in std::fs::read_dir(dir).unwrap() {
        let entry = entry_res.unwrap();
        let ft = entry.file_type().unwrap(); // Never follows symlinks (`P0-3`)
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let path = entry.path();

        if ft.is_dir() {
            if name_str == ".git" || name_str == "target" {
                continue;
            }
            assert_ne!(
                name_str, "__pycache__",
                "forbidden __pycache__ directory found at {}",
                path.display()
            );
            check_repo_zero_python(root, &path);
            continue;
        }

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            assert!(
                !matches!(ext_lower.as_str(), "py" | "pyc" | "pyo" | "pyi"),
                "forbidden Python artifact `{}` found in repository",
                path.display()
            );
        }

        if ft.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "SOURCE_BINDING.json" || rel.starts_with("bindings/aod/s2_vectors/") {
                continue;
            }
            let mut f = std::fs::File::open(&path).unwrap();
            let mut head = [0u8; 128];
            let mut read_len = 0usize;
            while read_len < head.len() {
                let n = f.read(&mut head[read_len..]).unwrap();
                if n == 0 {
                    break;
                }
                read_len += n;
            }
            let prefix = &head[..read_len];
            if prefix.starts_with(b"#!") {
                let first_line_end = prefix
                    .iter()
                    .position(|&b| b == b'\n' || b == b'\r')
                    .unwrap_or(prefix.len());
                let shebang = String::from_utf8_lossy(&prefix[..first_line_end]).to_ascii_lowercase();
                assert!(
                    !shebang.contains("python") && !shebang.contains("pypy"),
                    "forbidden Python shebang `{shebang}` in {rel}"
                );
            }
        }
    }
}

#[test]
fn zero_python_repository_and_ci_guard() {
    let root = repo_root();

    // 1. Walk entire repository (skipping .git & target, never following symlinks)
    check_repo_zero_python(&root, &root);

    // 2. Confirm legacy Python scripts are removed and .gitignore has no Python cache entries
    assert!(!root.join("scripts/audit_pdf_zero_raster.py").exists());
    assert!(!root.join("scripts/test_audit_pdf_zero_raster.py").exists());
    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(
        !gitignore.contains("__pycache__") && !gitignore.contains("*.pyc"),
        ".gitignore must not retain Python cache entries"
    );

    // 3. Scan every `.github/workflows/*.{yml,yaml}` file strictly per P0-3
    let wf_dir = root.join(".github/workflows");
    let mut wf_files: Vec<PathBuf> = std::fs::read_dir(&wf_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect();
    wf_files.sort();
    assert!(!wf_files.is_empty(), "expected at least one CI workflow file");

    let forbidden_tokens = [
        "python3",
        "python2",
        "pypy",
        "pytest",
        "pip",
        "pip3",
        "unittest",
        "setup-python",
        "virtualenv",
        "conda",
        "poetry",
    ];
    let forbidden_substrings = [
        "python3",
        "python2",
        "pypy",
        "pytest",
        "setup-python",
        "virtualenv",
        "unittest",
    ];

    let mut saw_ci_yml = false;
    for wf in &wf_files {
        let rel = wf
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(wf).unwrap();
        if rel == ".github/workflows/ci.yml" {
            saw_ci_yml = true;
            assert!(
                text.contains("aodc audit-pdf"),
                "ci.yml must invoke pure-Rust `aodc audit-pdf`"
            );
            assert!(
                text.contains("pdfimages"),
                "ci.yml must retain `pdfimages` cross-check"
            );
        }

        for (idx, line) in text.lines().enumerate() {
            let line_no = idx + 1;
            let lower = line.to_ascii_lowercase();

            for sub in forbidden_substrings {
                assert!(
                    !lower.contains(sub),
                    "forbidden Python substring `{sub}` in {rel}:{line_no}: {line}"
                );
            }
            for tok in lower.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
                if forbidden_tokens.contains(&tok) {
                    panic!(
                        "forbidden Python token `{tok}` in {rel}:{line_no}: {line}"
                    );
                }
            }
            assert!(
                !has_dot_py_token(&lower),
                "forbidden `.py` file token in {rel}:{line_no}: {line}"
            );

            let trimmed = line.trim();
            let bare_check = if trimmed.starts_with("branches:") {
                trimmed.replace("\"v2/zero-python-evolution\"", "")
            } else {
                line.to_string()
            };
            assert!(
                !bare_check.to_ascii_lowercase().contains("python"),
                "forbidden bare `python` mention outside exact allowlisted branch literal in {rel}:{line_no}: {line}"
            );
        }
    }
    assert!(saw_ci_yml, "expected .github/workflows/ci.yml");
}
