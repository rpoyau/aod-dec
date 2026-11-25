use std::path::PathBuf;
use std::process::Command;

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

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

#[test]
fn self_host_four_stage_fixed_point_cli_json_and_trit_sample_parity() {
    let root = repo_root();
    let tmp_dir = std::env::temp_dir().join(format!("aodc_sh_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();

    let custom_sample = tmp_dir.join("trit_pipeline_sample.dec");
    let stage2_bin = tmp_dir.join("aodc_stage2_emitted");
    let sample_src = "\
struct Vec2:
    x: int
    y: int

dec scale_role:
    (v: int) where v > 10 -> int:
        v * 2
    (0: int) -> int:
        99
    (v: int) -> int:
        -v + 3

dec add_sq(acc: int, k: int) -> int:
    acc + (k * k)

dec triple(k: int) -> int:
    k * 3

dec main() -> int:
    let anchor = 00_8
    assert anchor != 0_8
    let pt = Vec2(15, -7)
    let packed = b64(-123456789)
    let unpacked = int(packed)
    assert unpacked == -123456789
    let xs = range_seq(1, 6)
    let sq_sum = fold(xs, 0, add_sq)
    let piped = each(xs, triple) |> +/
    let role = pt.x <=> pt.y
    let bonus = mux(role, -5, 0, 11)
    emit scale_role(pt.x)
    emit scale_role(0)
    emit scale_role(pt.y)
    emit sq_sum + piped + bonus
    0
";
    std::fs::write(&custom_sample, sample_src).unwrap();

    // 1. Verify 4-stage fixed-point bootstrap, JSON schema, and custom Trit-Boxed sample 3-way parity
    let out = Command::new(aodc_bin())
        .current_dir(&root)
        .args([
            "bootstrap-verify",
            "--sample",
            &custom_sample.to_string_lossy(),
            "--emit-stage2",
            &stage2_bin.to_string_lossy(),
            "--json",
        ])
        .output()
        .expect("spawn aodc bootstrap-verify --json");
    assert!(
        out.status.success(),
        "bootstrap-verify failed: status={}, stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let doc = parse_strict_json(stdout.trim()).expect("strict zero-float JSON");
    assert_eq!(
        doc.get("schema").and_then(JsonValue::as_str),
        Some("aod.dec.bootstrap_verify.v1")
    );
    assert_eq!(doc.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(
        doc.get("fixed_point_verified").and_then(JsonValue::as_bool),
        Some(true)
    );
    assert_eq!(
        doc.get("sample_stdout_parity_verified")
            .and_then(JsonValue::as_bool),
        Some(true)
    );
    let s2_sha = doc.get("stage2_sha256").and_then(JsonValue::as_str).unwrap();
    let s3_sha = doc.get("stage3_sha256").and_then(JsonValue::as_str).unwrap();
    assert_eq!(s2_sha.len(), 64);
    assert_eq!(s2_sha, s3_sha, "SHA256(stage2) must equal SHA256(stage3)");
    let s2_bytes = doc.get("stage2_bytes").and_then(JsonValue::as_int).unwrap();
    let s3_bytes = doc.get("stage3_bytes").and_then(JsonValue::as_int).unwrap();
    assert_eq!(s2_bytes, s3_bytes);

    // 2. Verify emitted stage2 ELF64 header, SHA-256 identity, and static libc-free isolation
    let emitted_bytes = std::fs::read(&stage2_bin).expect("read emitted stage2 binary");
    assert!(emitted_bytes.starts_with(b"\x7FELF"));
    assert_eq!(Sha256::digest_hex(&emitted_bytes), s2_sha);
    let ldd_out = Command::new("ldd")
        .arg(&stage2_bin)
        .output()
        .expect("run ldd on stage2 ELF");
    let ldd_combined = format!(
        "{}{}",
        String::from_utf8_lossy(&ldd_out.stdout),
        String::from_utf8_lossy(&ldd_out.stderr)
    );
    assert!(
        ldd_combined.contains("not a dynamic executable")
            || ldd_combined.contains("statically linked"),
        "expected static libc-free ELF64, got ldd output: {ldd_combined}"
    );

    // 3. Negative control: §5.17 Spine Audit rejects source with zero `dec` blocks without creating output ELF
    let bad_sample = tmp_dir.join("missing_spine.dec");
    let bad_elf = tmp_dir.join("should_not_exist.elf");
    std::fs::write(&bad_sample, "const ONLY_CONST: int = 42\n").unwrap();
    let bad_run = Command::new(&stage2_bin)
        .args([&bad_sample, &bad_elf])
        .output()
        .expect("run stage2 on malformed Dec source");
    assert!(
        !bad_run.status.success(),
        "malformed sample without dec block must be rejected by stage2 spine audit"
    );
    assert!(
        !bad_elf.exists(),
        "stage2 must not create output ELF when spine audit fails"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
