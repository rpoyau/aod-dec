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

    // 4. Multi-file imported struct as field (minimal control + nontrivial computed VM parity)
    let mf_dir = tmp_dir.join("multi_file");
    std::fs::create_dir_all(&mf_dir).unwrap();
    let net_src = "struct Net:\n    x: int\n";
    std::fs::write(mf_dir.join("net.dec"), net_src).unwrap();
    let minimal_imported = mf_dir.join("minimal_imported.dec");
    let minimal_local = mf_dir.join("minimal_local.dec");
    let minimal_body = "\
struct Teaching:
    n: Net
    labels: [[int]]

dec main() -> int:
    0
";
    std::fs::write(&minimal_imported, format!("use net:\n    Net\n\n{minimal_body}")).unwrap();
    std::fs::write(&minimal_local, format!("{net_src}\n{minimal_body}")).unwrap();
    for (label, src_path) in [("imported_min", &minimal_imported), ("local_min", &minimal_local)] {
        let elf = mf_dir.join(format!("{label}.elf"));
        let vm = Command::new(aodc_bin())
            .args(["run", &src_path.to_string_lossy()])
            .output()
            .expect("vm minimal");
        assert!(vm.status.success(), "{label} vm failed: {}", String::from_utf8_lossy(&vm.stderr));
        let comp = Command::new(&stage2_bin)
            .args([src_path, &elf])
            .output()
            .expect("stage2 minimal");
        assert!(comp.status.success(), "{label} stage2 failed: {}", String::from_utf8_lossy(&comp.stderr));
        let run = Command::new(&elf).output().expect("run minimal elf");
        assert!(run.status.success());
        assert_eq!(run.stdout, vm.stdout);
    }

    let nontrivial_imported = mf_dir.join("nontrivial_imported.dec");
    let nontrivial_elf = mf_dir.join("nontrivial_imported.elf");
    std::fs::write(
        &nontrivial_imported,
        "\
use net:
    Net

struct Teaching:
    n: Net
    labels: [[int]]

dec main() -> int:
    let t = Teaching(Net(42), [[1, 2], [3, 4]])
    t.n.x + t.labels[1][1]
",
    )
    .unwrap();
    let vm_nt = Command::new(aodc_bin())
        .args(["run", &nontrivial_imported.to_string_lossy()])
        .output()
        .expect("vm nontrivial");
    assert!(vm_nt.status.success());
    let s2_nt = Command::new(&stage2_bin)
        .args([&nontrivial_imported, &nontrivial_elf])
        .output()
        .expect("stage2 nontrivial");
    assert!(s2_nt.status.success(), "stage2 nontrivial failed");
    let run_nt = Command::new(&nontrivial_elf).output().expect("run nontrivial elf");
    assert!(run_nt.status.success());
    assert_eq!(String::from_utf8_lossy(&run_nt.stdout).trim(), "46");
    assert_eq!(run_nt.stdout, vm_nt.stdout);

    // 5. Separate 6-argument (21) and 7-argument (28) function calls with VM parity
    for (arity, want) in [(6usize, "21"), (7usize, "28")] {
        let names = ["a", "b", "c", "d", "e", "f", "g"];
        let params: String = names[..arity]
            .iter()
            .map(|n| format!("{n}: int"))
            .collect::<Vec<_>>()
            .join(", ");
        let sum_expr = names[..arity].join(" + ");
        let args: String = (1..=arity)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let src_path = mf_dir.join(format!("arity_{arity}.dec"));
        let elf_path = mf_dir.join(format!("arity_{arity}.elf"));
        std::fs::write(
            &src_path,
            format!("dec example({params}) -> int:\n    {sum_expr}\n\ndec main() -> int:\n    example({args})\n"),
        )
        .unwrap();
        let vm_ar = Command::new(aodc_bin())
            .args(["run", &src_path.to_string_lossy()])
            .output()
            .expect("vm arity");
        assert!(vm_ar.status.success());
        assert_eq!(String::from_utf8_lossy(&vm_ar.stdout).trim(), want);
        let s2_ar = Command::new(&stage2_bin)
            .args([&src_path, &elf_path])
            .output()
            .expect("stage2 arity");
        assert!(s2_ar.status.success(), "stage2 arity {arity} failed");
        let run_ar = Command::new(&elf_path).output().expect("run arity elf");
        assert!(run_ar.status.success());
        assert_eq!(String::from_utf8_lossy(&run_ar.stdout).trim(), want);
        assert_eq!(run_ar.stdout, vm_ar.stdout);
    }

    // 6. PR #12 nominal package identity: diamond sharing passes; same-name cross-module impostor rejected
    std::fs::write(
        mf_dir.join("pkg_a.dec"),
        "struct Token:\n    v: int\n\ndec consume_a(t: Token) -> int:\n    return t.v + 10\n",
    )
    .unwrap();
    std::fs::write(
        mf_dir.join("pkg_b.dec"),
        "struct Token:\n    v: int\n\ndec make_b(x: int) -> Token:\n    return Token(x)\n",
    )
    .unwrap();
    let nominal_bad = mf_dir.join("nominal_impostor.dec");
    let nominal_bad_elf = mf_dir.join("nominal_impostor.elf");
    std::fs::write(
        &nominal_bad,
        "use pkg_a:\n    consume_a\nuse pkg_b:\n    make_b\n\ndec main() -> int:\n    return consume_a(make_b(5))\n",
    )
    .unwrap();
    let s2_imp = Command::new(&stage2_bin)
        .args([&nominal_bad, &nominal_bad_elf])
        .output()
        .expect("stage2 impostor");
    assert!(
        !s2_imp.status.success() && !nominal_bad_elf.exists(),
        "stage2 must reject cross-package nominal struct impostor"
    );

    std::fs::write(
        mf_dir.join("mid_a.dec"),
        "use net:\n    Net\n\ndec make_net(v: int) -> Net:\n    return Net(v)\n",
    )
    .unwrap();
    std::fs::write(
        mf_dir.join("mid_b.dec"),
        "use net:\n    Net\n\ndec read_net(n: Net) -> int:\n    return n.x * 3\n",
    )
    .unwrap();
    let diamond_src = mf_dir.join("diamond.dec");
    let diamond_elf = mf_dir.join("diamond.elf");
    std::fs::write(
        &diamond_src,
        "use mid_a:\n    make_net\nuse mid_b:\n    read_net\n\ndec main() -> int:\n    return read_net(make_net(11))\n",
    )
    .unwrap();
    let s2_dia = Command::new(&stage2_bin)
        .args([&diamond_src, &diamond_elf])
        .output()
        .expect("stage2 diamond");
    assert!(s2_dia.status.success(), "diamond nominal sharing must pass on stage2");
    let run_dia = Command::new(&diamond_elf).output().expect("run diamond elf");
    assert!(run_dia.status.success());
    assert_eq!(String::from_utf8_lossy(&run_dia.stdout).trim(), "33");

    // 7. Representative multi-file typed request-struct application entry (Stage 2 ELF == VM parity)
    std::fs::write(
        mf_dir.join("route_model.dec"),
        "\
struct RouteEntry:
    key: int
    weight: int

struct RouteReq:
    route: RouteEntry
    cap: int
    pulse: t1
    bias: int

dec apply_req(req: RouteReq) -> RouteEntry:
    let delta = mux(req.pulse, -2, 0, 3)
    let next_w = select((req.cap > 0), (req.route.weight + delta + req.bias), req.route.weight)
    return RouteEntry(req.route.key, next_w)
",
    )
    .unwrap();
    let app_src = mf_dir.join("route_app_entry.dec");
    let app_elf = mf_dir.join("route_app_entry.elf");
    std::fs::write(
        &app_src,
        "\
use route_model:
    RouteEntry, RouteReq, apply_req

dec score7(a: int, b: int, c: int, d: int, e: int, f: int, g: int) -> int:
    return a + b + c + d + e + f + g

dec main() -> int:
    let anchor = 00_8
    assert anchor != 0_8
    let r0 = RouteEntry(8192, -1)
    let pulse = 1 <=> 0
    let req = RouteReq(r0, 4096, pulse, 4)
    let r1 = apply_req(req)
    emit r1.key
    emit r1.weight
    return score7(r1.weight, 1, 2, 3, 4, 5, 6)
",
    )
    .unwrap();
    let vm_app = Command::new(aodc_bin())
        .args(["run", &app_src.to_string_lossy()])
        .output()
        .expect("vm route app");
    assert!(vm_app.status.success(), "vm route app failed: {}", String::from_utf8_lossy(&vm_app.stderr));
    let s2_app = Command::new(&stage2_bin)
        .args([&app_src, &app_elf])
        .output()
        .expect("stage2 route app");
    assert!(s2_app.status.success(), "stage2 route app failed");
    let run_app = Command::new(&app_elf).output().expect("run route app elf");
    assert!(run_app.status.success());
    assert_eq!(String::from_utf8_lossy(&run_app.stdout), "8192\n6\n27\n");
    assert_eq!(run_app.stdout, vm_app.stdout);

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
